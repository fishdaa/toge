use slint::{Model, ModelNotify, ModelRc, ModelTracker, StandardListViewItem, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use toge_core::ipc::session::{SessionRow, SessionState};

/// Rows are fetched from the daemon in pages of this size.
pub const PAGE: usize = 256;
/// Loaded pages kept around, so memory stays bounded however far the user scrolls.
const PAGE_LIMIT: usize = 16;
/// A page requested this long ago without arriving may be requested again.
const REQUEST_RETRY: Duration = Duration::from_secs(2);

/// How a located row should be selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// Select and scroll to the row (new query, rename).
    Scroll,
    /// Update the selection index without moving the viewport (live refresh).
    Keep,
    /// Re-attach the inline rename editor.
    Rename,
}

/// Requests from the UI thread to the thread that owns the daemon session.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Fetch(usize),
    /// Table column and direction, or `None` for the query's own order.
    Resort(Option<(i32, bool)>),
    Locate {
        path: String,
        focus: Focus,
    },
    /// Re-read changed paths, then select `select` (or clear the selection).
    Reconcile {
        paths: Vec<String>,
        select: Option<String>,
    },
}

struct Row {
    path: Box<str>,
    size: u64,
    modified_unix: i64,
}
impl Row {
    fn split(path: &str) -> (&str, &str) {
        path.rsplit_once('/')
            .map_or((path, ""), |(parent, name)| (name, parent))
    }
}
impl From<SessionRow> for Row {
    fn from(row: SessionRow) -> Self {
        Self {
            path: row.path.into_boxed_str(),
            size: row.size,
            modified_unix: row.modified_unix,
        }
    }
}

#[derive(Default)]
struct Pages {
    loaded: HashMap<usize, Vec<Row>>,
    /// Least recently used first.
    order: VecDeque<usize>,
    requested: HashMap<usize, Instant>,
}
impl Pages {
    fn touch(&mut self, page: usize) {
        if self.order.back() != Some(&page) {
            self.order.retain(|&p| p != page);
            self.order.push_back(page);
        }
    }
}

/// A window onto results held by the daemon. Only pages near what the table
/// displays are loaded; other rows render as blank placeholders until their
/// page arrives.
#[derive(Default)]
pub struct Results {
    generation: Cell<u64>,
    preview_selection: RefCell<Option<String>>,
    total: Cell<usize>,
    pages: RefCell<Pages>,
    session: RefCell<Option<Sender<Command>>>,
    placeholder: RefCell<Option<ModelRc<StandardListViewItem>>>,
    pub size_indexed: Cell<bool>,
    notify: ModelNotify,
}
impl Results {
    /// Attach a newly opened session. Rows from any previous session are dropped.
    pub fn attach(&self, session: Sender<Command>, state: SessionState) {
        *self.session.borrow_mut() = Some(session);
        self.reset(state);
    }

    /// Display a bounded provisional page while the query is still being built.
    pub fn preview(&self, rows: Vec<SessionRow>, selected: i32) -> bool {
        let first = self.generation.get() != 0 || self.session.borrow().is_some();
        if self.session.borrow().is_some() {
            *self.preview_selection.borrow_mut() = self.path(selected);
        }
        self.detach();
        self.reset(SessionState {
            generation: 0,
            total_count: rows.len(),
            total_size: 0,
        });
        self.fill(0, 0, rows);
        first
    }

    pub fn take_preview_selection(&self) -> Option<String> {
        self.preview_selection.borrow_mut().take()
    }

    /// Stop sending requests to a session that is gone.
    pub fn detach(&self) {
        self.session.borrow_mut().take();
    }

    pub fn send(&self, command: Command) -> bool {
        self.session
            .borrow()
            .as_ref()
            .is_some_and(|session| session.send(command).is_ok())
    }

    pub fn total(&self) -> usize {
        self.total.get()
    }

    /// Adopt a (possibly rebuilt) daemon state. Returns true when the results
    /// were rebuilt and every loaded row was discarded.
    pub fn apply_state(&self, state: SessionState) -> bool {
        if state.generation == self.generation.get() && state.total_count == self.total.get() {
            return false;
        }
        self.reset(state);
        true
    }

