//! Result sessions: keep a query's matching IDs in the daemon and serve only
//! the row ranges a client displays. See `toge_core::ipc::session`.

use crate::{
    DaemonState, discover_roots, index_created_path, is_ignored_path, is_within_roots,
    prepare_query_ids, remove_deleted_path, sort_params,
};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use toge_core::config::Config;
use toge_core::index::Index;
use toge_core::ipc::DaemonStatus;
use toge_core::ipc::session::{
    MAX_SESSION_FETCH, MAX_SESSION_FRAME_SIZE, SessionOpen, SessionRequest, SessionResponse,
    SessionRow, SessionState, read_frame, write_frame,
};
use toge_core::query::{Query, Sort};
use toge_core::sort::{OrderCache, SortKey};

/// Clients poll with `Sync`; a session left silent this long is abandoned.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// Live refreshes re-run the whole query, so pace them by its cost.
const SYNC_MIN_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) struct SessionEnv<'a> {
    pub config: &'a Config,
    pub roots: Vec<PathBuf>,
    pub state_dir: &'a Path,
    pub config_dir: &'a Path,
}

impl<'a> SessionEnv<'a> {
    pub fn new(config: &'a Config, state_dir: &'a Path, config_dir: &'a Path) -> Self {
        Self {
            config,
            roots: discover_roots(config),
            state_dir,
            config_dir,
        }
    }
}

pub(crate) struct Session {
    query: Query,
    sort: Option<(SortKey, bool)>,
    ids: Vec<u32>,
    total_size: u64,
    epoch: u64,
    revision: u64,
    generation: u64,
    built_at: Instant,
    build_cost: Duration,
}

fn query_sort(key: SortKey, ascending: bool) -> Sort {
    match (key, ascending) {
        (SortKey::Name, true) => Sort::NameAsc,
        (SortKey::Name, false) => Sort::NameDesc,
        (SortKey::Path, true) => Sort::PathAsc,
        (SortKey::Path, false) => Sort::PathDesc,
        (SortKey::Size, true) => Sort::SizeAsc,
        (SortKey::Size, false) => Sort::SizeDesc,
        (SortKey::Modified, true) => Sort::ModifiedAsc,
        (SortKey::Modified, false) => Sort::ModifiedDesc,
        (SortKey::Created, true) => Sort::CreatedAsc,
        (SortKey::Created, false) => Sort::CreatedDesc,
        (SortKey::Accessed, true) => Sort::AccessedAsc,
        (SortKey::Accessed, false) => Sort::AccessedDesc,
        (SortKey::Extension, true) => Sort::ExtensionAsc,
        (SortKey::Extension, false) => Sort::ExtensionDesc,
    }
}

impl Session {
    pub fn open(
        st: &mut DaemonState,
        open: &SessionOpen,
        index_size: bool,
    ) -> Result<Self, String> {
        if st.status != DaemonStatus::Ready {
            return Err("daemon not ready".into());
        }
        let query = Query::parse(&open.raw).map_err(|error| error.to_string())?;
        let mut session = Self {
            query,
            sort: open.sort,
            ids: Vec::new(),
            total_size: 0,
            epoch: 0,
            revision: 0,
            generation: 0,
            built_at: Instant::now(),
            build_cost: Duration::ZERO,
        };
        session.build(&mut st.index, &mut st.orders, index_size);
        Ok(session)
    }

    pub fn state(&self) -> SessionState {
        SessionState {
            generation: self.generation,
            total_count: self.ids.len(),
            total_size: self.total_size,
        }
    }

    fn build(&mut self, index: &mut Index, orders: &mut OrderCache, index_size: bool) {
        let started = Instant::now();
        let mut query = self.query.clone();
        if let Some((key, ascending)) = self.sort {
            query.sort = query_sort(key, ascending);
        }
        // Fetched rows refresh their own metadata, so re-reading every
        // zero-size match only pays off when the order depends on size.
        let size_sorted = sort_params(query.sort).0 == SortKey::Size;
        self.ids = prepare_query_ids(index, orders, &query, index_size && size_sorted);
        self.recount(index);
        self.epoch = index.epoch();
        self.revision = index.revision();
        self.generation += 1;
        self.built_at = Instant::now();
        self.build_cost = started.elapsed();
    }

    fn recount(&mut self, index: &Index) {
        self.total_size = self
            .ids
            .iter()
            .map(|&id| index.entries[id as usize].size)
            .fold(0, u64::saturating_add);
    }

    /// Retained IDs are only meaningful for the index epoch they came from.
    fn ensure_valid(&mut self, st: &mut DaemonState, index_size: bool) {
        if self.epoch != st.index.epoch() {
            self.build(&mut st.index, &mut st.orders, index_size);
        }
    }

