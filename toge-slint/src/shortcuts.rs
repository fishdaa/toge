//! Configurable application shortcuts. Window shortcuts are handled by the
//! desktop portal; focused-window shortcuts are matched from Slint key events.
use slint::platform::Key;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
pub struct Definition {
    pub id: &'static str,
    pub label: &'static str,
    pub default: &'static str,
    pub global: bool,
}

pub const DEFINITIONS: &[Definition] = &[
    Definition {
        id: "focus-search",
        label: "Focus search",
        default: "Ctrl+L",
        global: false,
    },
    Definition {
        id: "new-window",
        label: "New window",
        default: "Ctrl+N",
        global: false,
    },
    Definition {
        id: "open",
        label: "Open result",
        default: "Return",
        global: false,
    },
    Definition {
        id: "copy",
        label: "Copy file",
        default: "Ctrl+C",
        global: false,
    },
    Definition {
        id: "copy-path",
        label: "Copy path",
        default: "Ctrl+Shift+C",
        global: false,
    },
    Definition {
        id: "cut",
        label: "Cut file",
        default: "Ctrl+X",
        global: false,
    },
    Definition {
        id: "rename",
        label: "Rename",
        default: "F2",
        global: false,
    },
    Definition {
        id: "delete",
        label: "Move to Trash",
        default: "Delete",
        global: false,
    },
    Definition {
        id: "delete-permanently",
        label: "Delete permanently",
        default: "Shift+Delete",
        global: false,
    },
    Definition {
        id: "up",
        label: "Previous result",
        default: "Up",
        global: false,
    },
    Definition {
        id: "down",
        label: "Next result",
        default: "Down",
        global: false,
    },
    Definition {
        id: "home",
        label: "First result",
        default: "Home",
        global: false,
    },
    Definition {
        id: "end",
        label: "Last result",
        default: "End",
        global: false,
    },
    Definition {
        id: "page-up",
        label: "Previous page",
        default: "PageUp",
        global: false,
    },
    Definition {
        id: "page-down",
        label: "Next page",
        default: "PageDown",
        global: false,
    },
    Definition {
        id: "toggle-window",
        label: "Toggle window (systemwide)",
        default: "",
        global: true,
    },
    Definition {
        id: "show-window",
        label: "Show window (systemwide)",
        default: "",
        global: true,
    },
    Definition {
        id: "hide-window",
        label: "Hide window (systemwide)",
        default: "",
        global: true,
    },
    Definition {
        id: "global-new-window",
        label: "New window (systemwide)",
        default: "",
        global: true,
    },
];

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    control: bool,
    alt: bool,
    shift: bool,
    meta: bool,
    key: String,
}

