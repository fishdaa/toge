use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortState {
    pub column: i32,
    pub ascending: bool,
}

pub fn path() -> PathBuf {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    root.join("toge/slint-ui.toml")
}

impl SortState {
    pub fn load(path: &Path) -> io::Result<Option<Self>> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let (mut column, mut ascending) = (None, None);
        for line in text.lines() {
            let Some((key, value)) = line.split('#').next().unwrap_or_default().split_once('=')
            else {
                continue;
            };
            match key.trim() {
                "sort_column" => column = value.trim().parse::<i32>().ok(),
                "sort_ascending" => ascending = value.trim().parse::<bool>().ok(),
                _ => {}
            }
        }
        match (column, ascending) {
            (Some(column @ 0..=3), Some(ascending)) => Ok(Some(Self { column, ascending })),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid table sort settings",
            )),
        }
    }

    pub fn save(self, path: &Path) -> io::Result<()> {
        if !(0..=3).contains(&self.column) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid sort column",
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing settings directory"))?;
        std::fs::create_dir_all(parent)?;
        static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
        let temp = parent.join(format!(
            ".slint-ui-{}-{}.tmp",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| {
            write!(
                file,
                "sort_column = {}\nsort_ascending = {}\n",
                self.column, self.ascending
            )?;
            file.sync_all()?;
            std::fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
}

pub fn connect(ui: &crate::AppWindow, path: PathBuf, mailbox: Arc<crate::worker::Mailbox>) {
    if let Some(state) = SortState::load(&path).unwrap_or_else(|error| {
        eprintln!("Could not restore table sort: {error}");
        None
    }) {
        mailbox.set_sort(Some((state.column, state.ascending)));
        ui.invoke_apply_sort(state.column, state.ascending);
    }
    let weak = slint::ComponentHandle::as_weak(ui);
    ui.on_sort_results(move |column, ascending| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_rename_working() || !(0..=3).contains(&column) {
            ui.invoke_apply_sort(ui.get_sort_column(), ui.get_sort_ascending());
            return;
        }
        ui.invoke_apply_sort(column, ascending);
        // Sessions opened from now on use the new order; the daemon re-sorts
        // the current one in place.
        mailbox.set_sort(Some((column, ascending)));
        crate::worker::results(&ui).send(crate::model::Command::Resort(Some((column, ascending))));
        ui.invoke_select_row(-1);
        if let Err(error) = (SortState { column, ascending }).save(&path) {
            ui.set_status(format!("Could not save table sort: {error}").into());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_sort_restores_order_across_new_models_and_result_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile/toge/slint-ui.toml");
        assert_eq!(SortState::load(&path).unwrap(), None);
        for column in 0..=3 {
            for ascending in [false, true] {
                let expected = SortState { column, ascending };
                expected.save(&path).unwrap();
                assert_eq!(SortState::load(&path).unwrap(), Some(expected));
            }
        }
        SortState {
            column: 2,
            ascending: false,
        }
        .save(&path)
        .unwrap();
        let state = SortState::load(&path).unwrap().unwrap();
        // The restored column reaches the daemon as a sort key for new sessions.
        let mailbox = crate::worker::Mailbox::default();
        mailbox.set_sort(Some((state.column, state.ascending)));
        assert_eq!(
            crate::worker::sort_key(mailbox.sort()),
            Some((toge_core::sort::SortKey::Size, false))
        );
    }

    #[test]
    fn invalid_saved_sort_never_selects_an_invalid_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sort.toml");
        for text in [
            "",
            "sort_column = -1\nsort_ascending = true",
            "sort_column = 4\nsort_ascending = false",
            "sort_column = 0",
            "sort_column = 2\nsort_ascending = invalid",
        ] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(
                SortState::load(&path).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        let original = std::fs::read(&path).unwrap();
        assert!(
            SortState {
                column: 4,
                ascending: false
            }
            .save(&path)
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn saving_sort_keeps_daemon_config_and_cleans_up_failed_temporary_writes() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "[roots]\ninclude = [\"/keep\"]").unwrap();
        let path = dir.path().join("slint-ui.toml");
        let state = SortState {
            column: 0,
            ascending: true,
        };
        state.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(config).unwrap(),
            "[roots]\ninclude = [\"/keep\"]"
        );
        let bad = dir.path().join("directory");
        std::fs::create_dir(&bad).unwrap();
        assert!(state.save(&bad).is_err());
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }
}
