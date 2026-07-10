use crate::commands;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main-tray";
const TRAY_SHOW: &str = "tray-show";
const TRAY_NEW_WINDOW: &str = "tray-new-window";
const TRAY_TOGGLE: &str = "tray-toggle-window";
const TRAY_OPTIONS: &str = "tray-options";
const TRAY_QUIT: &str = "tray-quit";

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    if app.tray_by_id(TRAY_ID).is_some() {
        return Ok(());
    }

    let show_item = MenuItem::with_id(app, TRAY_SHOW, "Show Window", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let new_window_item = MenuItem::with_id(app, TRAY_NEW_WINDOW, "New Window", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let toggle_item = MenuItem::with_id(app, TRAY_TOGGLE, "Toggle Window", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let options_item = MenuItem::with_id(app, TRAY_OPTIONS, "Options...", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let separator = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let quit_item =
        MenuItem::with_id(app, TRAY_QUIT, "Quit", true, None::<&str>).map_err(|e| e.to_string())?;

    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &new_window_item,
            &toggle_item,
            &options_item,
            &separator,
            &quit_item,
        ],
    )
    .map_err(|e| e.to_string())?;

    let app_handle_events = app.clone();

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Toge")
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .on_tray_icon_event(move |_tray, event| {
            handle_tray_event(&app_handle_events, event);
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app).map_err(|e| e.to_string())?;
    Ok(())
}

fn handle_menu_event(app: &AppHandle, menu_id: &str) {
    let state = app.state::<crate::state::AppState>();
    let _ = match menu_id {
        TRAY_SHOW => commands::show_main_window_internal(app, &state).map(|_| ()),
        TRAY_NEW_WINDOW => commands::create_new_main_window_internal(app, &state).map(|_| ()),
        TRAY_TOGGLE => commands::toggle_main_window_internal(app, &state).map(|_| ()),
        TRAY_OPTIONS => commands::open_options_window_internal(app),
        TRAY_QUIT => {
            state.mark_exiting();
            app.exit(0);
            Ok(())
        }
        _ => Ok(()),
    };
}

fn handle_tray_event(app: &AppHandle, event: TrayIconEvent) {
    match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => {
            let state = app.state::<crate::state::AppState>();
            let _ = commands::show_main_window_internal(app, &state);
        }
        _ => {}
    }
}
