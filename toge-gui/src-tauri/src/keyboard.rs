use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use toge_core::config::{Config, KeyboardConfig, KeyboardScope, KeyboardShortcutConfig};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyboardSettingsPayload {
    pub new_window_hotkey: String,
    pub show_window_hotkey: String,
    pub toggle_window_hotkey: String,
    pub command_shortcuts: Vec<KeyboardShortcutPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyboardShortcutPayload {
    pub command_id: String,
    pub scope: String,
    pub accelerator: String,
}

pub fn default_keyboard_settings() -> KeyboardSettingsPayload {
    KeyboardSettingsPayload {
        new_window_hotkey: "Ctrl+N".to_string(),
        show_window_hotkey: String::new(),
        toggle_window_hotkey: String::new(),
        command_shortcuts: vec![
            shortcut("search.execute", "search_edit", "Enter"),
            shortcut("search.clear", "search_edit", "Escape"),
            shortcut("results.select_next", "global", "ArrowDown"),
            shortcut("results.select_previous", "global", "ArrowUp"),
            shortcut("results.open", "result_list", "Enter"),
            shortcut("results.copy_path", "global", "Ctrl+C"),
            shortcut("results.trash", "global", "Delete"),
            shortcut("results.delete_permanently", "global", "Shift+Delete"),
            shortcut("window.open_diagnostics", "global", "Ctrl+Period"),
            shortcut("window.open_options", "global", "Ctrl+Comma"),
        ],
    }
}

pub fn settings_from_config(config: &Config) -> KeyboardSettingsPayload {
    let defaults = default_keyboard_settings();
    let mut settings = KeyboardSettingsPayload {
        new_window_hotkey: config.keyboard.new_window_hotkey.clone(),
        show_window_hotkey: config.keyboard.show_window_hotkey.clone(),
        toggle_window_hotkey: config.keyboard.toggle_window_hotkey.clone(),
        command_shortcuts: config
            .keyboard
            .command_shortcuts
            .iter()
            .map(|shortcut| KeyboardShortcutPayload {
                command_id: shortcut.command_id.clone(),
                scope: keyboard_scope_name(shortcut.scope).to_string(),
                accelerator: shortcut.accelerator.clone(),
            })
            .collect(),
    };

    if settings.new_window_hotkey.is_empty()
        && settings.show_window_hotkey.is_empty()
        && settings.toggle_window_hotkey.is_empty()
        && settings.command_shortcuts.is_empty()
    {
        settings = defaults;
    } else if settings.command_shortcuts.is_empty() {
        settings.command_shortcuts = defaults.command_shortcuts;
    }

    settings
}

pub fn apply_settings_to_config(
    config: &mut Config,
    payload: KeyboardSettingsPayload,
) -> Result<KeyboardSettingsPayload, String> {
    let normalized = normalize_and_validate(payload)?;
    config.keyboard = KeyboardConfig {
        new_window_hotkey: normalized.new_window_hotkey.clone(),
        show_window_hotkey: normalized.show_window_hotkey.clone(),
        toggle_window_hotkey: normalized.toggle_window_hotkey.clone(),
        command_shortcuts: normalized
            .command_shortcuts
            .iter()
            .map(|shortcut| -> Result<KeyboardShortcutConfig, String> {
                Ok(KeyboardShortcutConfig {
                    command_id: shortcut.command_id.clone(),
                    scope: parse_scope(&shortcut.scope)?,
                    accelerator: shortcut.accelerator.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(normalized)
}

pub fn normalize_and_validate(
    mut payload: KeyboardSettingsPayload,
) -> Result<KeyboardSettingsPayload, String> {
    payload.new_window_hotkey = normalize_accelerator(&payload.new_window_hotkey)?;
    payload.show_window_hotkey = normalize_accelerator(&payload.show_window_hotkey)?;
    payload.toggle_window_hotkey = normalize_accelerator(&payload.toggle_window_hotkey)?;

    for shortcut in &mut payload.command_shortcuts {
        shortcut.scope = keyboard_scope_name(parse_scope(&shortcut.scope)?).to_string();
        shortcut.accelerator = normalize_accelerator(&shortcut.accelerator)?;
        if shortcut.command_id.trim().is_empty() {
            return Err("command shortcut is missing command_id".to_string());
        }
        if shortcut.accelerator.is_empty() {
            return Err(format!(
                "command shortcut for {} is missing an accelerator",
                shortcut.command_id
            ));
        }
    }

    let mut seen = HashMap::<String, Vec<ShortcutOwner>>::new();
    for shortcut in &payload.command_shortcuts {
        if let Some(existing_shortcuts) = seen.get(&shortcut.accelerator)
            && let Some(existing) = existing_shortcuts
                .iter()
                .find(|existing| scopes_conflict(&existing.scope, &shortcut.scope))
        {
            return Err(format!(
                "shortcut conflict: {} for {} ({}) is already used by {} ({})",
                shortcut.accelerator,
                shortcut.command_id,
                scope_label(&shortcut.scope),
                existing.command_id,
                scope_label(&existing.scope),
            ));
        }

        seen.entry(shortcut.accelerator.clone())
            .or_default()
            .push(ShortcutOwner {
                command_id: shortcut.command_id.clone(),
                scope: shortcut.scope.clone(),
            });
    }

    let mut hotkeys = HashMap::<String, &str>::new();
    for (name, accelerator) in [
        ("New window hotkey", &payload.new_window_hotkey),
        ("Show window hotkey", &payload.show_window_hotkey),
        ("Toggle window hotkey", &payload.toggle_window_hotkey),
    ] {
        if accelerator.is_empty() {
            continue;
        }
        if let Some(existing) = hotkeys.insert(accelerator.clone(), name) {
            return Err(format!(
                "window hotkey conflict: {} is already used by {}",
                accelerator, existing
            ));
        }
    }

    Ok(payload)
}

pub fn normalize_accelerator(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut meta = false;
    let mut key: Option<String> = None;

    for raw_part in trimmed.split('+') {
        let part = raw_part.trim();
        if part.is_empty() {
            return Err(format!("invalid accelerator: {}", value));
        }

        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            "meta" | "cmd" | "command" | "super" => meta = true,
            _ => {
                if key.is_some() {
                    return Err(format!(
                        "accelerator must contain exactly one key: {}",
                        value
                    ));
                }
                key = Some(normalize_key(part)?);
            }
        }
    }

    let key = key.ok_or_else(|| format!("accelerator is missing a key: {}", value))?;
    let mut parts = Vec::new();
    if ctrl {
        parts.push("Ctrl".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    if meta {
        parts.push("Meta".to_string());
    }
    parts.push(key);

    Ok(parts.join("+"))
}

fn normalize_key(value: &str) -> Result<String, String> {
    let lower = value.trim().to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "enter" | "return" => "Enter".to_string(),
        "escape" | "esc" => "Escape".to_string(),
        "arrowup" | "up" => "ArrowUp".to_string(),
        "arrowdown" | "down" => "ArrowDown".to_string(),
        "arrowleft" | "left" => "ArrowLeft".to_string(),
        "arrowright" | "right" => "ArrowRight".to_string(),
        "delete" | "del" => "Delete".to_string(),
        "backspace" => "Backspace".to_string(),
        "space" | "spacebar" => "Space".to_string(),
        "period" | "." => "Period".to_string(),
        "comma" | "," => "Comma".to_string(),
        "tab" => "Tab".to_string(),
        other if other.len() == 1 => other.to_ascii_uppercase(),
        other if other.starts_with('f') && other[1..].chars().all(|c| c.is_ascii_digit()) => {
            format!("F{}", &other[1..])
        }
        "mediatrackprevious" => "MediaTrackPrevious".to_string(),
        "mediatracknext" => "MediaTrackNext".to_string(),
        "mediaplaypause" => "MediaPlayPause".to_string(),
        "mediastop" => "MediaStop".to_string(),
        "audiovolumedown" => "AudioVolumeDown".to_string(),
        "audiovolumeup" => "AudioVolumeUp".to_string(),
        "audiovolumemute" => "AudioVolumeMute".to_string(),
        _ => return Err(format!("unsupported key in accelerator: {}", value)),
    };

    reject_runtime_unsupported_key(&normalized)?;
    Ok(normalized)
}

#[derive(Clone)]
struct ShortcutOwner {
    command_id: String,
    scope: String,
}

fn scope_label(scope: &str) -> &'static str {
    match scope {
        "global" => "Global",
        "search_edit" => "Search Edit",
        "result_list" => "Result List",
        _ => "Unknown Scope",
    }
}

fn parse_scope(scope: &str) -> Result<KeyboardScope, String> {
    match scope {
        "global" => Ok(KeyboardScope::Global),
        "search_edit" => Ok(KeyboardScope::SearchEdit),
        "result_list" => Ok(KeyboardScope::ResultList),
        _ => Err(format!("unknown keyboard scope: {}", scope)),
    }
}

#[cfg(target_os = "linux")]
fn reject_runtime_unsupported_key(key: &str) -> Result<(), String> {
    if matches!(
        key,
        "MediaTrackPrevious"
            | "MediaTrackNext"
            | "MediaPlayPause"
            | "MediaStop"
            | "AudioVolumeDown"
            | "AudioVolumeUp"
            | "AudioVolumeMute"
    ) {
        return Err(format!(
            "unsupported key in accelerator: {} (not currently supported by Tauri hotkey handling on Linux)",
            key
        ));
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn reject_runtime_unsupported_key(_key: &str) -> Result<(), String> {
    Ok(())
}

fn keyboard_scope_name(scope: KeyboardScope) -> &'static str {
    match scope {
        KeyboardScope::Global => "global",
        KeyboardScope::SearchEdit => "search_edit",
        KeyboardScope::ResultList => "result_list",
    }
}

fn scopes_conflict(left: &str, right: &str) -> bool {
    left == "global" || right == "global" || left == right
}

fn shortcut(command_id: &str, scope: &str, accelerator: &str) -> KeyboardShortcutPayload {
    KeyboardShortcutPayload {
        command_id: command_id.to_string(),
        scope: scope.to_string(),
        accelerator: accelerator.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_accelerators() {
        assert_eq!(
            normalize_accelerator("ctrl+shift+n").unwrap(),
            "Ctrl+Shift+N"
        );
        assert_eq!(normalize_accelerator("period").unwrap(), "Period");
        assert_eq!(normalize_accelerator(" ").unwrap(), "");
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn accepts_media_keys_on_supported_platforms() {
        assert_eq!(
            normalize_accelerator("MediaTrackPrevious").unwrap(),
            "MediaTrackPrevious"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_media_keys_on_linux() {
        let error = normalize_accelerator("MediaTrackPrevious").unwrap_err();
        assert!(error.contains("not currently supported by Tauri hotkey handling on Linux"));
    }

    #[test]
    fn rejects_conflicting_shortcuts() {
        let payload = KeyboardSettingsPayload {
            new_window_hotkey: String::new(),
            show_window_hotkey: String::new(),
            toggle_window_hotkey: String::new(),
            command_shortcuts: vec![
                shortcut("search.execute", "global", "Enter"),
                shortcut("results.open", "result_list", "Enter"),
            ],
        };

        let error = normalize_and_validate(payload).unwrap_err();
        assert!(error.contains("shortcut conflict"));
    }

    #[test]
    fn allows_enter_in_search_and_result_scopes() {
        let payload = default_keyboard_settings();
        let normalized = normalize_and_validate(payload).unwrap();

        assert!(normalized.command_shortcuts.iter().any(|shortcut| {
            shortcut.command_id == "search.execute"
                && shortcut.scope == "search_edit"
                && shortcut.accelerator == "Enter"
        }));
        assert!(normalized.command_shortcuts.iter().any(|shortcut| {
            shortcut.command_id == "results.open"
                && shortcut.scope == "result_list"
                && shortcut.accelerator == "Enter"
        }));
    }
}
