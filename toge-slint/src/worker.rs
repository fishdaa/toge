use crate::model::{Command, Focus, PAGE, Results};
use slint::Model as _;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use toge_core::ipc::session::{SessionOpen, SessionRequest, SessionResponse, SessionState};
use toge_core::sort::SortKey;

/// How often an idle session asks the daemon whether its results changed.
const SYNC_INTERVAL: Duration = Duration::from_secs(1);
/// Fetches queued behind a fast scroll are dropped except for the latest few.
const FETCH_BACKLOG: usize = 4;
/// How often the status bar refreshes the daemon's index summary.
const INDEX_STATUS_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct Query {
    pub id: u64,
    pub text: String,
    pub due: Instant,
}
#[derive(Default)]
struct State {
    generation: u64,
    pending: Option<Query>,
    closed: bool,
    active: Option<(u64, std::os::unix::net::UnixStream)>,
    sort: Option<(i32, bool)>,
}
#[derive(Default)]
pub struct Mailbox {
    state: Mutex<State>,
    wake: Condvar,
}
impl Mailbox {
    pub fn submit(&self, text: String, immediate: bool) {
        let mut s = self.state.lock().unwrap();
        s.generation += 1;
        if let Some((_, socket)) = s.active.take() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
        s.pending = Some(Query {
            id: s.generation,
            text,
            due: Instant::now()
                + if immediate {
                    Duration::ZERO
                } else {
                    Duration::from_millis(100)
                },
        });
        self.wake.notify_one();
    }
    /// Table sort used when the next session opens.
    pub fn set_sort(&self, sort: Option<(i32, bool)>) {
        self.state.lock().unwrap().sort = sort;
    }
    pub fn sort(&self) -> Option<(i32, bool)> {
        self.state.lock().unwrap().sort
    }
    pub fn closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }
    pub fn current(&self, id: u64) -> bool {
        let s = self.state.lock().unwrap();
        !s.closed && s.generation == id
    }
    fn register(&self, id: u64, socket: &std::os::unix::net::UnixStream) -> std::io::Result<()> {
        let mut s = self.state.lock().unwrap();
        if s.closed || s.generation != id {
            return Err(std::io::Error::other("superseded"));
        }
        s.active = Some((id, socket.try_clone()?));
        Ok(())
    }
    fn finish(&self, id: u64) {
        let mut s = self.state.lock().unwrap();
        if s.active.as_ref().is_some_and(|(active, _)| *active == id) {
            s.active = None;
        }
    }
    pub fn close(&self) {
        let mut s = self.state.lock().unwrap();
        s.closed = true;
        if let Some((_, socket)) = s.active.take() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
        self.wake.notify_one();
    }
    fn next(&self) -> Option<Query> {
        let mut s = self.state.lock().unwrap();
        loop {
            if s.closed {
                return None;
            }
            if let Some(q) = &s.pending {
                let remaining = q.due.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return s.pending.take();
                }
                s = self.wake.wait_timeout(s, remaining).unwrap().0;
            } else {
                s = self.wake.wait(s).unwrap();
            }
        }
    }
}

/// Table columns map to daemon sort keys; anything else keeps the query's order.
pub fn sort_key(sort: Option<(i32, bool)>) -> Option<(SortKey, bool)> {
    let (column, ascending) = sort?;
    let key = match column {
        0 => SortKey::Name,
        1 => SortKey::Path,
        2 => SortKey::Size,
        3 => SortKey::Modified,
        _ => return None,
    };
    Some((key, ascending))
}

