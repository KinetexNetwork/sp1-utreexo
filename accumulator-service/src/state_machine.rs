use serde::Serialize;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task;
use std::time::Instant;

/// Commands sent to the service worker.
#[derive(Debug)]
pub enum Command {
    Build { parquet: String, resume_from: Option<String> },
    Pause,
    Resume,
    Stop,
    Update(u64),
    Dump,
    Restore(Vec<u8>),
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
}

impl Context {
    /// Create and start the background worker.
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::channel(8);
        let state = Arc::new(RwLock::new(ServiceState::Idle));
        let state_bg = state.clone();
        // spawn worker
        task::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                Command::Dump => {
                    // Spawn pollard prune task
                    let state_clone = state_bg.clone();
                    task::spawn(async move {
                        // stub: prune the forest with placeholder delete list
                        let res = crate::pollard::prune_forest("mem_forest.bin", "delete_list").await;
                        let mut s = state_clone.write().await;
                        match res {
                            Ok(_) => *s = ServiceState::Idle,
                            Err(e) => *s = ServiceState::Error { message: e.to_string() },
                        }
                    });
                }
                Command::Restore(data) => {
                    // stub: restore state from uploaded data
                    let mut s = state_bg.write().await;
                    *s = ServiceState::Idle;
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
                    _ => {}
                }
            }
        });
        Context { state, start_time: Instant::now(), tx }
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
}