use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use toge_core::config::Config;

pub struct AppState {
    socket_path: PathBuf,
    config_path: PathBuf,
    query_counter: AtomicU64,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            socket_path: crate::ipc_client::socket_path(),
            config_path: default_config_path(),
            query_counter: AtomicU64::new(1),
        }
    }

    pub fn socket_path(&self) -> PathBuf {
        self.socket_path.clone()
    }

    pub fn next_query_id(&self) -> u64 {
        self.query_counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn load_config(&self) -> Config {
        Config::load(&self.config_path).unwrap_or_else(|_| Config::default_config())
    }
}

fn default_config_path() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = env::var_os("HOME").expect("HOME not set");
            PathBuf::from(home).join(".config")
        })
        .join("toge")
        .join("config.toml")
}
