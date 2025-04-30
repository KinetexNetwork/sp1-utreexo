use serde::Serialize;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock, Mutex};
use tokio::task;
use std::time::Instant;



use std::path::PathBuf;

/// Commands sent to the service worker.
#[derive(Debug)]
pub enum Command {
    Build { parquet: String, resume_from: Option<String> },
    Pause,
    Resume,
    Stop,
    Update(u64),
    /// Create a snapshot of the current accumulator into the given directory.
    Dump { dir: PathBuf },
    /// Restore the in-memory snapshot **and** on-disk files from the given directory.
    Restore { dir: PathBuf },
}

/// Public state of the service.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ServiceState {
    Idle,
    Building,
    Paused,
    Updating { height: u64 },
    Error { message: String },
}

/// Simple status snapshot.
#[derive(Serialize)]
pub struct Status {
    pub state: ServiceState,
    pub uptime_secs: u64,
}

/// Shared context for the accumulator service.
#[derive(Clone)]
pub struct Context {
    state: Arc<RwLock<ServiceState>>,
    start_time: Instant,
    tx: mpsc::Sender<Command>,
    // Global mutex to serialise any on-disk snapshot mutation.
    fs_lock: Arc<Mutex<()>>, 
}

impl Context {
    /// Create and start the background worker.
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::channel(8);
        let state = Arc::new(RwLock::new(ServiceState::Idle));
        let state_bg = state.clone();
        let fs_lock = Arc::new(Mutex::new(()));
        let fs_lock_bg = fs_lock.clone();
        // spawn worker
        task::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                Command::Dump { dir } => {
                    let state_clone = state_bg.clone();
                    let fs_lock = fs_lock_bg.clone();
                    task::spawn(async move {
                        let res = Self::perform_dump(dir, fs_lock).await;
                        let mut s = state_clone.write().await;
                        match res {
                            Ok(_) => *s = ServiceState::Idle,
                            Err(e) => *s = ServiceState::Error { message: e.to_string() },
                        }
                    });
                }
                Command::Restore { dir } => {
                    let state_clone = state_bg.clone();
                    let fs_lock = fs_lock_bg.clone();
                    task::spawn(async move {
                        let res = Self::perform_restore(dir, fs_lock).await;
                        let mut s = state_clone.write().await;
                        match res {
                            Ok(_) => *s = ServiceState::Idle,
                            Err(e) => *s = ServiceState::Error { message: e.to_string() },
                        }
                    });
                }
                Command::Build { parquet, resume_from } => {
                    // Enter building state
                    {
                        let mut s = state_bg.write().await;
                        *s = ServiceState::Building;
                    }
                    // Spawn build task
                    let state_clone = state_bg.clone();
                    task::spawn(async move {
                        // Call the builder logic
                        let res = crate::builder::start_build(&parquet, resume_from.as_deref()).await;
                        let mut s = state_clone.write().await;
                        match res {
                            Ok(_) => *s = ServiceState::Idle,
                            Err(e) => *s = ServiceState::Error { message: e.to_string() },
                        }
                    });
                }
                Command::Pause => {
                    let mut s = state_bg.write().await;
                    *s = ServiceState::Paused;
                }
                Command::Resume => {
                    let mut s = state_bg.write().await;
                    *s = ServiceState::Building;
                }
                Command::Stop => {
                    let mut s = state_bg.write().await;
                    *s = ServiceState::Idle;
                }
                Command::Update(h) => {
                    // Enter updating state and spawn update task
                    {
                        let mut s = state_bg.write().await;
                        *s = ServiceState::Updating { height: h };
                    }
                    let state_clone = state_bg.clone();
                    task::spawn(async move {
                        let res = crate::updater::update_block(h).await;
                        let mut s = state_clone.write().await;
                        match res {
                            Ok(_) => *s = ServiceState::Idle,
                            Err(e) => *s = ServiceState::Error { message: e.to_string() },
                        }
                    });
                }
                // All command variants are handled explicitly above
                }
            }
        });
        Context { state, start_time: Instant::now(), tx, fs_lock }
    }

    /// Send a command to the worker.
    pub async fn send(&self, cmd: Command) -> Result<(), mpsc::error::SendError<Command>> {
        self.tx.send(cmd).await
    }

    /// Get a snapshot of the current status.
    pub async fn status(&self) -> Status {
        let s = self.state.read().await.clone();
        let uptime_secs = self.start_time.elapsed().as_secs();
        Status { state: s, uptime_secs }
    }

    // ------------------------------------------------------------------
    // Internal helpers (dump / restore) – Phase-A implementation
    // ------------------------------------------------------------------

    async fn perform_dump(dir: PathBuf, fs_lock: Arc<Mutex<()>>) -> std::io::Result<()> {
        // hold lock until function returns to avoid races
        let _guard = fs_lock.lock().await;

        // The heavy IO runs in blocking thread so we don't stall the async runtime.
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            std::fs::create_dir_all(&dir)?;

            // 1. Copy current mem_forest snapshot
            std::fs::copy("mem_forest.bin", dir.join("mem_forest.bin"))?;

            // 2. Copy block_hashes.bin if it exists
            if std::path::Path::new("block_hashes.bin").exists() {
                let _ = std::fs::copy("block_hashes.bin", dir.join("block_hashes.bin"));
            }

            // 3. Build pollard from current forest and store
            let forest_bytes = std::fs::read("mem_forest.bin")?;
            let pollard = utreexo_script::pollard::forest_to_pollard(&forest_bytes, &[])
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            let mut f = std::fs::File::create(dir.join("pollard.bin"))?;
            pollard
                .serialize(&mut f)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            Ok(())
        })
        .await?
    }

    async fn perform_restore(dir: PathBuf, fs_lock: Arc<Mutex<()>>) -> std::io::Result<()> {
        let _guard = fs_lock.lock().await;

        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let forest_src = dir.join("mem_forest.bin");
            let pollard_src = dir.join("pollard.bin");

            if !forest_src.exists() || !pollard_src.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "snapshot missing mem_forest.bin or pollard.bin",
                ));
            }

            // Overwrite destination files atomically by first removing them.
            let _ = std::fs::remove_file("mem_forest.bin");
            let _ = std::fs::remove_file("pollard.bin");
            std::fs::copy(&forest_src, "mem_forest.bin")?;
            std::fs::copy(&pollard_src, "pollard.bin")?;

            let bh_src = dir.join("block_hashes.bin");
            if bh_src.exists() {
                let _ = std::fs::copy(bh_src, "block_hashes.bin");
            }
            Ok(())
        })
        .await?
    }
}