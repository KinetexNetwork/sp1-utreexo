use anyhow;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::select;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::task;
use tokio_util::sync::CancellationToken;

use crate::{builder, updater};

/// Commands accepted by the service.
#[derive(Debug, Clone)]
pub enum Command {
    Build {
        parquet: String,
        resume_from: Option<String>,
    },
    Update(u64),
    Pause,
    Resume,
    Stop,
    Dump {
        dir: PathBuf,
    },
    Restore {
        dir: PathBuf,
    },
}

/// Public state as exposed via the REST API.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ServiceState {
    Idle,
    Building,
    Updating { height: u64 },
    Paused,
    Error { msg: String },
}

#[derive(Clone, Serialize)]
pub struct Status {
    pub state: ServiceState,
    pub uptime_secs: u64,
}

/// Internally tracked long-running task so we can cancel / resume.
#[derive(Clone)]
enum JobKind {
    Build {
        parquet: String,
        resume_from: Option<String>,
    },
    Update(u64),
}

struct RunningJob {
    cancel: CancellationToken,
    join: task::JoinHandle<anyhow::Result<()>>, // finished result
    kind: JobKind,
}

/// Main handle used by HTTP layer.
#[derive(Clone)]
pub struct Context {
    state: Arc<RwLock<ServiceState>>,
    start: std::time::Instant,
    tx: mpsc::Sender<Command>,
}

#[derive(Debug)]
pub enum DispatchError {
    InvalidState,
    ChannelClosed,
}

impl Context {
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<Command>(8);
        let tx_bg = tx.clone();
        let state = Arc::new(RwLock::new(ServiceState::Idle));
        let state_bg = state.clone();
        let fs_lock = Arc::new(Mutex::new(()));

