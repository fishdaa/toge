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
    pub fn close(&self) {
        self.state.lock().unwrap().closed = true;
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
        crate::client::query_stream(socket, q.id, &q.text, 0, |response, first, done| {
            let selection = selection.clone();
            if !mailbox.current(q.id) {
                return Err(std::io::Error::other("superseded"));
            }
            let m = mailbox.clone();
            let id = q.id;
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
            })
            .map_err(|error| std::io::Error::other(error.to_string()))
        })
    })();
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
    } else if selected != selection.last_selected {
        // A selection made during transfer takes precedence.
        selection.preferred = None;
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
    if first || next != ui.get_selected() {
        ui.invoke_select_row(next);
    }
    selection.last_selected = results.path(next);
    ui.set_has_error(false);
    ui.set_busy(!done);
    ui.set_status(
        format!(
            "Showing {n} of {} matches{}",
            response.total_count,
            if done { "" } else { " — Receiving…" },
        )
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
