use std::cell::RefCell;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortState {
    pub column: i32,
    pub ascending: bool,
}

/// Name, Path and Size widths in logical pixels. Modified fills the rest.
pub type ColumnWidths = [f32; 3];
const MIN_COLUMN_WIDTHS: ColumnWidths = [140.0, 160.0, 70.0];
const MAX_COLUMN_WIDTH: f32 = 4000.0;

/// Clamp resized widths to the table's minimums, rejecting non-finite values.
pub fn valid_widths(widths: ColumnWidths) -> Option<ColumnWidths> {
    if !widths.iter().all(|width| width.is_finite()) {
        return None;
    }
    let mut valid = widths;
    for (width, min) in valid.iter_mut().zip(MIN_COLUMN_WIDTHS) {
        *width = width.round().clamp(min, MAX_COLUMN_WIDTH);
    }
    Some(valid)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiState {
    pub sort: Option<SortState>,
    pub column_widths: Option<ColumnWidths>,
}

pub fn path() -> PathBuf {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    root.join("toge/slint-ui.toml")
}

impl UiState {
    /// Invalid entries are dropped individually, so a bad sort setting never
    /// selects an invalid column and does not discard saved column widths.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let (mut column, mut ascending, mut widths) = (None, None, None);
        for line in text.lines() {
            let Some((key, value)) = line.split('#').next().unwrap_or_default().split_once('=')
            else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "sort_column" => column = value.parse::<i32>().ok(),
                "sort_ascending" => ascending = value.parse::<bool>().ok(),
                "column_widths" => {
                    let parsed: Vec<f32> = value
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .split(',')
                        .filter_map(|width| width.trim().parse().ok())
                        .collect();
                    widths = parsed.try_into().ok().and_then(valid_widths);
                }
                _ => {}
            }
        }
        let sort = match (column, ascending) {
            (Some(column @ 0..=3), Some(ascending)) => Some(SortState { column, ascending }),
            _ => None,
        };
        Ok(Self {
            sort,
            column_widths: widths,
        })
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if self
            .sort
            .is_some_and(|sort| !(0..=3).contains(&sort.column))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid sort column",
            ));
        }
        let mut text = String::new();
        if let Some(sort) = self.sort {
            text += &format!(
                "sort_column = {}\nsort_ascending = {}\n",
                sort.column, sort.ascending
            );
        }
        if let Some([name, path, size]) = self.column_widths.and_then(valid_widths) {
            text += &format!("column_widths = [{name}, {path}, {size}]\n");
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
            file.write_all(text.as_bytes())?;
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
    let state = UiState::load(&path).unwrap_or_else(|error| {
        eprintln!("Could not restore table settings: {error}");
        UiState::default()
    });
    if let Some(sort) = state.sort {
        mailbox.set_sort(Some((sort.column, sort.ascending)));
        ui.invoke_apply_sort(sort.column, sort.ascending);
    }
    if let Some([name, path, size]) = state.column_widths {
        ui.invoke_apply_column_widths(name, path, size);
    }
    let state = Rc::new(RefCell::new(state));
    let weak = slint::ComponentHandle::as_weak(ui);
    let (sort_state, sort_path) = (state.clone(), path.clone());
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
        let mut state = sort_state.borrow_mut();
        state.sort = Some(SortState { column, ascending });
        if let Err(error) = state.save(&sort_path) {
            ui.set_status(format!("Could not save table sort: {error}").into());
        }
    });
    let weak = slint::ComponentHandle::as_weak(ui);
    ui.on_column_widths_changed(move |name, path_width, size| {
        let Some(widths) = valid_widths([name, path_width, size]) else {
            return;
        };
        let mut state = state.borrow_mut();
        if state.column_widths == Some(widths) {
            return;
        }
        state.column_widths = Some(widths);
        if let Err(error) = state.save(&path)
            && let Some(ui) = weak.upgrade()
        {
            ui.set_status(format!("Could not save column widths: {error}").into());
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
        assert_eq!(UiState::load(&path).unwrap(), UiState::default());
        for column in 0..=3 {
            for ascending in [false, true] {
                let expected = UiState {
                    sort: Some(SortState { column, ascending }),
                    column_widths: None,
                };
                expected.save(&path).unwrap();
                assert_eq!(UiState::load(&path).unwrap(), expected);
            }
        }
        UiState {
            sort: Some(SortState {
                column: 2,
                ascending: false,
            }),
            column_widths: None,
        }
        .save(&path)
        .unwrap();
        let state = UiState::load(&path).unwrap().sort.unwrap();
        // The restored column reaches the daemon as a sort key for new sessions.
        let mailbox = crate::worker::Mailbox::default();
        mailbox.set_sort(Some((state.column, state.ascending)));
        assert_eq!(
            crate::worker::sort_key(mailbox.sort()),
            Some((toge_core::sort::SortKey::Size, false))
        );
    }

    #[test]
    fn column_widths_round_trip_alongside_sort() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slint-ui.toml");
        let state = UiState {
            sort: Some(SortState {
                column: 1,
                ascending: true,
            }),
            column_widths: Some([260.0, 410.0, 96.0]),
        };
        state.save(&path).unwrap();
        assert_eq!(UiState::load(&path).unwrap(), state);
        // Widths alone survive without a saved sort.
        let widths_only = UiState {
            sort: None,
            column_widths: Some([180.0, 200.0, 80.0]),
        };
        widths_only.save(&path).unwrap();
        assert_eq!(UiState::load(&path).unwrap(), widths_only);
    }

    #[test]
    fn invalid_column_widths_are_clamped_or_ignored() {
        assert_eq!(
            valid_widths([10.0, 99999.0, 80.4]),
            Some([140.0, MAX_COLUMN_WIDTH, 80.0])
        );
        assert_eq!(valid_widths([f32::NAN, 200.0, 80.0]), None);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slint-ui.toml");
        for text in [
            "column_widths = [200, 300]",
            "column_widths = [200, 300, 90, 100]",
            "column_widths = [200, wide, 90]",
            "column_widths = [inf, 300, 90]",
        ] {
            std::fs::write(
                &path,
                format!("sort_column = 0\nsort_ascending = true\n{text}"),
            )
            .unwrap();
            let state = UiState::load(&path).unwrap();
            assert_eq!(state.column_widths, None, "{text}");
            // A bad width entry does not discard the sort.
            assert!(state.sort.is_some(), "{text}");
        }
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
            "sort_column = 2\nsort_ascending = invalid\ncolumn_widths = [200, 300, 90]",
        ] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(UiState::load(&path).unwrap().sort, None, "{text}");
        }
        assert_eq!(
            UiState::load(&path).unwrap().column_widths,
            Some([200.0, 300.0, 90.0])
        );
        let original = std::fs::read(&path).unwrap();
        assert!(
            UiState {
                sort: Some(SortState {
                    column: 4,
                    ascending: false
                }),
                column_widths: None,
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
        let state = UiState {
            sort: Some(SortState {
                column: 0,
                ascending: true,
            }),
            column_widths: Some([220.0, 340.0, 90.0]),
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
