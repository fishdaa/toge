use crate::ipc_client;
use crate::keyboard::{
    KeyboardSettingsPayload, apply_settings_to_config, default_keyboard_settings,
    settings_from_config,
};
use crate::state::AppState;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use toge_core::config::Config;
use toge_core::sys::{FanotifyWatcher, FsWatcher, WatchEvent};

#[derive(serde::Serialize)]
pub struct SearchResult {
    pub rows: Vec<ResultRow>,
    pub total_count: u64,
    pub total_size: u64,
    pub size_indexed: bool,
}

#[derive(serde::Serialize)]
pub struct ResultRow {
    pub path: String,
    pub name: String,
    pub parent: String,
    pub extension: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_unix: i64,
}

#[derive(serde::Serialize)]
pub struct StatusResult {
    pub indexed_count: u64,
    pub status: String,
    pub status_message: String,
    pub size_indexed: bool,
    pub watcher_healthy: bool,
    pub watched_dir_count: u64,
    pub watch_failure_count: u64,
    pub watch_overflow_count: u64,
    pub watcher_log: Vec<String>,
    pub last_updated_unix: i64,
    pub build_duration_ms: u64,
}

#[derive(serde::Serialize)]
pub struct WatcherSelfTestResult {
    pub passed: bool,
    pub summary: String,
    pub events: Vec<String>,
}

