use crate::models::Settings;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::Semaphore;

pub struct AppState {
    pub root: PathBuf,
    pub settings: RwLock<Settings>,
    pub client: reqwest::Client,
    /// Download concurrency limit
    #[allow(dead_code)]
    pub semaphore: Arc<Semaphore>,
    /// Currently running game process (if any)
    pub game_process: Arc<Mutex<Option<tokio::process::Child>>>,
    /// Which instance is currently launching / running
    pub running_instance: Arc<Mutex<Option<String>>>,
    /// Monotonic task counter for event correlation
    pub task_counter: AtomicU64,
    /// Cancel flag for the active install task
    pub install_cancel: Arc<AtomicBool>,
    /// Pending Microsoft device-code flow (if any)
    pub ms_flow: Arc<Mutex<Option<crate::models::MsFlow>>>,
    /// Cached Java detection results (unix seconds, list)
    pub java_cache: Mutex<Option<(u64, Vec<crate::models::JavaInfo>)>>,
}

impl AppState {
    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn libraries_dir(&self) -> PathBuf {
        self.root.join("libraries")
    }
    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }
    #[allow(dead_code)]
    pub fn assets_indexes_dir(&self) -> PathBuf {
        self.root.join("assets").join("indexes")
    }
    #[allow(dead_code)]
    pub fn assets_objects_dir(&self) -> PathBuf {
        self.root.join("assets").join("objects")
    }
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    #[allow(dead_code)]
    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn accounts_path(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    pub fn next_task_id(&self) -> u64 {
        self.task_counter.fetch_add(1, Ordering::Relaxed)
    }
}
