use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::select;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::task;
use tokio_util::sync::CancellationToken;
use anyhow;

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
    Dump { dir: PathBuf },
    Restore { dir: PathBuf },
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
    join:   task::JoinHandle<anyhow::Result<()>>, // finished result
    kind:   JobKind,
}

/// Main handle used by HTTP layer.
#[derive(Clone)]
pub struct Context {
    state: Arc<RwLock<ServiceState>>,
    start: std::time::Instant,
    tx:    mpsc::Sender<Command>,
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
                    Command::Build { parquet, resume_from } => {
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
                            run_with_cancel(task_cancel, async move { updater::update_block(h).await })
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
                                JobKind::Build { parquet, resume_from } => {
                                    let _ = tx_bg.send(Command::Build { parquet, resume_from }).await;
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
                        // Serialised via fs_lock
                        let lock = fs_lock.clone();
                        let st = state_bg.clone();
                        task::spawn(async move {
                            let _g = lock.lock().await;
                            let res = state_helpers::perform_dump(dir).await;
                            if let Err(e) = res {
                                *st.write().await = ServiceState::Error { msg: e.to_string() };
                            }
                        });
                    }
                    // =========== RESTORE ============
                    Command::Restore { dir } => {
                        let lock = fs_lock.clone();
                        let st = state_bg.clone();
                        if let Some(job) = &running {
                            job.cancel.cancel();
                            running = None;
                        }
                        *st.write().await = ServiceState::Idle;
                        task::spawn(async move {
                            let _g = lock.lock().await;
                            let res = state_helpers::perform_restore(dir).await;
                            if let Err(e) = res {
                                *st.write().await = ServiceState::Error { msg: e.to_string() };
                            }
                        });
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
                        Err(e) => *state_bg.write().await = ServiceState::Error {
                            msg: format!("join error: {e}")
                        },
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

    pub async fn send(&self, cmd: Command) -> Result<(), mpsc::error::SendError<Command>> {
        self.tx.send(cmd).await
    }

    pub async fn status(&self) -> Status {
        Status {
            uptime_secs: self.start.elapsed().as_secs(),
            state: self.state.read().await.clone(),
        }
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
    use std::path::PathBuf;
    use std::io::{Error, ErrorKind};

    /// Copy snapshot plus derive pollard (same as Phase-A implementation).
    pub fn dump_sync(dir: PathBuf) -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        std::fs::copy("mem_forest.bin", dir.join("mem_forest.bin"))?;
        if std::path::Path::new("block_hashes.bin").exists() {
            let _ = std::fs::copy("block_hashes.bin", dir.join("block_hashes.bin"));
        }
        let forest_bytes = std::fs::read("mem_forest.bin")?;
        let pollard = utreexo_script::pollard::forest_to_pollard(&forest_bytes, &[])
            .map_err(|e| Error::new(ErrorKind::Other, e))?;
        let mut f = std::fs::File::create(dir.join("pollard.bin"))?;
        pollard.serialize(&mut f).map_err(|e| Error::new(ErrorKind::Other, e))?;
        Ok(())
    }

    pub fn restore_sync(dir: PathBuf) -> std::io::Result<()> {
        let forest_src = dir.join("mem_forest.bin");
        let pollard_src = dir.join("pollard.bin");
        if !forest_src.exists() || !pollard_src.exists() {
            return Err(Error::new(ErrorKind::NotFound, "snapshot missing files"));
        }
        let _ = std::fs::remove_file("mem_forest.bin");
        let _ = std::fs::remove_file("pollard.bin");
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