    fn resort(&mut self, st: &mut DaemonState, sort: Option<(SortKey, bool)>, index_size: bool) {
        self.sort = sort;
        if self.epoch != st.index.epoch() {
            self.build(&mut st.index, &mut st.orders, index_size);
            return;
        }
        let DaemonState { index, orders, .. } = st;
        let (key, ascending) = sort.unwrap_or_else(|| sort_params(self.query.sort));
        if matches!(
            key,
            SortKey::Modified | SortKey::Created | SortKey::Accessed
        ) {
            for &id in &self.ids {
                index.update_metadata_by_id(id);
            }
            self.recount(index);
        } else if key == SortKey::Size && index_size {
            // Built without the bulk size refresh; sizes now decide the order.
            for &id in &self.ids {
                let entry = &index.entries[id as usize];
                if !entry.is_dir && entry.size == 0 {
                    index.update_metadata_by_id(id);
                }
            }
            self.recount(index);
        }
        orders.sort(index, &mut self.ids, key, ascending);
        self.generation += 1;
    }

    pub fn handle(
        &mut self,
        st: &mut DaemonState,
        request: SessionRequest,
        env: &SessionEnv,
    ) -> SessionResponse {
        let index_size = env.config.index_size;
        match request {
            SessionRequest::Fetch { offset, len } => {
                if len > MAX_SESSION_FETCH {
                    return SessionResponse::Error("fetch range too large".into());
                }
                self.ensure_valid(st, index_size);
                let start = offset.min(self.ids.len());
                let end = start.saturating_add(len).min(self.ids.len());
                let rows = self.ids[start..end]
                    .iter()
                    .map(|&id| {
                        // Without metadata indexing, stat only the rows being shown.
                        let entry = &st.index.entries[id as usize];
                        if entry.modified == 0 || (index_size && !entry.is_dir && entry.size == 0) {
                            st.index.update_metadata_by_id(id);
                        }
                        let entry = &st.index.entries[id as usize];
                        SessionRow {
                            path: entry.path.clone(),
                            is_dir: entry.is_dir,
                            size: entry.size,
                            modified_unix: entry.modified,
                        }
                    })
                    .collect();
                SessionResponse::Rows {
                    state: self.state(),
                    offset,
                    rows,
                }
            }
            SessionRequest::Resort { sort } => {
                self.resort(st, sort, index_size);
                SessionResponse::State(self.state())
            }
            SessionRequest::Locate { path } => {
                self.ensure_valid(st, index_size);
                let position = st
                    .index
                    .id_by_path(&path)
                    .and_then(|id| self.ids.iter().position(|&candidate| candidate == id));
                SessionResponse::Located {
                    state: self.state(),
                    position,
                }
            }
            SessionRequest::Sync => {
                let due = self.built_at.elapsed() >= SYNC_MIN_INTERVAL.max(self.build_cost * 4);
                if self.epoch != st.index.epoch() || (self.revision != st.index.revision() && due) {
                    self.build(&mut st.index, &mut st.orders, index_size);
                }
                SessionResponse::State(self.state())
            }
            SessionRequest::Reconcile { paths } => {
                for path in &paths {
                    reconcile_path(st, path, env);
                }
                self.build(&mut st.index, &mut st.orders, index_size);
                SessionResponse::State(self.state())
            }
        }
    }
}

/// Bring one path's index entries in line with the filesystem, applying the
/// same root and ignore rules as the watcher.
fn reconcile_path(st: &mut DaemonState, path: &str, env: &SessionEnv) {
    if !Path::new(path).is_absolute() || !is_within_roots(path, &env.roots) {
        return;
    }
    let metadata = std::fs::symlink_metadata(path).ok();
    let is_dir = metadata.as_ref().is_some_and(|metadata| metadata.is_dir());
    if is_ignored_path(path, env.state_dir, env.config_dir, is_dir) {
        return;
    }
    remove_deleted_path(&mut st.index, path);
    if metadata.is_some() {
        index_created_path(st, path, is_dir, env.config);
    }
}

/// True when the client has already hung up, e.g. because a newer keystroke
/// superseded this query. Such queries are skipped instead of queueing behind
/// the index lock ahead of the query the user is waiting for.
fn client_gone(stream: &UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    let mut byte = 0u8;
    // SAFETY: a one-byte peek into a valid local buffer on an owned socket.
    let received = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    received == 0
}

/// Serve one session until the client disconnects or goes idle. The index
/// lock is held only while a single request is answered, never while writing.
pub(crate) fn serve_session(
    stream: &mut UnixStream,
    open: &SessionOpen,
    env: &SessionEnv,
    state: &Mutex<DaemonState>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(IDLE_TIMEOUT))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    if client_gone(stream) {
        return Ok(());
    }
    let opened = {
        let mut st = state.lock().unwrap();
        if client_gone(stream) {
            return Ok(());
        }
        Session::open(&mut st, open, env.config.index_size)
    };
    let mut session = match opened {
        Ok(session) => session,
        Err(error) => return write_frame(stream, &SessionResponse::Error(error).encode()),
    };
    write_frame(stream, &SessionResponse::State(session.state()).encode())?;
    while let Some(bytes) = read_frame(stream, MAX_SESSION_FRAME_SIZE)? {
        let response = match SessionRequest::decode(&bytes) {
            Ok(request) => {
                let mut st = state.lock().unwrap();
                session.handle(&mut st, request, env)
            }
            Err(error) => SessionResponse::Error(error),
        };
        write_frame(stream, &response.encode())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