pub fn start(mailbox: Arc<Mailbox>, ui: slint::Weak<crate::AppWindow>) {
    // Serializes only the daemon-launch check, not query handling: each dispatched
    // query runs on its own thread so a slow/stale query (e.g. a broad substring
    // scan that takes ~1s server-side) can't stall a fresher one that supersedes
    // it mid-flight, the way a single shared worker thread would.
    let daemon_start = Arc::new(Mutex::new(()));
    {
        let (ui, socket, mailbox) = (ui.clone(), crate::client::socket_path(), mailbox.clone());
        std::thread::spawn(move || poll_index_status(ui, socket, &mailbox));
    }
    std::thread::spawn(move || {
        let socket = crate::client::socket_path();
        while let Some(q) = mailbox.next() {
            let mailbox = mailbox.clone();
            let ui = ui.clone();
            let socket = socket.clone();
            let daemon_start = daemon_start.clone();
            std::thread::spawn(move || run_session(q, &mailbox, &ui, &socket, &daemon_start));
        }
    });
}

fn wait_until_ready(
    q: &Query,
    mailbox: &Arc<Mailbox>,
    ui: &slint::Weak<crate::AppWindow>,
    socket: &std::path::Path,
    daemon_start: &Arc<Mutex<()>>,
) -> std::io::Result<()> {
    {
        let _guard = daemon_start.lock().unwrap();
        crate::client::ensure_daemon_running(socket)?;
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if !mailbox.current(q.id) {
            return Err(std::io::Error::other("superseded"));
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Daemon is still busy or indexing. Retry shortly.",
            ));
        }
        let status = match crate::client::status(socket) {
            Ok(status) => status,
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(error) => return Err(error),
        };
        if status.status == toge_core::ipc::DaemonStatus::Ready {
            return Ok(());
        }
        let message = format!("{:?}: {}", status.status, status.status_message);
        let m = mailbox.clone();
        let id = q.id;
        let _ = ui.upgrade_in_event_loop(move |ui| {
            if m.current(id) {
                ui.set_status(message.into());
            }
        });
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Keep only the latest few fetches of a backlog, preserving command order.
fn coalesce(commands: Vec<Command>) -> Vec<Command> {
    let mut kept_fetches = Vec::new();
    for command in commands.iter().rev() {
        if let Command::Fetch(page) = command
            && !kept_fetches.contains(page)
            && kept_fetches.len() < FETCH_BACKLOG
        {
            kept_fetches.push(*page);
        }
    }
    let mut seen = Vec::new();
    commands
        .into_iter()
        .filter(|command| match command {
            Command::Fetch(page) => {
                if kept_fetches.contains(page) && !seen.contains(page) {
                    seen.push(*page);
                    true
                } else {
                    false
                }
            }
            _ => true,
        })
        .collect()
}

/// What the UI should do with a daemon response besides adopting its state.
enum Reply {
    Rows {
        offset: usize,
        rows: Vec<toge_core::ipc::session::SessionRow>,
    },
    Located {
        focus: Focus,
        position: Option<usize>,
    },
    Rebuilt {
        rows: Vec<toge_core::ipc::session::SessionRow>,
    },
    Synced {
        rows: Option<Vec<toge_core::ipc::session::SessionRow>>,
    },
}

fn run_session(
    q: Query,
    mailbox: &Arc<Mailbox>,
    ui: &slint::Weak<crate::AppWindow>,
    socket: &std::path::Path,
    daemon_start: &Arc<Mutex<()>>,
) {
    let size_indexed = config_size_indexed();
    let outcome = (|| {
        wait_until_ready(&q, mailbox, ui, socket, daemon_start)?;
        let sort = mailbox.sort();
        let mut session = crate::client::open_session(
            socket,
            SessionOpen {
                raw: q.text.clone(),
                sort: sort_key(sort),
            },
            |socket| mailbox.register(q.id, socket),
            |rows| {
                if !mailbox.current(q.id) {
                    return Err(std::io::Error::other("superseded"));
                }
                let (m, id) = (mailbox.clone(), q.id);
                ui.upgrade_in_event_loop(move |ui| {
                    if m.current(id) {
                        results(&ui).set_size_indexed(size_indexed);
                        if results(&ui).preview(rows, ui.get_selected()) {
                            ui.invoke_select_row(-1);
                        }
                        ui.set_busy(true);
                        ui.set_status("Searching… (preview)".into());
                    }
                })
                .map_err(|error| std::io::Error::other(error.to_string()))
            },
        )?;
        // Keep the preview until the sorted first page is ready. Opening only
        // supplies IDs/totals; fetching may rebuild after an index mutation.
        let rows = first_page(&mut session)?;
        let (commands, inbox) = channel();
        let (m, id, state) = (mailbox.clone(), q.id, session.state());
        ui.upgrade_in_event_loop(move |ui| {
            if m.current(id) {
                opened(&ui, &m, commands, state, rows, sort, size_indexed);
            }
        })
        .map_err(|error| std::io::Error::other(error.to_string()))?;
        serve_commands(&q, mailbox, ui, &mut session, &inbox)
    })();
    mailbox.finish(q.id);
    if let Err(error) = outcome {
        let m = mailbox.clone();
        let _ = ui.upgrade_in_event_loop(move |ui| {
            if m.current(q.id) {
                results(&ui).detach();
                ui.set_busy(false);
                ui.set_has_error(true);
                ui.set_status(format!("{error} — Retry to reconnect").into());
            }
        });
    }
}

