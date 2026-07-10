use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use toge_core::config::Config;

pub struct AppState {
    socket_path: PathBuf,
    config_path: PathBuf,
    query_counter: AtomicU64,
    window_counter: AtomicU64,
    exiting: AtomicBool,
    pressed_window_hotkeys: AtomicU8,
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
            window_counter: AtomicU64::new(1),
            exiting: AtomicBool::new(false),
            pressed_window_hotkeys: AtomicU8::new(0),
        }
    }

    pub fn socket_path(&self) -> PathBuf {
        self.socket_path.clone()
    }

    pub fn next_query_id(&self) -> u64 {
        self.query_counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn next_window_id(&self) -> u64 {
        self.window_counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn mark_exiting(&self) {
        self.exiting.store(true, Ordering::SeqCst);
    }

    pub fn is_exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }

    pub fn press_window_hotkey(&self, mask: u8) -> bool {
        self.pressed_window_hotkeys.fetch_or(mask, Ordering::SeqCst) & mask == 0
    }

    pub fn release_window_hotkey(&self, mask: u8) {
        self.pressed_window_hotkeys
            .fetch_and(!mask, Ordering::SeqCst);
    }

    pub fn reset_window_hotkeys(&self) {
        self.pressed_window_hotkeys.store(0, Ordering::SeqCst);
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_path.clone()
    }

    pub fn load_config(&self) -> Config {
        Config::load(&self.config_path).unwrap_or_else(|_| Config::default_config())
    }

    pub fn save_config(&self, config: &Config) -> Result<(), String> {
        config.save(&self.config_path)
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;

    #[test]
    fn window_hotkeys_are_edge_triggered_without_a_time_delay() {
        let state = AppState::new();

        assert!(state.press_window_hotkey(0b001));
        assert!(!state.press_window_hotkey(0b001));

        state.release_window_hotkey(0b001);
        assert!(state.press_window_hotkey(0b001));

        state.reset_window_hotkeys();
        assert!(state.press_window_hotkey(0b001));
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
