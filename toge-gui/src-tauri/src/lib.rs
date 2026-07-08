pub mod actions;
pub mod commands;
pub mod format;
pub mod ipc_client;
pub mod state;

use state::AppState;

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::search_query,
            commands::get_status,
            commands::open_path,
            commands::reveal_in_folder,
            commands::copy_to_clipboard,
            commands::reindex_index,
            commands::open_debug_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
