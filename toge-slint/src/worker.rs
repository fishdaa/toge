use slint::Model;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

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

pub fn start(mailbox: Arc<Mailbox>, ui: slint::Weak<crate::AppWindow>) {
    // Serializes only the daemon-launch check, not query handling: each dispatched
    // query runs on its own thread so a slow/stale query (e.g. a broad substring
    // scan that takes ~1s server-side) can't stall a fresher one that supersedes
    // it mid-flight, the way a single shared worker thread would.
    let daemon_start = Arc::new(Mutex::new(()));
    std::thread::spawn(move || {
        let socket = crate::client::socket_path();
        while let Some(q) = mailbox.next() {
            let mailbox = mailbox.clone();
            let ui = ui.clone();
            let socket = socket.clone();
            let daemon_start = daemon_start.clone();
            std::thread::spawn(move || run_query(q, &mailbox, &ui, &socket, &daemon_start));
        }
    });
}

fn run_query(
    q: Query,
    mailbox: &Arc<Mailbox>,
    ui: &slint::Weak<crate::AppWindow>,
    socket: &std::path::Path,
    daemon_start: &Arc<Mutex<()>>,
) {
    let size_indexed = config_size_indexed();
    let outcome = (|| {
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
                break;
            }
            let message = format!("{:?}: {}", status.status, status.status_message);
            let m = mailbox.clone();
            let id = q.id;
            let _ = ui.upgrade_in_event_loop(move |ui| {
                if m.current(id) {
                    ui.set_status(message.into());
                }
            });
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Daemon is still indexing. Retry shortly.",
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let selection = Arc::new(Mutex::new(StreamSelection::default()));
        crate::client::query_stream(
            socket,
            q.id,
            &q.text,
            0,
            |socket| mailbox.register(q.id, socket),
            |response, first, done| {
                let selection = selection.clone();
                if !mailbox.current(q.id) {
                    return Err(std::io::Error::other("superseded"));
                }
                let m = mailbox.clone();
                let id = q.id;
                // Wait until the UI applies this batch before reading another one.
                // This bounds queued row data and lets socket backpressure reach the daemon.
                let (applied, wait) = std::sync::mpsc::sync_channel(1);
                ui.upgrade_in_event_loop(move |ui| {
                    if !m.current(id) {
                        return;
                    }
                    apply_batch(
                        &ui,
                        response,
                        first,
                        done,
                        size_indexed,
                        &mut selection.lock().unwrap(),
                    );
                    let _ = applied.send(());
                })
                .map_err(|error| std::io::Error::other(error.to_string()))?;
                loop {
                    if !mailbox.current(id) {
                        return Err(std::io::Error::other("superseded"));
                    }
                    match wait.recv_timeout(Duration::from_millis(50)) {
                        Ok(()) => return Ok(()),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => return Err(std::io::Error::other("UI closed")),
                    }
                }
            },
        )
    })();
    mailbox.finish(q.id);
    if let Err(error) = outcome {
        let m = mailbox.clone();
        let _ = ui.upgrade_in_event_loop(move |ui| {
            if m.current(q.id) {
                ui.set_busy(false);
                ui.set_has_error(true);
                ui.set_status(format!("{error} — Retry to reconnect").into());
            }
        });
    }
}

#[derive(Default)]
struct StreamSelection {
    preferred: Option<String>,
    last_selected: Option<String>,
    restore_scroll: bool,
}

fn apply_batch(
    ui: &crate::AppWindow,
    response: toge_core::ipc::ResultsResponse,
    first: bool,
    done: bool,
    size_indexed: bool,
    selection: &mut StreamSelection,
) {
    let model = ui.get_rows();
    let results = model
        .as_any()
        .downcast_ref::<crate::model::Results>()
        .unwrap();
    let selected = results.path(ui.get_selected());
    if first {
        // The previously selected path may arrive in a later batch.
        selection.preferred = selected.clone();
        selection.restore_scroll = false;
    } else if selected != selection.last_selected {
        // A selection made during transfer takes precedence.
        selection.preferred = None;
        selection.restore_scroll = false;
    }
    results.set_size_indexed(size_indexed);
    if first {
        results.replace(response.rows);
    } else {
        results.append(response.rows);
    }
    crate::actions::sync_rename(ui, results);
    let n = results.row_count();
    let preferred_index = selection
        .preferred
        .as_deref()
        .map_or(-1, |p| results.find(p));
    let index = if preferred_index >= 0 {
        selection.preferred = None;
        selection.restore_scroll = true;
        preferred_index
    } else {
        selected.as_deref().map_or(-1, |p| results.find(p))
    };
    // Avoid reselecting on ordinary appends, which would reset the user's scroll.
    let next = if index >= 0 {
        index
    } else if n > 0 {
        0
    } else {
        -1
    };
    if first || next != ui.get_selected() || (done && selection.restore_scroll) {
        // The virtual table's row-height estimate can change as batches arrive.
        // Reposition a restored selection once the final model size is known.
        ui.invoke_select_row(next);
    }
    if done {
        selection.restore_scroll = false;
    }
    selection.last_selected = results.path(next);
    ui.set_has_error(false);
    ui.set_busy(!done);
    ui.set_status(
        if done {
            format!("Showing {n} of {} matches", response.total_count)
        } else {
            format!("Received {n} matches — Searching…")
        }
        .into(),
    );
}

fn config_size_indexed() -> bool {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    toge_core::config::Config::load(&root.join("toge/config.toml"))
        .unwrap_or_else(|_| toge_core::config::Config::default_config())
        .index_size
}
#[cfg(test)]
mod tests {
    use super::*;
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