fn first_page(
    session: &mut toge_core::ipc::session::SessionClient<std::os::unix::net::UnixStream>,
) -> std::io::Result<Vec<toge_core::ipc::session::SessionRow>> {
    match session.request(&SessionRequest::Fetch {
        offset: 0,
        len: PAGE,
    })? {
        SessionResponse::Rows { rows, .. } => Ok(rows),
        _ => Err(std::io::Error::other("unexpected first page response")),
    }
}

fn serve_commands(
    q: &Query,
    mailbox: &Arc<Mailbox>,
    ui: &slint::Weak<crate::AppWindow>,
    session: &mut toge_core::ipc::session::SessionClient<std::os::unix::net::UnixStream>,
    inbox: &Receiver<Command>,
) -> std::io::Result<()> {
    loop {
        if !mailbox.current(q.id) {
            return Ok(());
        }
        let first = match inbox.recv_timeout(SYNC_INTERVAL) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            // The UI attached a newer session or closed.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        let Some(first) = first else {
            let before = session.state();
            session.request(&SessionRequest::Sync)?;
            let rows = if session.state() == before {
                None
            } else {
                Some(first_page(session)?)
            };
            post(mailbox, ui, q.id, session.state(), Reply::Synced { rows })?;
            continue;
        };
        let backlog = std::iter::once(first).chain(inbox.try_iter()).collect();
        for command in coalesce(backlog) {
            if !mailbox.current(q.id) {
                return Ok(());
            }
            match command {
                Command::Action {
                    generation,
                    offset,
                    len,
                    action,
                } => {
                    let outcome =
                        selection_paths(session, generation, offset, len, || mailbox.current(q.id));
                    let state = session.state();
                    let m = mailbox.clone();
                    let id = q.id;
                    ui.upgrade_in_event_loop(move |ui| {
                        if !m.current(id) {
                            return;
                        }
                        match outcome {
                            Ok(paths) if results(&ui).generation() == generation => {
                                ui.set_status(
                                    status_text(state, results(&ui).size_indexed.get()).into(),
                                );
                                ui.invoke_resolved_action(
                                    action.into(),
                                    slint::ModelRc::new(slint::VecModel::from(
                                        paths
                                            .into_iter()
                                            .map(slint::SharedString::from)
                                            .collect::<Vec<_>>(),
                                    )),
                                );
                            }
                            Ok(_) => ui.set_status(
                                "Results changed — select the items again and retry".into(),
                            ),
                            Err(error) => ui.set_status(format!("Action failed: {error}").into()),
                        }
                    })
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                }
                Command::Fetch(page) => {
                    let request = SessionRequest::Fetch {
                        offset: page * PAGE,
                        len: PAGE,
                    };
                    if let SessionResponse::Rows { offset, rows, .. } = session.request(&request)? {
                        post(
                            mailbox,
                            ui,
                            q.id,
                            session.state(),
                            Reply::Rows { offset, rows },
                        )?;
                    }
                }
                Command::Resort(sort) => {
                    session.request(&SessionRequest::Resort {
                        sort: sort_key(sort),
                    })?;
                    let rows = first_page(session)?;
                    post(mailbox, ui, q.id, session.state(), Reply::Rebuilt { rows })?;
                }
                Command::Locate { path, focus } => {
                    locate(session, mailbox, ui, q.id, path, focus)?;
                }
                Command::Reconcile { paths, select } => {
                    reconcile(session, &paths)?;
                    let rows = first_page(session)?;
                    post(mailbox, ui, q.id, session.state(), Reply::Rebuilt { rows })?;
                    match select {
                        Some(path) => locate(session, mailbox, ui, q.id, path, Focus::Scroll)?,
                        None => post(
                            mailbox,
                            ui,
                            q.id,
                            session.state(),
                            Reply::Located {
                                focus: Focus::Scroll,
                                position: None,
                            },
                        )?,
                    }
                }
            }
        }
    }
}

