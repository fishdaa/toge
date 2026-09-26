use slint::{Model, ModelNotify, ModelRc, ModelTracker, StandardListViewItem, VecModel};
use std::cell::{Cell, RefCell};
use toge_core::ipc::ResultRow;

#[derive(Default)]
pub struct Results {
    rows: RefCell<Vec<ResultRow>>,
    order: RefCell<Vec<usize>>,
    rendered: RefCell<Vec<Option<ModelRc<StandardListViewItem>>>>,
    sort: Cell<Option<(i32, bool)>>,
    pub size_indexed: Cell<bool>,
    notify: ModelNotify,
}
impl Results {
    pub fn path(&self, index: i32) -> Option<String> {
        let index = usize::try_from(index).ok()?;
        self.order
            .borrow()
            .get(index)
            .map(|&i| self.rows.borrow()[i].path.clone())
    }
    pub fn find(&self, path: &str) -> i32 {
        let rows = self.rows.borrow();
        self.order
            .borrow()
            .iter()
            .position(|&i| rows[i].path == path)
            .map_or(-1, |i| i as i32)
    }
    pub fn replace(&self, rows: Vec<ResultRow>) {
        *self.order.borrow_mut() = (0..rows.len()).collect();
        let rendered_len = rows.len();
        *self.rows.borrow_mut() = rows;
        let mut rendered = self.rendered.borrow_mut();
        rendered.clear();
        rendered.resize_with(rendered_len, || None);
        self.resort();
    }
    pub fn append(&self, rows: Vec<ResultRow>) {
        let start = self.rows.borrow().len();
        let count = rows.len();
        if count == 0 {
            return;
        }
        self.rows.borrow_mut().extend(rows);
        self.order.borrow_mut().extend(start..start + count);
        self.rendered
            .borrow_mut()
            .resize_with(start + count, || None);
        if self.sort.get().is_some() {
            self.resort();
        } else {
            self.notify.row_added(start, count);
        }
    }
    pub fn remove_path(&self, path: &str) {
        let prefix = format!("{path}/");
        let rows = self
            .rows
            .borrow()
            .iter()
            .filter(|row| row.path != path && !row.path.starts_with(&prefix))
            .cloned()
            .collect();
        self.replace(rows);
    }
    pub fn rename_path(&self, old: &str, new: &str) {
        let prefix = format!("{old}/");
        let rows = self
            .rows
            .borrow()
            .iter()
            .cloned()
            .map(|mut row| {
                if row.path == old || row.path.starts_with(&prefix) {
                    row.path = format!("{new}{}", &row.path[old.len()..]);
                    let path = std::path::Path::new(&row.path);
                    row.name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    row.parent = path
                        .parent()
                        .unwrap_or(std::path::Path::new("/"))
                        .to_string_lossy()
                        .into_owned();
                    row.extension = if row.is_dir {
                        String::new()
                    } else {
                        path.extension()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    };
                }
                row
            })
            .collect();
        self.replace(rows);
    }
    pub fn sort(&self, column: i32, ascending: bool) {
        self.sort.set(Some((column, ascending)));
        self.resort();
    }
    pub fn set_size_indexed(&self, value: bool) {
        if self.size_indexed.replace(value) != value {
            self.rendered.borrow_mut().fill(None);
            self.notify.reset();
        }
    }
    fn resort(&self) {
        if let Some((column, ascending)) = self.sort.get() {
            let rows = self.rows.borrow();
            self.order.borrow_mut().sort_by(|&a, &b| {
                let (a, b) = (&rows[a], &rows[b]);
                let cmp = match column {
                    1 => a.parent.cmp(&b.parent),
                    2 => a.size.cmp(&b.size),
                    3 => a.modified_unix.cmp(&b.modified_unix),
                    _ => a.name.cmp(&b.name),
                }
                .then_with(|| a.path.cmp(&b.path));
                if ascending { cmp } else { cmp.reverse() }
            });
        }
        self.notify.reset();
    }
}
impl Model for Results {
    type Data = ModelRc<StandardListViewItem>;
    fn row_count(&self) -> usize {
        self.order.borrow().len()
    }
    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let index = *self.order.borrow().get(row)?;