    fn reset(&self, state: SessionState) {
        self.generation.set(state.generation);
        self.total.set(state.total_count);
        *self.pages.borrow_mut() = Pages::default();
        self.notify.reset();
    }

    /// Store fetched rows. Rows from an older generation are discarded.
    pub fn fill(&self, generation: u64, offset: usize, rows: Vec<SessionRow>) {
        if generation != self.generation.get() || !offset.is_multiple_of(PAGE) {
            return;
        }
        let page = offset / PAGE;
        let count = rows.len().min(self.total.get().saturating_sub(offset));
        {
            let mut pages = self.pages.borrow_mut();
            pages.requested.remove(&page);
            pages
                .loaded
                .insert(page, rows.into_iter().take(count).map(Row::from).collect());
            pages.touch(page);
            while pages.loaded.len() > PAGE_LIMIT {
                let Some(oldest) = pages.order.pop_front() else {
                    break;
                };
                pages.loaded.remove(&oldest);
            }
        }
        for row in offset..offset + count {
            self.notify.row_changed(row);
        }
    }

    pub fn path(&self, index: i32) -> Option<String> {
        let index = usize::try_from(index).ok()?;
        self.pages
            .borrow()
            .loaded
            .get(&(index / PAGE))?
            .get(index % PAGE)
            .map(|row| row.path.to_string())
    }

    pub fn set_size_indexed(&self, value: bool) {
        if self.size_indexed.replace(value) != value {
            self.notify.reset();
        }
    }

    fn request(&self, pages: &mut Pages, page: usize) {
        if page * PAGE >= self.total.get() || pages.loaded.contains_key(&page) {
            return;
        }
        let now = Instant::now();
        if pages
            .requested
            .get(&page)
            .is_some_and(|at| now.duration_since(*at) < REQUEST_RETRY)
        {
            return;
        }
        if self.send(Command::Fetch(page)) {
            pages.requested.insert(page, now);
        }
    }

