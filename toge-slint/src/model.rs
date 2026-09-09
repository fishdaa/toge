use slint::{Model, ModelNotify, ModelRc, ModelTracker, StandardListViewItem, VecModel};
use std::cell::{Cell, RefCell};
use toge_core::ipc::ResultRow;

#[derive(Default)]
pub struct Results {
    rows: RefCell<Vec<ResultRow>>,
    order: RefCell<Vec<usize>>,
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
        *self.rows.borrow_mut() = rows;
        self.resort();
    }
    pub fn sort(&self, column: i32, ascending: bool) {
        self.sort.set(Some((column, ascending)));
        self.resort();
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
        let rows = self.rows.borrow();
        let r = &rows[index];
        let size = if self.size_indexed.get() {
            crate::format::format_size(r.size)
        } else {
            "—".into()
        };
        Some(ModelRc::new(VecModel::from(
            vec![
                r.name.clone(),
                r.parent.clone(),
                size,
                crate::format::format_time(r.modified_unix),
            ]
            .into_iter()
            .map(|text| StandardListViewItem::from(slint::SharedString::from(text)))
            .collect::<Vec<_>>(),
        )))
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