impl Chord {
    pub fn parse(input: &str) -> Option<Self> {
        let mut chord = Self {
            control: false,
            alt: false,
            shift: false,
            meta: false,
            key: String::new(),
        };
        for piece in input.split('+') {
            match piece.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" if !chord.control => chord.control = true,
                "alt" if !chord.alt => chord.alt = true,
                "shift" if !chord.shift => chord.shift = true,
                "meta" | "super" if !chord.meta => chord.meta = true,
                key if chord.key.is_empty() && valid_key(key) => chord.key = canonical_key(key),
                _ => return None,
            }
        }
        (!chord.key.is_empty()).then_some(chord)
    }

    #[allow(clippy::fn_params_excessive_bools)]
    pub fn from_event(
        text: &str,
        control: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    ) -> Option<Self> {
        let key = match text.chars().next()? {
            k if k == char::from(Key::Return) => "Return".into(),
            k if (char::from(Key::F1)..=char::from(Key::F12)).contains(&k) => {
                format!("F{}", u32::from(k) - u32::from(char::from(Key::F1)) + 1)
            }
            k if k == char::from(Key::Space) => "Space".into(),
            k if k == char::from(Key::Escape) => "Escape".into(),
            k if k == char::from(Key::Tab) => "Tab".into(),
            k if k == char::from(Key::Delete) => "Delete".into(),
            k if k == char::from(Key::UpArrow) => "Up".into(),
            k if k == char::from(Key::DownArrow) => "Down".into(),
            k if k == char::from(Key::Home) => "Home".into(),
            k if k == char::from(Key::End) => "End".into(),
            k if k == char::from(Key::PageUp) => "PageUp".into(),
            k if k == char::from(Key::PageDown) => "PageDown".into(),
            k if k.is_ascii_alphanumeric() => k.to_ascii_uppercase().to_string(),
            _ => return None,
        };
        Some(Self {
            control,
            alt,
            shift,
            meta,
            key,
        })
    }

    pub fn portal_trigger(&self) -> String {
        let mut parts = Vec::new();
        if self.control {
            parts.push("CTRL".to_string());
        }
        if self.alt {
            parts.push("ALT".to_string());
        }
        if self.shift {
            parts.push("SHIFT".to_string());
        }
        if self.meta {
            parts.push("LOGO".to_string());
        }
        parts.push(match self.key.as_str() {
            "Space" => "space".to_string(),
            "PageUp" => "Page_Up".to_string(),
            "PageDown" => "Page_Down".to_string(),
            key if key.len() == 1 => key.to_ascii_lowercase(),
            key => key.to_string(),
        });
        parts.join("+")
    }

    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.control {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.meta {
            parts.push("Meta");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

fn valid_key(key: &str) -> bool {
    key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric())
        || key
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=12).contains(&number))
        || matches!(
            key,
            "return"
                | "enter"
                | "space"
                | "escape"
                | "tab"
                | "delete"
                | "up"
                | "down"
                | "home"
                | "end"
                | "pageup"
                | "pagedown"
        )
}

fn canonical_key(key: &str) -> String {
    match key {
        "return" | "enter" => "Return".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "space" => "Space".into(),
        "escape" => "Escape".into(),
        "tab" => "Tab".into(),
        "up" => "Up".into(),
        "down" => "Down".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "delete" => "Delete".into(),
        _ => key.to_ascii_uppercase(),
    }
}

#[derive(Clone, Debug)]
pub struct Shortcuts {
    bindings: Vec<Option<Chord>>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            bindings: DEFINITIONS
                .iter()
                .map(|definition| Chord::parse(definition.default))
                .collect(),
        }
    }
}

impl Shortcuts {
    #[cfg(test)]
    pub fn get(&self, id: &str) -> Option<&Chord> {
        let index = DEFINITIONS
            .iter()
            .position(|definition| definition.id == id)?;
        self.bindings[index].as_ref()
    }

