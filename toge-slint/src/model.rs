use slint::{Model, ModelNotify, ModelRc, ModelTracker, StandardListViewItem, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use toge_core::ipc::ResultRow;

// Keep one path allocation per ordinary row. Wire-only metadata is discarded.
// Retain exceptional display labels (e.g. highlighted paths) without changing them.
struct Row {
    path: Box<str>,
    labels: Option<Box<(Box<str>, Box<str>)>>,
    size: u64,
    modified_unix: i64,
}
impl Row {
    fn split(path: &str) -> (&str, &str) {
        path.rsplit_once('/')
            .map_or((path, ""), |(parent, name)| (name, parent))
    }
    fn name(&self) -> &str {
        self.labels
            .as_ref()
            .map_or_else(|| Self::split(&self.path).0, |labels| &labels.0)
    }
    fn parent(&self) -> &str {
        self.labels
            .as_ref()
            .map_or_else(|| Self::split(&self.path).1, |labels| &labels.1)
    }
}
impl From<ResultRow> for Row {
    fn from(row: ResultRow) -> Self {
        let (name, parent) = Self::split(&row.path);
        let labels = (name != row.name || parent != row.parent)
            .then(|| Box::new((row.name.into_boxed_str(), row.parent.into_boxed_str())));
        Self {
            path: row.path.into_boxed_str(),
            labels,
            size: row.size,
            modified_unix: row.modified_unix,
        }
    }
}

// Bound retained formatting even after scrolling through millions of results.
const RENDER_CACHE_LIMIT: usize = 256;
#[derive(Default)]
struct RenderCache {
    rows: HashMap<usize, ModelRc<StandardListViewItem>>,
    order: VecDeque<usize>,
}
impl RenderCache {
    fn clear(&mut self) {
        self.rows.clear();
        self.order.clear();
    }
    fn insert(&mut self, index: usize, row: ModelRc<StandardListViewItem>) {
        if self.rows.len() == RENDER_CACHE_LIMIT {
            self.rows.remove(&self.order.pop_front().unwrap());
        }
        self.order.push_back(index);
        self.rows.insert(index, row);
    }
}

#[derive(Default)]
pub struct Results {
    rows: RefCell<Vec<Row>>,
    order: RefCell<Vec<usize>>,
    rendered: RefCell<RenderCache>,
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
            .map(|&i| self.rows.borrow()[i].path.to_string())
    }
    pub fn find(&self, path: &str) -> i32 {
        let rows = self.rows.borrow();
        self.order
            .borrow()
            .iter()
            .position(|&i| rows[i].path.as_ref() == path)
            .map_or(-1, |i| i as i32)
    }
    pub fn replace(&self, rows: Vec<ResultRow>) {
        *self.order.borrow_mut() = (0..rows.len()).collect();
        *self.rows.borrow_mut() = rows.into_iter().map(Row::from).collect();
        self.rendered.borrow_mut().clear();
        self.resort();
    }
    pub fn append(&self, rows: Vec<ResultRow>) {
        let start = self.rows.borrow().len();
        let count = rows.len();
        if count == 0 {
            return;
        }
        self.rows
            .borrow_mut()
            .extend(rows.into_iter().map(Row::from));
        self.order.borrow_mut().extend(start..start + count);
        if self.sort.get().is_some() {
            self.resort();
        } else {
            self.notify.row_added(start, count);
        }
    }
    pub fn remove_path(&self, path: &str) {
        let prefix = format!("{path}/");
        self.rows
            .borrow_mut()
            .retain(|row| row.path.as_ref() != path && !row.path.starts_with(&prefix));
        self.rebuild_order();
    }
    pub fn rename_path(&self, old: &str, new: &str) {
        let prefix = format!("{old}/");
        for row in self.rows.borrow_mut().iter_mut() {
            if row.path.as_ref() == old || row.path.starts_with(&prefix) {
                row.path = format!("{new}{}", &row.path[old.len()..]).into_boxed_str();
                let path = std::path::Path::new(row.path.as_ref());
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let parent = path
                    .parent()
                    .unwrap_or(std::path::Path::new("/"))
                    .to_string_lossy();
                let (derived_name, derived_parent) = Row::split(&row.path);
                row.labels = (name != derived_name || parent != derived_parent).then(|| {
                    Box::new((
                        name.into_owned().into_boxed_str(),
                        parent.into_owned().into_boxed_str(),
                    ))
                });
            }
        }
        self.rebuild_order();
    }
    fn rebuild_order(&self) {
        *self.order.borrow_mut() = (0..self.rows.borrow().len()).collect();
        self.rendered.borrow_mut().clear();
        self.resort();
    }
    pub fn sort(&self, column: i32, ascending: bool) {
        self.sort.set(Some((column, ascending)));
        self.resort();
    }
    pub fn set_size_indexed(&self, value: bool) {
        if self.size_indexed.replace(value) != value {
            self.rendered.borrow_mut().clear();
            self.notify.reset();
        }
    }
    fn resort(&self) {
        if let Some((column, ascending)) = self.sort.get() {
            let rows = self.rows.borrow();
            self.order.borrow_mut().sort_by(|&a, &b| {
                let (a, b) = (&rows[a], &rows[b]);
                let cmp = match column {
                    1 => a.parent().cmp(b.parent()),
                    2 => a.size.cmp(&b.size),
                    3 => a.modified_unix.cmp(&b.modified_unix),
                    _ => a.name().cmp(b.name()),
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

        if let Some(rendered) = self.rendered.borrow().rows.get(&index).cloned() {
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
                r.name().to_string(),
                r.parent().to_string(),
                size,
                crate::format::format_time(r.modified_unix),
            ]
            .into_iter()
            .map(|text| StandardListViewItem::from(slint::SharedString::from(text)))
            .collect::<Vec<_>>(),
        ));
        self.rendered.borrow_mut().insert(index, rendered.clone());
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
    fn scrolling_cache_is_bounded_and_evicted_rows_can_be_rendered_again() {
        let m = Results::default();
        m.replace(
            (0..4096)
                .map(|i| row(&format!("/tmp/{i}.txt"), i))
                .collect(),
        );
        m.set_size_indexed(true);
        for i in 0..m.row_count() {
            assert_eq!(
                m.row_data(i).unwrap().row_data(2).unwrap().text,
                crate::format::format_size(i as u64)
            );
        }
        assert_eq!(m.rendered.borrow().rows.len(), RENDER_CACHE_LIMIT);
        assert_eq!(m.rendered.borrow().order.len(), RENDER_CACHE_LIMIT);
        assert!(!m.rendered.borrow().rows.contains_key(&0));
        assert_eq!(m.row_data(0).unwrap().row_data(2).unwrap().text, "0 B");
        m.set_size_indexed(false);
        assert!(m.rendered.borrow().rows.is_empty());
        assert_eq!(m.row_data(0).unwrap().row_data(2).unwrap().text, "—");
        m.replace(vec![]);
        assert!(m.rendered.borrow().rows.is_empty());
        assert!(m.row_data(0).is_none());
    }
    #[test]
    fn compact_rows_preserve_wire_labels_and_unicode_paths() {
        let mut wire = row("/tmp/日本/é.txt", 42);
        wire.name = "é.txt".into();
        wire.parent = "/tmp/日本".into();
        let compact = Row::from(wire.clone());
        assert!(compact.labels.is_none());
        assert_eq!(compact.name(), wire.name);
        assert_eq!(compact.parent(), wire.parent);
        // The IPC contract can supply labels that differ from the raw path.
        wire.name = "custom label".into();
        let compact = Row::from(wire);
        assert_eq!(compact.name(), "custom label");
        assert_eq!(compact.parent(), "/tmp/日本");
        assert_eq!(Row::split("/root.txt"), ("root.txt", ""));
        assert_eq!(Row::split("relative.txt"), ("relative.txt", ""));
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
        assert_eq!(m.rows.borrow()[1].name(), "child.txt");
        m.rename_path("/tmp/new/child.txt", "/tmp/new/child.pdf");
        assert_eq!(m.rows.borrow()[1].name(), "child.pdf");
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