#[tauri::command]
pub async fn search_query(
    state: State<'_, AppState>,
    query: String,
    max_results: Option<usize>,
) -> Result<SearchResult, String> {
    let socket = state.socket_path();
    let config_path = state.config_path();
    let id = state.next_query_id();
    let max = max_results.unwrap_or(10_000);

    tauri::async_runtime::spawn_blocking(move || {
        let size_indexed = Config::load(&config_path)
            .unwrap_or_else(|_| Config::default_config())
            .index_size;
        let (event_tx, _event_rx) = mpsc::channel();

        ipc_client::ensure_daemon_running(&socket).map_err(|e| e.to_string())?;
        ipc_client::wait_for_ready(&socket, Duration::from_secs(30), &event_tx)
            .map_err(|e| e.to_string())?;

        let response = ipc_client::query(&socket, id, &query, max, 0).map_err(|e| e.to_string())?;

        let rows: Vec<ResultRow> = response
            .rows
            .iter()
            .map(|row| ResultRow {
                path: row.path.clone(),
                name: row.name.clone(),
                parent: row.parent.clone(),
                extension: row.extension.clone(),
                is_dir: row.is_dir,
                size_bytes: row.size,
                modified_unix: row.modified_unix,
            })
            .collect();

        Ok(SearchResult {
            rows,
            total_count: response.total_count as u64,
            total_size: response.total_size,
            size_indexed,
        })
    })
    .await
    .map_err(|e| format!("search worker failed: {e}"))?
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Result<StatusResult, String> {
    let socket = state.socket_path();
    let config = state.load_config();

    ipc_client::ensure_daemon_running(&socket).map_err(|e| e.to_string())?;
    let status = ipc_client::status(&socket).map_err(|e| e.to_string())?;

    Ok(StatusResult {
        indexed_count: status.indexed_count as u64,
        status: format!("{:?}", status.status),
        status_message: status.status_message,
        size_indexed: config.index_size,
        watcher_healthy: status.watcher_healthy,
        watched_dir_count: status.watched_dir_count as u64,
        watch_failure_count: status.watch_failure_count as u64,
        watch_overflow_count: status.watch_overflow_count,
        watcher_log: status.watcher_log,
        last_updated_unix: status.last_updated_unix,
        build_duration_ms: status.build_duration_ms,
    })
}

#[tauri::command]
pub fn open_path(path: String) {
    crate::actions::open_path(&path);
}

#[tauri::command]
pub fn reveal_in_folder(path: String) {
    crate::actions::reveal_in_folder(&path);
}

#[tauri::command]
pub fn copy_to_clipboard(text: String) {
    crate::actions::copy_to_clipboard(&text);
}

#[tauri::command]
pub fn trash_path(path: String) -> Result<(), String> {
    crate::actions::trash_path(&path)
}

#[tauri::command]
pub fn delete_path(path: String) -> Result<(), String> {
    crate::actions::delete_path(&path)
}

#[tauri::command]
pub fn reindex_index(state: State<'_, AppState>) -> Result<(), String> {
    let socket = state.socket_path();
    ipc_client::ensure_daemon_running(&socket).map_err(|e| e.to_string())?;
    ipc_client::reindex(&socket).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_keyboard_settings(
    state: State<'_, AppState>,
) -> Result<KeyboardSettingsPayload, String> {
    let config = state.load_config();
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn save_keyboard_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: KeyboardSettingsPayload,
) -> Result<KeyboardSettingsPayload, String> {
    let mut config = state.load_config();
    let normalized = apply_settings_to_config(&mut config, settings)?;
    state.save_config(&config)?;
    crate::global_hotkeys::register_window_hotkeys(&app, &normalized)?;
    app.emit("keyboard-settings-updated", &normalized)
        .map_err(|e| e.to_string())?;
    Ok(normalized)
}

#[tauri::command]
pub fn restore_default_keyboard_settings() -> Result<KeyboardSettingsPayload, String> {
    Ok(default_keyboard_settings())
}

#[tauri::command]
pub fn run_watcher_self_test() -> Result<WatcherSelfTestResult, String> {
    #[cfg(not(target_os = "linux"))]
    {
        return Ok(WatcherSelfTestResult {
            passed: false,
            summary: "Watcher self-test is only available on Linux".to_string(),
            events: vec![],
        });
    }

    #[cfg(target_os = "linux")]
    {
        let mut watcher =
            FanotifyWatcher::new().map_err(|e| format!("watcher init failed: {}", e))?;

        let test_dir = make_watcher_test_dir().map_err(|e| format!("temp dir failed: {}", e))?;
        let test_file = test_dir.join("watcher-self-test.mkv");
        let test_file_str = test_file.to_string_lossy().to_string();

        let outcome = (|| -> Result<WatcherSelfTestResult, String> {
            watcher
                .watch(&test_dir)
                .map_err(|e| format!("watch failed: {}", e))?;

            fs::write(&test_file, b"self-test").map_err(|e| format!("create failed: {}", e))?;
            let create_events = wait_for_events(&mut watcher, Duration::from_secs(2))
                .map_err(|e| format!("create poll failed: {}", e))?;

            fs::remove_file(&test_file).map_err(|e| format!("delete failed: {}", e))?;
            let delete_events = wait_for_events(&mut watcher, Duration::from_secs(2))
                .map_err(|e| format!("delete poll failed: {}", e))?;

            let mut event_lines = Vec::new();
            let mut saw_create = false;
            let mut saw_delete = false;

            for event in create_events.into_iter().chain(delete_events) {
                event_lines.push(format_watch_event(&event));
                match event {
                    WatchEvent::Create {
                        path,
                        is_dir: false,
                    } if path == test_file_str => {
                        saw_create = true;
                    }
                    WatchEvent::Delete { path } if path == test_file_str => {
                        saw_delete = true;
                    }
                    _ => {}
                }
            }

            let passed = saw_create && saw_delete;
            let summary = if passed {
                "Watcher self-test passed: create and delete events observed".to_string()
            } else {
                format!(
                    "Watcher self-test failed: create seen = {}, delete seen = {}",
                    saw_create, saw_delete
                )
            };

            Ok(WatcherSelfTestResult {
                passed,
                summary,
                events: event_lines,
            })
        })();

        let _ = fs::remove_file(&test_file);
        let _ = fs::remove_dir_all(&test_dir);

        outcome
    }
}

#[cfg(target_os = "linux")]
fn make_watcher_test_dir() -> std::io::Result<PathBuf> {
    let base = std::env::temp_dir();
    let unique = format!(
        "toge-watcher-self-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let dir = base.join(unique);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "linux")]
fn wait_for_events(
    watcher: &mut FanotifyWatcher,
    timeout: Duration,
) -> std::io::Result<Vec<WatchEvent>> {
    let deadline = std::time::Instant::now() + timeout;
    let mut all = Vec::new();

    while std::time::Instant::now() < deadline {
        let events = watcher.poll_events()?;
        if !events.is_empty() {
            all.extend(events);
            return Ok(all);
        }
        thread::sleep(Duration::from_millis(25));
    }

    Ok(all)
}

#[cfg(target_os = "linux")]
fn format_watch_event(event: &WatchEvent) -> String {
    match event {
        WatchEvent::Create { path, is_dir } => {
            format!("create {}{}", path, if *is_dir { " (dir)" } else { "" })
        }
        WatchEvent::Delete { path } => format!("delete {}", path),
        WatchEvent::Modify { path } => format!("modify {}", path),
        WatchEvent::Move { from, to } => format!("move {} -> {}", from, to),
        WatchEvent::Overflow { path } => format!("overflow {}", path),
    }
}

#[tauri::command]
pub async fn open_debug_window(app: tauri::AppHandle) -> Result<(), String> {
    open_debug_window_internal(&app)
}

pub(crate) fn open_debug_window_internal(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("debug") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(app, "debug", WebviewUrl::default())
        .title("Toge Debug")
        .inner_size(760.0, 560.0)
        .min_inner_size(520.0, 360.0)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn open_options_window(app: tauri::AppHandle) -> Result<(), String> {
    open_options_window_internal(&app)
}

pub(crate) fn open_options_window_internal(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("options") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(app, "options", WebviewUrl::default())
        .title("Everything Options")
        .inner_size(720.0, 560.0)
        .min_inner_size(640.0, 480.0)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn close_options_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_new_main_window(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    create_new_main_window_internal(&app, &state)
}

pub(crate) fn create_new_main_window_internal(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    let label = if app.get_webview_window("main").is_none() {
        "main".to_string()
    } else {
        format!("main-{}", state.next_window_id())
    };

    build_main_window(app, &label)?;
    Ok(label)
}

#[tauri::command]
pub async fn show_main_window(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    show_main_window_internal(&app, &state)
}

pub(crate) fn show_main_window_internal(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    if let Some(window) = first_main_window(app) {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(window.label().to_string());
    }

    let label = if app.get_webview_window("main").is_none() {
        "main".to_string()
    } else {
        format!("main-{}", state.next_window_id())
    };
    build_main_window(app, &label)?;
    Ok(label)
}

#[tauri::command]
pub async fn toggle_main_window(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    toggle_main_window_internal(&app, &state)
}

pub(crate) fn toggle_main_window_internal(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    for window in app.webview_windows().values() {
        if is_main_window_label(window.label()) {
            let is_visible = window.is_visible().map_err(|e| e.to_string())?;
            if is_visible {
                window.hide().map_err(|e| e.to_string())?;
                return Ok(window.label().to_string());
            }
        }
    }

    if let Some(window) = first_main_window(app) {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(window.label().to_string());
    }

    let label = if app.get_webview_window("main").is_none() {
        "main".to_string()
    } else {
        format!("main-{}", state.next_window_id())
    };
    build_main_window(app, &label)?;
    Ok(label)
}

pub(crate) fn handle_main_window_close_requested(
    window: &tauri::Window,
    event: &WindowEvent,
) -> bool {
    if !is_main_window_label(window.label()) {
        return false;
    }

    if let WindowEvent::CloseRequested { api, .. } = event {
        let state = window.app_handle().state::<AppState>();
        if state.is_exiting() {
            return false;
        }

        api.prevent_close();
        let _ = window.hide();
        return true;
    }

    false
}

fn build_main_window(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    if app.get_webview_window(label).is_some() {
        return Ok(());
    }

    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .title("Toge")
        .inner_size(960.0, 640.0)
        .min_inner_size(480.0, 320.0)
        .resizable(true)
        .decorations(true)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn first_main_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window("main") {
        return Some(window);
    }

    app.webview_windows()
        .iter()
        .find_map(|(label, window)| is_main_window_label(label).then(|| window.clone()))
}

fn is_main_window_label(label: &str) -> bool {
    label == "main" || label.starts_with("main-")
}