        if let Some(rendered) = self.rendered.borrow()[index].clone() {
            return Some(rendered);
        }

        let rows = self.rows.borrow();
        let r = &rows[index];
        let size = if self.size_indexed.get() {
            crate::format::format_size(r.size)
        } else {
            "—".into()
        };
        let rendered = ModelRc::new(VecModel::from(
            vec![
                r.name.clone(),
                r.parent.clone(),
                size,
                crate::format::format_time(r.modified_unix),
            ]
            .into_iter()
            .map(|text| StandardListViewItem::from(slint::SharedString::from(text)))
            .collect::<Vec<_>>(),
        ));
        self.rendered.borrow_mut()[index] = Some(rendered.clone());
        Some(rendered)
    }
    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(path: &str, size: u64) -> ResultRow {
        ResultRow {
            path: path.into(),
            name: path.into(),
            parent: "/".into(),
            extension: String::new(),
            is_dir: false,
            size,
            modified_unix: size as i64,
            created_unix: 0,
            accessed_unix: 0,
        }
    }
    #[test]
    fn appending_keeps_cached_rows_and_active_sort() {
        let m = Results::default();
        m.replace(vec![row("large", 100), row("small", 9)]);
        let cached = m.row_data(0).unwrap();
        m.append(vec![row("middle", 20)]);
        assert_eq!(m.path(0).as_deref(), Some("large"));
        assert_eq!(m.row_count(), 3);
        assert_eq!(m.row_data(0).unwrap(), cached);
        m.sort(2, true);
        m.append(vec![row("tiny", 1)]);
        assert_eq!(m.path(0).as_deref(), Some("tiny"));
        assert_eq!(m.find("large"), 3);
        m.append(vec![]);
        assert_eq!(m.row_count(), 4);
    }
    #[test]
    fn renaming_updates_descendants_cached_cells_and_sort_order() {
        let m = Results::default();
        let mut directory = row("/tmp/old", 0);
        directory.is_dir = true;
        m.replace(vec![
            directory,
            row("/tmp/old/child.txt", 1),
            row("/tmp/older/keep", 2),
        ]);
        m.sort(1, true);
        let _ = m.row_data(m.find("/tmp/old/child.txt") as usize).unwrap();
        m.rename_path("/tmp/old", "/tmp/new");
        assert_eq!(m.find("/tmp/old"), -1);
        assert_eq!(m.row_count(), 3);
        let child = m.find("/tmp/new/child.txt") as usize;
        let cells = m.row_data(child).unwrap();
        assert_eq!(cells.row_data(0).unwrap().text, "child.txt");
        assert_eq!(cells.row_data(1).unwrap().text, "/tmp/new");
        assert!(m.find("/tmp/older/keep") >= 0);
        assert_eq!(m.rows.borrow()[1].extension, "txt");
        m.rename_path("/tmp/new/child.txt", "/tmp/new/child.pdf");
        assert_eq!(m.rows.borrow()[1].extension, "pdf");
    }
    #[test]
    fn removing_directory_removes_only_its_descendants() {
        let m = Results::default();
        m.replace(vec![
            row("/tmp/old", 0),
            row("/tmp/old/child", 1),
            row("/tmp/older/keep", 2),
        ]);
        m.remove_path("/tmp/old");
        assert_eq!(m.row_count(), 1);
        assert_eq!(m.path(0).as_deref(), Some("/tmp/older/keep"));
    }
    #[test]
    fn numeric_sort_and_path_selection_survive_replacement() {
        let m = Results::default();
        m.replace(vec![row("large", 100), row("small", 9)]);
        m.sort(2, true);
        assert_eq!(m.path(0).as_deref(), Some("small"));
        m.replace(vec![row("small", 9), row("large", 100), row("middle", 20)]);
        assert_eq!(m.find("large"), 2);
        m.sort(3, false);
        assert_eq!(m.path(0).as_deref(), Some("large"));
        m.replace(vec![]);
        assert_eq!(m.find("large"), -1);
        assert!(m.path(-1).is_none());
    }
}