/// Collect only the action's paths, leaving the bounded display cache alone.
/// Abort if any batch belongs to a rebuilt order; mixing generations could
/// otherwise rename, open or delete items the user did not select.
fn selection_paths<S: std::io::Read + std::io::Write>(
    session: &mut toge_core::ipc::session::SessionClient<S>,
    generation: u64,
    offset: usize,
    len: usize,
    current: impl Fn() -> bool,
) -> std::io::Result<Vec<String>> {
    use toge_core::ipc::session::MAX_SESSION_FETCH;
    let end = offset
        .checked_add(len)
        .filter(|&end| end <= session.state().total_count)
        .ok_or_else(|| std::io::Error::other("Selection is no longer available"))?;
    let mut paths = Vec::new();
    let mut start = offset;
    while start < end {
        if !current() || session.state().generation != generation {
            return Err(std::io::Error::other(
                "Results changed — select the items again and retry",
            ));
        }
        let count = (end - start).min(MAX_SESSION_FETCH);
        match session.request(&SessionRequest::Fetch {
            offset: start,
            len: count,
        })? {
            SessionResponse::Rows {
                state,
                offset,
                rows,
            } if state.generation == generation && offset == start && rows.len() == count => {
                paths.extend(rows.into_iter().map(|row| row.path));
            }
            _ => {
                return Err(std::io::Error::other(
                    "Results changed — select the items again and retry",
                ));
            }
        }
        start += count;
    }
    Ok(paths)
}

/// Keep a single UI action within the wire protocol's per-request path limit.
/// The caller publishes one refreshed page after every batch has completed.
fn reconcile<S: std::io::Read + std::io::Write>(
    session: &mut toge_core::ipc::session::SessionClient<S>,
    paths: &[String],
) -> std::io::Result<()> {
    for batch in paths.chunks(toge_core::ipc::session::MAX_SESSION_RECONCILE) {
        session.request(&SessionRequest::Reconcile {
            paths: batch.to_vec(),
        })?;
    }
    Ok(())
}

fn locate(
    session: &mut toge_core::ipc::session::SessionClient<std::os::unix::net::UnixStream>,
    mailbox: &Arc<Mailbox>,
    ui: &slint::Weak<crate::AppWindow>,
    id: u64,
    path: String,
    focus: Focus,
) -> std::io::Result<()> {
    if let SessionResponse::Located { position, .. } =
        session.request(&SessionRequest::Locate { path })?
    {
        post(
            mailbox,
            ui,
            id,
            session.state(),
            Reply::Located { focus, position },
        )?;
    }
    Ok(())
}

fn post(
    mailbox: &Arc<Mailbox>,
    ui: &slint::Weak<crate::AppWindow>,
    id: u64,
    state: SessionState,
    reply: Reply,
) -> std::io::Result<()> {
    let m = mailbox.clone();
    ui.upgrade_in_event_loop(move |ui| {
        if m.current(id) {
            apply(&ui, state, reply);
        }
    })
    .map_err(|error| std::io::Error::other(error.to_string()))
}