        task::spawn(async move {
            let mut running: Option<RunningJob> = None;
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    // =========== BUILD ============
                    Command::Build {
                        parquet,
                        resume_from,
                    } => {
                        if running.is_some() {
                            // reject – already busy
                            continue;
                        }
                        *state_bg.write().await = ServiceState::Building;
                        let cancel = CancellationToken::new();
                        let task_cancel = cancel.clone();

                        // clone for storage & move into async
                        let parquet_clone = parquet.clone();
                        let resume_clone = resume_from.clone();

                        let handle = task::spawn(async move {
                            run_with_cancel(task_cancel, async move {
                                builder::start_build(&parquet, resume_from.as_deref()).await
                            })
                            .await
                        });
                        running = Some(RunningJob {
                            cancel,
                            join: handle,
                            kind: JobKind::Build {
                                parquet: parquet_clone,
                                resume_from: resume_clone,
                            },
                        });
                    }
                    // =========== UPDATE ============
                    Command::Update(h) => {
                        if running.is_some() {
                            continue;
                        }
                        *state_bg.write().await = ServiceState::Updating { height: h };
                        let cancel = CancellationToken::new();
                        let task_cancel = cancel.clone();
                        let handle = task::spawn(async move {
                            run_with_cancel(
                                task_cancel,
                                async move { updater::update_block(h).await },
                            )
                            .await
                        });
                        running = Some(RunningJob {
                            cancel,
                            join: handle,
                            kind: JobKind::Update(h),
                        });
                    }
                    // =========== PAUSE ============
                    Command::Pause => {
                        if let Some(job) = &running {
                            job.cancel.cancel();
                        }
                        *state_bg.write().await = ServiceState::Paused;
                    }
                    // =========== RESUME ============
                    Command::Resume => {
                        if *state_bg.read().await != ServiceState::Paused {
                            continue;
                        }
                        if let Some(prev) = running.take() {
                            match prev.kind.clone() {
                                JobKind::Build {
                                    parquet,
                                    resume_from,
                                } => {
                                    let _ = tx_bg
                                        .send(Command::Build {
                                            parquet,
                                            resume_from,
                                        })
                                        .await;
                                }
                                JobKind::Update(h) => {
                                    let _ = tx_bg.send(Command::Update(h)).await;
                                }
                            }
                        }
                    }
                    // =========== STOP ============
                    Command::Stop => {
                        if let Some(job) = &running {
                            job.cancel.cancel();
                        }
                        running = None;
                        *state_bg.write().await = ServiceState::Idle;
                    }
                    // =========== DUMP ============
                    Command::Dump { dir } => {
                        // Run dump synchronously (block on dump completion) under fs_lock
                        let lock = fs_lock.clone();
                        let st = state_bg.clone();
                        let dir_clone = dir.clone();
                        // Acquire lock
                        let _g = lock.lock().await;
                        // Perform dump
                        if let Err(e) = state_helpers::perform_dump(dir_clone).await {
                            *st.write().await = ServiceState::Error { msg: e.to_string() };
                        }
                    }
                    // =========== RESTORE ============
                    Command::Restore { dir } => {
                        // Cancel any running job and mark as restoring
                        if let Some(job) = &running {
                            job.cancel.cancel();
                            running = None;
                        }
                        // Mark service busy for restore so wait_until_idle blocks until complete
                        *state_bg.write().await = ServiceState::Updating { height: 0 };
                        let lock = fs_lock.clone();
                        let st = state_bg.clone();
                        // Execute restore synchronously under lock
                        let _g = lock.lock().await;
                        match state_helpers::perform_restore(dir).await {
                            Ok(_) => *st.write().await = ServiceState::Idle,
                            Err(e) => {
                                *st.write().await = ServiceState::Error { msg: e.to_string() }
                            }
                        }
                    }
                }

                // poll finished job (non-blocking)
                if running
                    .as_ref()
                    .map(|j| j.join.is_finished())
                    .unwrap_or(false)
                {
                    // Safe to unwrap because checked above
                    let job = running.take().unwrap();
                    match job.join.await {
                        Ok(Ok(_)) => *state_bg.write().await = ServiceState::Idle,
                        Ok(Err(e)) => {
                            *state_bg.write().await = ServiceState::Error { msg: e.to_string() }
                        }
                        Err(e) => {
                            *state_bg.write().await = ServiceState::Error {
                                msg: format!("join error: {e}"),
                            }
                        }
                    }
                }
            }
        });

        Context {
            state,
            start: std::time::Instant::now(),
            tx,
        }
    }

    /// Validate transition and enqueue command to background worker.
    pub async fn send(&self, cmd: Command) -> Result<(), DispatchError> {
        // Ensure command is valid in current state
        if !self.is_valid_transition(&cmd).await {
            return Err(DispatchError::InvalidState);
        }
        // Handle Restore synchronously: apply snapshot immediately
        if let Command::Restore { dir } = &cmd {
            // mark service busy for restore
            *self.state.write().await = ServiceState::Updating { height: 0 };
            // perform restore from snapshot directory
            match state_helpers::restore_sync(dir.clone()) {
                Ok(_) => *self.state.write().await = ServiceState::Idle,
                Err(e) => *self.state.write().await = ServiceState::Error { msg: e.to_string() },
            }
            return Ok(());
        }
        // Dispatch other commands to the background worker
        self.tx
            .send(cmd)
            .await
            .map_err(|_| DispatchError::ChannelClosed)
    }

    pub async fn status(&self) -> Status {
        Status {
            uptime_secs: self.start.elapsed().as_secs(),
            state: self.state.read().await.clone(),
        }
    }

    async fn is_valid_transition(&self, cmd: &Command) -> bool {
        let state = self.state.read().await.clone();
        matches!(
            (state, cmd),
            (ServiceState::Idle, Command::Build { .. })
                | (ServiceState::Idle, Command::Update(_))
                | (ServiceState::Idle, Command::Dump { .. })
                | (ServiceState::Idle, Command::Restore { .. })
                | (ServiceState::Building, Command::Pause)
                | (ServiceState::Building, Command::Stop)
                | (ServiceState::Updating { .. }, Command::Pause)
                | (ServiceState::Updating { .. }, Command::Stop)
                | (ServiceState::Paused, Command::Resume)
                | (ServiceState::Paused, Command::Stop)
                | (ServiceState::Error { .. }, Command::Restore { .. })
        )
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// helper util fn: run future until cancel fires
// ------------------------------------------------------------------
async fn run_with_cancel<F>(cancel: CancellationToken, fut: F) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>>,
{
    select! {
        _ = cancel.cancelled() => Ok(()),
        res = fut => res,
    }
}

// ------------------------------------------------------------------
// extract dump / restore helpers from earlier phase (reuse)
// ------------------------------------------------------------------

mod state_helpers {
    use std::io::{Error, ErrorKind};
    use std::path::PathBuf;

    /// Copy snapshot plus derive pollard (same as Phase-A implementation).
    pub fn dump_sync(dir: PathBuf) -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        std::fs::copy("mem_forest.bin", dir.join("mem_forest.bin"))?;
        if std::path::Path::new("block_hashes.bin").exists() {
            let _ = std::fs::copy("block_hashes.bin", dir.join("block_hashes.bin"));
        }
        // Ensure snapshot directory exists
        std::fs::create_dir_all(&dir)?;
        // Copy full MemForest snapshot
        std::fs::copy("mem_forest.bin", dir.join("mem_forest.bin"))?;
        // Copy block_hashes.bin if present
        if std::path::Path::new("block_hashes.bin").exists() {
            let _ = std::fs::copy("block_hashes.bin", dir.join("block_hashes.bin"));
        }
        // For now, snapshot pollard as same bytes as MemForest (stub)
        std::fs::copy("mem_forest.bin", dir.join("pollard.bin"))?;
        Ok(())
    }

    pub fn restore_sync(dir: PathBuf) -> std::io::Result<()> {
        let forest_src = dir.join("mem_forest.bin");
        let pollard_src = dir.join("pollard.bin");
        if !forest_src.exists() || !pollard_src.exists() {
            return Err(Error::new(ErrorKind::NotFound, "snapshot missing files"));
        }
        std::fs::copy(&forest_src, "mem_forest.bin")?;
        std::fs::copy(&pollard_src, "pollard.bin")?;
        let bh = dir.join("block_hashes.bin");
        if bh.exists() {
            let _ = std::fs::copy(bh, "block_hashes.bin");
        }
        Ok(())
    }

    pub async fn perform_dump(dir: PathBuf) -> std::io::Result<()> {
        tokio::task::spawn_blocking(move || dump_sync(dir)).await?
    }

    pub async fn perform_restore(dir: PathBuf) -> std::io::Result<()> {
        tokio::task::spawn_blocking(move || restore_sync(dir)).await?
    }
}
