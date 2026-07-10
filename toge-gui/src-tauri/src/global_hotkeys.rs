use crate::commands;
use crate::keyboard::KeyboardSettingsPayload;
use crate::state::AppState;
use std::str::FromStr;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let config = state.load_config();
    let settings = crate::keyboard::settings_from_config(&config);
    // Keep these registered across focus changes. A shortcut callback may focus
    // the main window, so unregistering from that focus event can re-enter the
    // global-shortcut manager while it is still dispatching the callback.
    register_window_hotkeys(app, &settings)
}

pub fn register_window_hotkeys(
    app: &AppHandle,
    settings: &KeyboardSettingsPayload,
) -> Result<(), String> {
    let manager = app.global_shortcut();
    app.state::<AppState>().reset_window_hotkeys();
    manager.unregister_all().map_err(|e| e.to_string())?;

    for (action, accelerator) in [
        (WindowHotkeyAction::NewWindow, settings.new_window_hotkey.as_str()),
        (WindowHotkeyAction::ShowWindow, settings.show_window_hotkey.as_str()),
        (WindowHotkeyAction::ToggleWindow, settings.toggle_window_hotkey.as_str()),
    ] {
        if accelerator.is_empty() {
            continue;
        }

        let shortcut = Shortcut::from_str(accelerator).map_err(|e| e.to_string())?;
        manager
            .on_shortcut(shortcut, move |app, _shortcut, event| {
                handle_shortcut_event(app, action, event);
            })
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn handle_shortcut_event(app: &AppHandle, action: WindowHotkeyAction, event: ShortcutEvent) {
    let state = app.state::<AppState>();
    if event.state() == ShortcutState::Released {
        state.release_window_hotkey(action.mask());
        return;
    }

    if event.state() != ShortcutState::Pressed {
        return;
    }
    if !state.press_window_hotkey(action.mask()) {
        return;
    }

    let _ = match action {
        WindowHotkeyAction::NewWindow => commands::create_new_main_window_internal(app, &state),
        WindowHotkeyAction::ShowWindow => commands::show_main_window_internal(app, &state),
        WindowHotkeyAction::ToggleWindow => commands::toggle_main_window_internal(app, &state),
    };
}

#[derive(Clone, Copy, Debug)]
enum WindowHotkeyAction {
    NewWindow,
    ShowWindow,
    ToggleWindow,
}

impl WindowHotkeyAction {
    fn mask(self) -> u8 {
        match self {
            Self::NewWindow => 0b001,
            Self::ShowWindow => 0b010,
            Self::ToggleWindow => 0b100,
        }
    }
}