pub fn results(ui: &crate::AppWindow) -> impl std::ops::Deref<Target = Results> + '_ {
    struct Handle(slint::ModelRc<slint::ModelRc<slint::StandardListViewItem>>);
    impl std::ops::Deref for Handle {
        type Target = Results;
        fn deref(&self) -> &Results {
            self.0.as_any().downcast_ref::<Results>().unwrap()
        }
    }
    Handle(ui.get_rows())
}

fn status_text(state: SessionState, size_indexed: bool) -> String {
    crate::format::search_status(state.total_count, state.total_size, size_indexed)
}

/// Keep the status bar's index summary current, as toge-gui does every 3s.
/// Only reads daemon status; searches are what start the daemon. Stops when
/// the window closes.
fn poll_index_status(
    ui: slint::Weak<crate::AppWindow>,
    socket: std::path::PathBuf,
    mailbox: &Mailbox,
) {
    while !mailbox.closed() {
        let text = match crate::client::status(&socket) {
            Ok(status) => crate::format::index_status(&status),
            Err(_) => "Index unavailable".to_string(),
        };
        let posted = ui.upgrade_in_event_loop(move |ui| ui.set_index_status(text.into()));
        if posted.is_err() {
            return;
        }
        std::thread::sleep(INDEX_STATUS_INTERVAL);
    }
}

fn opened(
    ui: &crate::AppWindow,
    mailbox: &Mailbox,
    commands: std::sync::mpsc::Sender<Command>,
    state: SessionState,
    rows: Vec<toge_core::ipc::session::SessionRow>,
    sort: Option<(i32, bool)>,
    size_indexed: bool,
) {
    let results = results(ui);
    let previous = results
        .take_preview_selection()
        .or_else(|| results.path(ui.get_selected()));
    results.set_size_indexed(size_indexed);
    results.attach(commands, state);
    results.fill(state.generation, 0, rows);
    // A header click while the session was opening was sent to the old session.
    if mailbox.sort() != sort {
        results.send(Command::Resort(mailbox.sort()));
    }
    ui.set_has_error(false);
    ui.set_busy(false);
    ui.set_status(status_text(state, size_indexed).into());
    match previous {
        // The previously selected path may still match; keep it selected.
        Some(path) => {
            results.send(Command::Locate {
                path,
                focus: Focus::Scroll,
            });
        }
        None => ui.invoke_select_row(if state.total_count > 0 { 0 } else { -1 }),
    }
    follow_rename(ui, &results);
}

/// Re-attach an open inline rename editor to its path's new position.
fn follow_rename(ui: &crate::AppWindow, results: &Results) {
    if !ui.get_rename_path().is_empty() {
        results.send(Command::Locate {
            path: ui.get_rename_path().to_string(),
            focus: Focus::Rename,
        });
    }
}

fn apply(ui: &crate::AppWindow, state: SessionState, reply: Reply) {
    let results = results(ui);
    let selected = results.path(ui.get_selected());
    if results.apply_state(state) {
        // Live refreshes keep the selected file selected wherever it moved.
        // Explicit rebuilds (sort, rename, delete) set the selection themselves
        // and keep the action's status message.
        if !matches!(reply, Reply::Rebuilt { .. }) {
            ui.set_status(status_text(state, results.size_indexed.get()).into());
            if let Some(path) = selected {
                results.send(Command::Locate {
                    path,
                    focus: Focus::Keep,
                });
            }
            follow_rename(ui, &results);
        }
    }
    let total = results.total() as i32;
    match reply {
        Reply::Rows { offset, rows } => results.fill(state.generation, offset, rows),
        Reply::Located { focus, position } => {
            let position = position.map(|p| p as i32);
            match focus {
                Focus::Scroll => {
                    ui.invoke_select_row(position.unwrap_or(if total > 0 { 0 } else { -1 }));
                }
                Focus::Keep => match position {
                    Some(position) => ui.set_selected(position),
                    None if ui.get_selected() >= total => ui.set_selected(total - 1),
                    None => {}
                },
                Focus::Rename => crate::actions::place_rename(ui, position),
            }
        }
        Reply::Rebuilt { rows } | Reply::Synced { rows: Some(rows) } => {
            results.fill(state.generation, 0, rows);
            if ui.get_selected() >= total {
                ui.set_selected(total - 1);
            }
        }
        Reply::Synced { rows: None } => {}
    }
    ui.set_has_error(false);
}