    fn placeholder(&self) -> ModelRc<StandardListViewItem> {
        self.placeholder
            .borrow_mut()
            .get_or_insert_with(|| {
                ModelRc::new(VecModel::from(vec![StandardListViewItem::default(); 4]))
            })
            .clone()
    }
}
impl Model for Results {
    type Data = ModelRc<StandardListViewItem>;
    fn row_count(&self) -> usize {
        self.total.get()
    }
    fn row_data(&self, row: usize) -> Option<Self::Data> {
        if row >= self.total.get() {
            return None;
        }
        let page = row / PAGE;
        let mut pages = self.pages.borrow_mut();
        let Some(r) = pages
            .loaded
            .get(&page)
            .and_then(|rows| rows.get(row % PAGE))
        else {
            self.request(&mut pages, page);
            return Some(self.placeholder());
        };
        let (name, parent) = Row::split(&r.path);
        let size = if self.size_indexed.get() {
            crate::format::format_size(r.size)
        } else {
            "—".into()
        };
        let cells = [
            name.to_string(),
            parent.to_string(),
            size,
            crate::format::format_time(r.modified_unix),
        ];
        pages.touch(page);
        // Prefetch neighbours so ordinary scrolling rarely shows placeholders.
        if row % PAGE >= PAGE / 2 {
            self.request(&mut pages, page + 1);
        } else if page > 0 {
            self.request(&mut pages, page - 1);
        }
        Some(ModelRc::new(VecModel::from(
            cells
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
    use std::sync::mpsc::{Receiver, channel};

    fn state(generation: u64, total_count: usize) -> SessionState {
        SessionState {
            generation,
            total_count,
            total_size: 0,
        }
    }
    fn rows(offset: usize, count: usize) -> Vec<SessionRow> {
        (offset..offset + count)
            .map(|i| SessionRow {
                path: format!("/tmp/dir/{i}.txt"),
                is_dir: false,
                size: i as u64,
                modified_unix: 0,
            })
            .collect()
    }
    fn attached(total: usize) -> (Results, Receiver<Command>) {
        let (tx, rx) = channel();
        let model = Results::default();
        model.attach(tx, state(1, total));
        (model, rx)
    }
    fn text(model: &Results, row: usize, column: usize) -> String {
        model
            .row_data(row)
            .unwrap()
            .row_data(column)
            .unwrap()
            .text
            .to_string()
    }

    #[test]
    fn preview_is_visible_without_fetching_and_final_state_discards_it() {
        let (model, rx) = attached(10);
        model.fill(1, 0, rows(0, 10));
        model.preview(rows(20, 32), 3);
        assert_eq!(model.row_count(), 32);
        assert_eq!(text(&model, 0, 0), "20.txt");
        assert!(!model.send(Command::Fetch(1)));
        model.preview(rows(20, 64), -1);
        assert_eq!(
            model.take_preview_selection().as_deref(),
            Some("/tmp/dir/3.txt")
        );
        assert_eq!(rx.try_iter().count(), 0);
        let (tx, _) = channel();
        model.attach(tx, state(1, 100));
        assert!(model.path(0).is_none());
        model.fill(1, 0, rows(0, 100));
        assert_eq!(text(&model, 0, 0), "0.txt");
    }

    #[test]
    fn unloaded_rows_are_placeholders_that_request_their_page_once() {
        let (model, rx) = attached(PAGE * 3 + 5);
        assert_eq!(model.row_count(), PAGE * 3 + 5);
        assert_eq!(text(&model, PAGE + 3, 0), "");
        assert_eq!(text(&model, PAGE + 4, 0), "");
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [Command::Fetch(1)]);
        assert!(model.row_data(PAGE * 3 + 5).is_none());
        model.fill(1, PAGE, rows(PAGE, PAGE));
        assert_eq!(text(&model, PAGE + 3, 0), format!("{}.txt", PAGE + 3));
        assert_eq!(text(&model, PAGE + 3, 1), "/tmp/dir");
        assert_eq!(
            model.path((PAGE + 3) as i32).unwrap(),
            format!("/tmp/dir/{}.txt", PAGE + 3)
        );
        // Rendering the first half of a page prefetches the previous page.
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [Command::Fetch(0)]);
        assert!(model.path(0).is_none());
        assert!(model.path(-1).is_none());
    }

    #[test]
    fn stale_generations_are_ignored_and_rebuilds_drop_loaded_rows() {
        let (model, rx) = attached(10);
        model.fill(0, 0, rows(0, 10));
        assert!(model.path(0).is_none());
        model.fill(1, 0, rows(0, 10));
        assert!(model.path(9).is_some());
        assert!(!model.apply_state(state(1, 10)));
        assert!(model.apply_state(state(2, 4)));
        assert_eq!(model.row_count(), 4);
        assert!(model.path(0).is_none());
        let _ = model.row_data(0);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [Command::Fetch(0)]);
        // Rows past the new total are never exposed.
        model.fill(2, 0, rows(0, 10));
        assert!(model.path(3).is_some());
        assert!(model.path(4).is_none());
    }

    #[test]
    fn loaded_pages_are_bounded_and_evicted_pages_are_fetched_again() {
        let total = PAGE * (PAGE_LIMIT + 4);
        let (model, rx) = attached(total);
        for page in 0..PAGE_LIMIT + 4 {
            model.fill(1, page * PAGE, rows(page * PAGE, PAGE));
        }
        assert_eq!(model.pages.borrow().loaded.len(), PAGE_LIMIT);
        assert!(model.path(0).is_none());
        assert!(model.path((total - 1) as i32).is_some());
        let _ = rx.try_iter().count();
        assert_eq!(text(&model, 0, 0), "");
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [Command::Fetch(0)]);
    }

    #[test]
    fn size_column_follows_size_indexing_and_detached_models_stop_requesting() {
        let (model, rx) = attached(PAGE * 2);
        model.fill(1, 0, rows(0, PAGE));
        assert_eq!(text(&model, 3, 2), "—");
        model.set_size_indexed(true);
        assert_eq!(text(&model, 3, 2), crate::format::format_size(3));
        let _ = rx.try_iter().count();
        model.detach();
        let _ = model.row_data(PAGE + 1);
        assert!(!model.send(Command::Fetch(1)));
        assert!(rx.try_iter().next().is_none());
    }
}
