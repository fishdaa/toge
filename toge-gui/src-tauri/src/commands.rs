use crate::format;
use crate::ipc_client;
use crate::state::AppState;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

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
    pub size: String,
    pub modified: String,
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
    pub last_updated_unix: i64,
    pub build_duration_ms: u64,
}

#[tauri::command]
pub fn search_query(
    state: State<'_, AppState>,
    query: String,
    max_results: Option<usize>,
) -> Result<SearchResult, String> {
    let socket = state.socket_path();
    let config = state.load_config();
    let id = state.next_query_id();
    let max = max_results.unwrap_or(10_000);
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
            size: if config.index_size {
                format::format_size(row.size)
            } else {
                "—".to_string()
            },
            modified: format::format_time(row.modified_unix),
        })
        .collect();

    Ok(SearchResult {
        rows,
        total_count: response.total_count as u64,
        total_size: response.total_size,
        size_indexed: config.index_size,
    })
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
pub fn reindex_index(state: State<'_, AppState>) -> Result<(), String> {
    let socket = state.socket_path();
    ipc_client::ensure_daemon_running(&socket).map_err(|e| e.to_string())?;
    ipc_client::reindex(&socket).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_debug_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("debug") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(&app, "debug", WebviewUrl::default())
        .title("Toge Debug")
        .inner_size(760.0, 560.0)
        .min_inner_size(520.0, 360.0)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}
