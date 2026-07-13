pub mod actions;
pub mod commands;
pub mod format;
pub mod global_hotkeys;
pub mod ipc_client;
pub mod keyboard;
pub mod state;
pub mod tray;

use state::AppState;
use std::io;
use tauri::WindowEvent;

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .setup(|app| {
            crate::tray::initialize(app.handle()).map_err(io::Error::other)?;
            crate::global_hotkeys::initialize(app.handle()).map_err(io::Error::other)?;
            Ok(())
        })
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                let _ = crate::commands::handle_main_window_close_requested(window, event);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::window_ready,
            commands::search_query,
            commands::get_status,
            commands::open_path,
            commands::reveal_in_folder,
            commands::copy_to_clipboard,
            commands::trash_path,
            commands::delete_path,
            commands::reindex_index,
            commands::run_watcher_self_test,
            commands::open_debug_window,
            commands::open_options_window,
            commands::close_options_window,
            commands::create_new_main_window,
            commands::show_main_window,
            commands::toggle_main_window,
            commands::get_keyboard_settings,
            commands::save_keyboard_settings,
            commands::restore_default_keyboard_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