fn config_size_indexed() -> bool {
    let root = std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        }, std::path::PathBuf::from);
    toge_core::config::Config::load(&root.join("toge/config.toml"))
        .unwrap_or_else(|_| toge_core::config::Config::default_config())
        .index_size
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_spans_more_pages_than_the_display_cache_and_rejects_rebuilds() {
        use toge_core::ipc::session::{
            MAX_SESSION_FRAME_SIZE, SessionClient, SessionRow, read_frame, write_frame,
        };
        for rebuild in [false, true] {
            let (client, mut server) = std::os::unix::net::UnixStream::pair().unwrap();
            let daemon = std::thread::spawn(move || {
                read_frame(&mut server, MAX_SESSION_FRAME_SIZE)
                    .unwrap()
                    .unwrap();
                let mut state = SessionState {
                    generation: 1,
                    total_count: 6000,
                    total_size: 0,
                };
                write_frame(&mut server, &SessionResponse::State(state).encode()).unwrap();
                let mut batches = 0;
                while let Some(bytes) = read_frame(&mut server, MAX_SESSION_FRAME_SIZE).unwrap() {
                    let SessionRequest::Fetch { offset, len } =
                        SessionRequest::decode(&bytes).unwrap()
                    else {
                        panic!("expected fetch");
                    };
                    assert!(len <= toge_core::ipc::session::MAX_SESSION_FETCH);
                    batches += 1;
                    if rebuild && batches == 2 {
                        state.generation += 1;
                    }
                    let rows = (offset..offset + len)
                        .map(|i| SessionRow {
                            path: format!("/fixture/{i}.txt"),
                            is_dir: false,
                            size: 0,
                            modified_unix: 0,
                        })
                        .collect();
                    write_frame(
                        &mut server,
                        &SessionResponse::Rows {
                            state,
                            offset,
                            rows,
                        }
                        .encode(),
                    )
                    .unwrap();
                }
                batches
            });
            let mut session = SessionClient::open(
                client,
                SessionOpen {
                    raw: String::new(),
                    sort: None,
                },
            )
            .unwrap();
            let paths = selection_paths(&mut session, 1, 17, 5000, || true);
            if rebuild {
                assert!(paths.unwrap_err().to_string().contains("Results changed"));
            } else {
                let paths = paths.unwrap();
                assert_eq!(paths.len(), 5000);
                assert_eq!(paths.first().unwrap(), "/fixture/17.txt");
                assert_eq!(paths.last().unwrap(), "/fixture/5016.txt");
                assert!(selection_paths(&mut session, 1, 0, 1, || false).is_err());
            }
            drop(session);
            assert_eq!(daemon.join().unwrap(), if rebuild { 2 } else { 5 });
        }
    }

    #[test]
    fn large_reconciliation_keeps_the_session_usable() {
        use toge_core::ipc::session::{
            MAX_SESSION_FRAME_SIZE, MAX_SESSION_RECONCILE, SessionClient, read_frame, write_frame,
        };
        let (client, mut server) = std::os::unix::net::UnixStream::pair().unwrap();
        let paths: Vec<_> = (0..=(MAX_SESSION_RECONCILE * 2))
            .map(|i| format!("/fixture/{i}.txt"))
            .collect();
        let expected = paths.clone();
        let daemon = std::thread::spawn(move || {
            let bytes = read_frame(&mut server, MAX_SESSION_FRAME_SIZE)
                .unwrap()
                .unwrap();
            assert!(matches!(
                toge_core::ipc::Request::decode(&bytes).unwrap(),
                toge_core::ipc::Request::OpenSession(_)
            ));
            let mut state = SessionState {
                generation: 1,
                total_count: 0,
                total_size: 0,
            };
            write_frame(&mut server, &SessionResponse::State(state).encode()).unwrap();
            let mut received = Vec::new();
            for expected_len in [MAX_SESSION_RECONCILE, MAX_SESSION_RECONCILE, 1] {
                let bytes = read_frame(&mut server, MAX_SESSION_FRAME_SIZE)
                    .unwrap()
                    .unwrap();
                let SessionRequest::Reconcile { paths } = SessionRequest::decode(&bytes).unwrap()
                else {
                    panic!("expected reconciliation");
                };
                assert_eq!(paths.len(), expected_len);
                received.extend(paths);
                state.generation += 1;
                write_frame(&mut server, &SessionResponse::State(state).encode()).unwrap();
            }
            assert_eq!(received, expected);
            let bytes = read_frame(&mut server, MAX_SESSION_FRAME_SIZE)
                .unwrap()
                .unwrap();
            assert_eq!(
                SessionRequest::decode(&bytes).unwrap(),
                SessionRequest::Sync
            );
            write_frame(&mut server, &SessionResponse::State(state).encode()).unwrap();
        });
        let mut session = SessionClient::open(
            client,
            SessionOpen {
                raw: String::new(),
                sort: None,
            },
        )
        .unwrap();
        reconcile(&mut session, &paths).unwrap();
        session.request(&SessionRequest::Sync).unwrap();
        assert_eq!(session.state().generation, 4);
        daemon.join().unwrap();
    }

    #[test]
    fn latest_edit_invalidates_active_query_before_debounce() {
        let m = Mailbox::default();
        m.submit("old".into(), true);
        let old = m.next().unwrap();
        m.submit("new".into(), false);
        assert!(!m.current(old.id));
        m.submit("newest".into(), true);
        assert_eq!(m.next().unwrap().text, "newest");
        m.close();
        assert!(m.next().is_none());
    }

    #[test]
    fn fast_scroll_backlog_keeps_only_the_latest_distinct_fetches_in_order() {
        let locate = Command::Locate {
            path: "/a".into(),
            focus: Focus::Keep,
        };
        let backlog = vec![
            Command::Fetch(0),
            Command::Fetch(1),
            locate.clone(),
            Command::Fetch(2),
            Command::Fetch(3),
            Command::Fetch(4),
            Command::Fetch(5),
            Command::Fetch(4),
        ];
        assert_eq!(
            coalesce(backlog),
            [
                locate,
                Command::Fetch(2),
                Command::Fetch(3),
                Command::Fetch(4),
                Command::Fetch(5),
            ]
        );
    }

    #[test]
    fn table_columns_map_to_daemon_sort_keys() {
        assert_eq!(sort_key(Some((0, true))), Some((SortKey::Name, true)));
        assert_eq!(sort_key(Some((1, false))), Some((SortKey::Path, false)));
        assert_eq!(sort_key(Some((2, true))), Some((SortKey::Size, true)));
        assert_eq!(sort_key(Some((3, false))), Some((SortKey::Modified, false)));
        assert_eq!(sort_key(Some((4, true))), None);
        assert_eq!(sort_key(None), None);
        let m = Mailbox::default();
        assert_eq!(m.sort(), None);
        m.set_sort(Some((2, false)));
        assert_eq!(m.sort(), Some((2, false)));
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::net::UnixStream;

    #[test]
    fn editing_or_closing_shuts_down_active_socket_without_waiting_for_a_batch() {
        let mailbox = Mailbox::default();
        mailbox.submit("old".into(), true);
        let old = mailbox.next().unwrap();
        let (socket, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        mailbox.register(old.id, &socket).unwrap();
        mailbox.submit("new".into(), true);
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
        assert!(mailbox.register(old.id, &socket).is_err());
        let new = mailbox.next().unwrap();
        let (socket, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        mailbox.register(new.id, &socket).unwrap();
        // A stale worker finishing must not remove the current socket.
        mailbox.finish(old.id);
        mailbox.close();
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
        assert!(mailbox.register(new.id, &socket).is_err());
    }
}
