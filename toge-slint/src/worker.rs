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
    std::thread::spawn(move || {
        let socket = crate::client::socket_path();
        while let Some(q) = mailbox.next() {
            let outcome = (|| {
                crate::client::ensure_daemon_running(&socket)?;
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    if !mailbox.current(q.id) {
                        return Err(std::io::Error::other("superseded"));
                    }
                    let status = crate::client::status(&socket)?;
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
                crate::client::query(&socket, q.id, &q.text, 10_000, 0)
            })();
            let m = mailbox.clone();
            let size_indexed = config_size_indexed();
            let _ = ui.upgrade_in_event_loop(move |ui| {
                if !m.current(q.id) {
                    return;
                }
                ui.set_busy(false);
                match outcome {
                    Ok(response) => {
                        ui.set_has_error(false);
                        let model = ui.get_rows();
                        let results = model
                            .as_any()
                            .downcast_ref::<crate::model::Results>()
                            .unwrap();
                        let selected = results.path(ui.get_selected());
                        let n = response.rows.len();
                        results.size_indexed.set(size_indexed);
                        results.replace(response.rows);
                        let index = selected.as_deref().map_or(-1, |p| results.find(p));
                        ui.invoke_select_row(if index >= 0 {
                            index
                        } else if n > 0 {
                            0
                        } else {
                            -1
                        });
                        ui.set_status(
                            format!("Showing {n} of {} matches", response.total_count).into(),
                        );
                    }
                    Err(error) => {
                        ui.set_has_error(true);
                        ui.set_status(format!("{error} — Retry to reconnect").into());
                    }
                }
            });
        }
    });
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