    pub fn action(&self, event: &Chord) -> Option<&'static str> {
        let exact = DEFINITIONS
            .iter()
            .zip(&self.bindings)
            .find(|(definition, binding)| !definition.global && binding.as_ref() == Some(event))
            .map(|(definition, _)| definition.id);
        if exact.is_some() || !event.shift {
            return exact;
        }
        // Shift extends a result selection while moving through the table.
        // It is not a separate binding for the six navigation actions.
        let mut without_shift = event.clone();
        without_shift.shift = false;
        DEFINITIONS
            .iter()
            .zip(&self.bindings)
            .find(|(definition, binding)| {
                matches!(
                    definition.id,
                    "up" | "down" | "home" | "end" | "page-up" | "page-down"
                ) && binding.as_ref() == Some(&without_shift)
            })
            .map(|(definition, _)| definition.id)
    }

    pub fn set(&mut self, id: &str, text: &str) -> Result<(), &'static str> {
        let index = DEFINITIONS
            .iter()
            .position(|definition| definition.id == id)
            .ok_or("Unknown action")?;
        let binding = if text.trim().is_empty() {
            None
        } else {
            Some(Chord::parse(text).ok_or("Use a key such as Ctrl+L or Shift+Delete")?)
        };
        if let Some(ref chord) = binding
            && DEFINITIONS.iter().enumerate().any(|(other, definition)| {
                other != index
                    && definition.global == DEFINITIONS[index].global
                    && self.bindings[other].as_ref() == Some(chord)
            })
        {
            return Err("This shortcut is already assigned");
        }
        self.bindings[index] = binding;
        Ok(())
    }

    pub fn global_entries(&self) -> Vec<(&'static str, &'static str, String)> {
        DEFINITIONS
            .iter()
            .zip(&self.bindings)
            .filter_map(|(definition, binding)| definition.global.then_some((definition, binding)))
            .filter_map(|(definition, binding)| {
                binding
                    .as_ref()
                    .map(|binding| (definition.id, definition.label, binding.portal_trigger()))
            })
            .collect()
    }

    pub fn rows(&self) -> Vec<crate::ShortcutEntry> {
        DEFINITIONS
            .iter()
            .zip(&self.bindings)
            .map(|(definition, binding)| crate::ShortcutEntry {
                id: definition.id.into(),
                label: definition.label.into(),
                binding: binding
                    .as_ref()
                    .map_or_else(String::new, Chord::display)
                    .into(),
                global: definition.global,
            })
            .collect()
    }

    pub fn load(path: &Path) -> Self {
        let Ok(contents) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        // Begin empty so two customized bindings can exchange their defaults.
        let mut state = Self {
            bindings: vec![None; DEFINITIONS.len()],
        };
        let mut seen = vec![false; DEFINITIONS.len()];
        for line in contents.lines() {
            if let Some((id, binding)) = line.split_once('=')
                && let Some(index) = DEFINITIONS
                    .iter()
                    .position(|definition| definition.id == id.trim())
                && state.set(id.trim(), binding.trim()).is_ok()
            {
                seen[index] = true;
            }
        }
        for (index, definition) in DEFINITIONS.iter().enumerate() {
            if !seen[index] {
                let _ = state.set(definition.id, definition.default);
            }
        }
        state
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing shortcuts directory"))?;
        std::fs::create_dir_all(parent)?;
        let temp = parent.join(format!(
            ".shortcuts-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| {
            for (definition, binding) in DEFINITIONS.iter().zip(&self.bindings) {
                writeln!(
                    file,
                    "{}={}",
                    definition.id,
                    binding.as_ref().map_or_else(String::new, Chord::display)
                )?;
            }
            file.sync_all()?;
            std::fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
}

pub fn path() -> PathBuf {
    crate::preferences::path().with_file_name("shortcuts.conf")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bindings_round_trip_and_reject_conflicts() {
        let mut shortcuts = Shortcuts::default();
        assert_eq!(
            shortcuts.action(&Chord::parse("Ctrl+L").unwrap()),
            Some("focus-search")
        );
        assert_eq!(
            Chord::parse("Meta+F12").unwrap().portal_trigger(),
            "LOGO+F12"
        );
        assert_eq!(
            Chord::parse("Ctrl+Space").unwrap().portal_trigger(),
            "CTRL+space"
        );
        assert_eq!(
            shortcuts.action(&Chord::parse("Shift+Down").unwrap()),
            Some("down")
        );
        assert_eq!(
            shortcuts.action(&Chord::parse("Shift+PageDown").unwrap()),
            Some("page-down")
        );
        assert!(shortcuts.set("new-window", "Ctrl+L").is_err());
        shortcuts.set("new-window", "Alt+N").unwrap();
        shortcuts.set("toggle-window", "Ctrl+Alt+T").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("toge/shortcuts.conf");
        shortcuts.save(&path).unwrap();
        let loaded = Shortcuts::load(&path);
        assert_eq!(loaded.get("new-window"), Chord::parse("Alt+N").as_ref());
        assert_eq!(
            loaded.get("toggle-window"),
            Chord::parse("Ctrl+Alt+T").as_ref()
        );
        std::fs::write(&path, "focus-search=Ctrl+N\nnew-window=Ctrl+L\n").unwrap();
        let swapped = Shortcuts::load(&path);
        assert_eq!(swapped.get("focus-search"), Chord::parse("Ctrl+N").as_ref());
        assert_eq!(swapped.get("new-window"), Chord::parse("Ctrl+L").as_ref());
    }
}
