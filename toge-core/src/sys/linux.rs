use super::{FsWatcher, WatchEvent};
use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Linux filesystem watcher backed by notify's native implementation.
pub struct InotifyWatcher {
    watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<Event>>,
}

impl InotifyWatcher {
    pub fn new() -> io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let watcher = RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(result);
            },
            Config::default(),
        )
        .map_err(notify_error)?;

        Ok(Self { watcher, rx })
    }

    pub(crate) fn map_event(event: Event) -> Vec<WatchEvent> {
        let render = |path: &std::path::PathBuf| path.to_string_lossy().to_string();
        match event.kind {
            EventKind::Create(CreateKind::Any | CreateKind::File | CreateKind::Folder) => event
                .paths
                .into_iter()
                .map(|path| WatchEvent::Create {
                    path: render(&path),
                    is_dir: path.is_dir(),
                })
                .collect(),
            EventKind::Remove(RemoveKind::Any | RemoveKind::File | RemoveKind::Folder) => event
                .paths
                .into_iter()
                .map(|path| WatchEvent::Delete {
                    path: render(&path),
                })
                .collect(),
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() >= 2 => {
                vec![WatchEvent::Move {
                    from: render(&event.paths[0]),
                    to: render(&event.paths[1]),
                }]
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::From))
            | EventKind::Modify(ModifyKind::Name(RenameMode::Any))
                if event.paths.len() == 1 =>
            {
                vec![WatchEvent::Delete {
                    path: render(&event.paths[0]),
                }]
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) if event.paths.len() == 1 => {
                let path = &event.paths[0];
                vec![WatchEvent::Create {
                    path: render(path),
                    is_dir: path.is_dir(),
                }]
            }
            EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Metadata(_))
            | EventKind::Modify(ModifyKind::Other)
            | EventKind::Modify(ModifyKind::Any) => event
                .paths
                .into_iter()
                .map(|path| WatchEvent::Modify {
                    path: render(&path),
                })
                .collect(),
            EventKind::Any | EventKind::Other => Vec::new(),
            _ => Vec::new(),
        }
    }
}

impl FsWatcher for InotifyWatcher {
    fn watch(&mut self, path: &Path) -> io::Result<()> {
        self.watcher
            .watch(path, RecursiveMode::NonRecursive)
            .map_err(notify_error)
    }

    fn unwatch(&mut self, path: &Path) -> io::Result<()> {
        self.watcher.unwatch(path).map_err(notify_error)
    }

    fn poll_events(&mut self) -> io::Result<Vec<WatchEvent>> {
        let mut events = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(Ok(event)) => events.extend(Self::map_event(event)),
                Ok(Err(err)) => return Err(notify_error(err)),
                Err(TryRecvError::Empty) => return Ok(events),
                Err(TryRecvError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "watch event channel disconnected",
                    ))
                }
            }
        }
    }
}

fn notify_error(err: notify::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}
