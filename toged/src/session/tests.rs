use super::*;
use crate::WatcherStatus;
use std::fs;
use toge_core::ipc::session::SessionClient;

fn daemon(paths: &[(&str, u64)]) -> DaemonState {
    let mut index = Index::new();
    for (path, size) in paths {
        index.insert_with_metadata(path, false, *size, 1_700_000_000 + *size as i64, 1, 1);
    }
    DaemonState {
        index,
        status: DaemonStatus::Ready,
        status_message: String::new(),
        build_duration_ms: 0,
        last_updated_unix: 0,
        watcher: WatcherStatus::default(),
        watcher_log: Vec::new(),
        orders: toge_core::sort::OrderCache::default(),
        index_generation: 0,
    }
}

fn open(st: &mut DaemonState, raw: &str, sort: Option<(SortKey, bool)>) -> Session {
    Session::open(
        st,
        &SessionOpen {
            raw: raw.into(),
            sort,
        },
        false,
    )
    .unwrap()
}

fn fetch_paths(
    session: &mut Session,
    st: &mut DaemonState,
    env: &SessionEnv,
    offset: usize,
    len: usize,
) -> Vec<String> {
    match session.handle(st, SessionRequest::Fetch { offset, len }, env) {
        SessionResponse::Rows { rows, .. } => rows.into_iter().map(|row| row.path).collect(),
        other => panic!("unexpected {other:?}"),
    }
}

fn locate(
    session: &mut Session,
    st: &mut DaemonState,
    env: &SessionEnv,
    path: &str,
) -> Option<usize> {
    match session.handle(st, SessionRequest::Locate { path: path.into() }, env) {
        SessionResponse::Located { position, .. } => position,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn session_serves_ranges_resorts_and_locates_without_copying_results() {
    let config = Config::default_config();
    let dir = Path::new("/nonexistent");
    let env = SessionEnv::new(&config, dir, dir);
    let mut st = daemon(&[
        ("/r/b.mkv", 20),
        ("/r/a.mkv", 30),
        ("/r/c.txt", 1),
        ("/r/d.mkv", 10),
    ]);
    let mut session = open(&mut st, ".mkv", Some((SortKey::Name, true)));
    assert_eq!(
        session.state(),
        SessionState {
            generation: 1,
            total_count: 3,
            total_size: 60
        }
    );
    assert_eq!(
        fetch_paths(&mut session, &mut st, &env, 0, 2),
        ["/r/a.mkv", "/r/b.mkv"]
    );
    assert_eq!(
        fetch_paths(&mut session, &mut st, &env, 2, 10),
        ["/r/d.mkv"]
    );
    assert!(fetch_paths(&mut session, &mut st, &env, 9, 10).is_empty());
    assert!(matches!(
        session.handle(
            &mut st,
            SessionRequest::Fetch {
                offset: 0,
                len: MAX_SESSION_FETCH + 1
            },
            &env
        ),
        SessionResponse::Error(_)
    ));

    let resorted = session.handle(
        &mut st,
        SessionRequest::Resort {
            sort: Some((SortKey::Size, false)),
        },
        &env,
    );
    assert_eq!(resorted.state().unwrap().generation, 2);
    assert_eq!(
        fetch_paths(&mut session, &mut st, &env, 0, 3),
        ["/r/a.mkv", "/r/b.mkv", "/r/d.mkv"]
    );
    assert_eq!(locate(&mut session, &mut st, &env, "/r/d.mkv"), Some(2));
    assert_eq!(locate(&mut session, &mut st, &env, "/r/c.txt"), None);
    assert_eq!(locate(&mut session, &mut st, &env, "/missing"), None);
}

#[test]
fn removals_rebuild_before_stale_ids_can_be_served() {
    let config = Config::default_config();
    let dir = Path::new("/nonexistent");
    let env = SessionEnv::new(&config, dir, dir);
    let mut st = daemon(&[("/r/a.mkv", 1), ("/r/b.mkv", 2), ("/r/z.txt", 3)]);
    let mut session = open(&mut st, ".mkv", Some((SortKey::Name, true)));
    // Removing an early entry swaps the last entry (z.txt) into its ID slot.
    assert!(st.index.remove("/r/a.mkv"));
    let response = session.handle(&mut st, SessionRequest::Fetch { offset: 0, len: 8 }, &env);
    let SessionResponse::Rows { state, rows, .. } = response else {
        panic!("expected rows");
    };
    assert_eq!(state.generation, 2);
    assert_eq!(state.total_count, 1);
    assert_eq!(rows[0].path, "/r/b.mkv");
}

#[test]
fn sync_picks_up_additions_only_after_the_refresh_interval() {
    let config = Config::default_config();
    let dir = Path::new("/nonexistent");
    let env = SessionEnv::new(&config, dir, dir);
    let mut st = daemon(&[("/r/a.mkv", 1)]);
    let mut session = open(&mut st, ".mkv", None);
    st.index
        .insert_with_metadata("/r/new.mkv", false, 5, 1_700_000_005, 1, 1);
    let state = session
        .handle(&mut st, SessionRequest::Sync, &env)
        .state()
        .unwrap();
    assert_eq!((state.generation, state.total_count), (1, 1));
    session.built_at -= SYNC_MIN_INTERVAL;
    let state = session
        .handle(&mut st, SessionRequest::Sync, &env)
        .state()
        .unwrap();
    assert_eq!((state.generation, state.total_count), (2, 2));
    // Nothing changed since: no rebuild, the generation is stable.
    session.built_at -= SYNC_MIN_INTERVAL;
    let state = session
        .handle(&mut st, SessionRequest::Sync, &env)
        .state()
        .unwrap();
    assert_eq!(state.generation, 2);
}

#[test]
fn reindex_replacement_invalidates_sessions() {
    let config = Config::default_config();
    let dir = Path::new("/nonexistent");
    let env = SessionEnv::new(&config, dir, dir);
    let mut st = daemon(&[("/r/a.mkv", 1), ("/r/b.mkv", 2)]);
    let mut session = open(&mut st, ".mkv", Some((SortKey::Name, true)));
    let mut replacement = Index::new();
    replacement.insert_with_metadata("/r/c.mkv", false, 3, 1_700_000_003, 1, 1);
    replacement.succeed(&st.index);
    st.index = replacement;
    assert_eq!(fetch_paths(&mut session, &mut st, &env, 0, 8), ["/r/c.mkv"]);
}

#[test]
fn reconcile_applies_client_changes_within_roots_only() {
    let root = tempfile::Builder::new()
        .prefix("toged-session-")
        .tempdir_in(std::env::temp_dir())
        .unwrap();
    let outside = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let mut config = Config::default_config();
    config.roots = vec![root.path().to_path_buf()];
    let state_dir = root.path().join(".state");
    let env = SessionEnv::new(&config, &state_dir, &state_dir);
    let old = root.path().join("old.mkv");
    let new = root.path().join("new.mkv");
    let stray = outside.path().join("stray.mkv");
    fs::write(&old, b"x").unwrap();
    fs::write(&stray, b"x").unwrap();
    let old = old.to_str().unwrap();
    let new_path = new.to_str().unwrap();
    let mut st = daemon(&[]);
    st.index.insert(old, false);
    let mut session = open(&mut st, ".mkv", None);
    assert_eq!(session.state().total_count, 1);

    fs::rename(old, &new).unwrap();
    let state = session
        .handle(
            &mut st,
            SessionRequest::Reconcile {
                paths: vec![old.into(), new_path.into(), stray.to_str().unwrap().into()],
            },
            &env,
        )
        .state()
        .unwrap();
    assert_eq!(state.total_count, 1);
    assert_eq!(fetch_paths(&mut session, &mut st, &env, 0, 8), [new_path]);
    assert!(st.index.id_by_path(stray.to_str().unwrap()).is_none());

    fs::remove_file(&new).unwrap();
    let state = session
        .handle(
            &mut st,
            SessionRequest::Reconcile {
                paths: vec![new_path.into(), "relative.mkv".into()],
            },
            &env,
        )
        .state()
        .unwrap();
    assert_eq!(state.total_count, 0);
}

#[test]
fn served_session_answers_until_the_client_disconnects() {
    let config = Config::default_config();
    let state = Mutex::new(daemon(&[("/r/a.mkv", 1), ("/r/b.mkv", 2)]));
    let (client, mut daemon_side) = UnixStream::pair().unwrap();
    std::thread::scope(|scope| {
        let server = scope.spawn(|| {
            let bytes = read_frame(&mut daemon_side, 1 << 20).unwrap().unwrap();
            let toge_core::ipc::Request::OpenSession(open) =
                toge_core::ipc::Request::decode(&bytes).unwrap()
            else {
                panic!("expected open");
            };
            let dir = Path::new("/nonexistent");
            let env = SessionEnv::new(&config, dir, dir);
            serve_session(&mut daemon_side, &open, &env, &state)
        });
        let mut session = SessionClient::open(
            client,
            SessionOpen {
                raw: ".mkv".into(),
                sort: Some((SortKey::Size, false)),
            },
        )
        .unwrap();
        assert_eq!(session.state().total_count, 2);
        let SessionResponse::Rows { rows, .. } = session
            .request(&SessionRequest::Fetch { offset: 1, len: 1 })
            .unwrap()
        else {
            panic!("expected rows");
        };
        assert_eq!(rows[0].path, "/r/a.mkv");
        // The index lock is free between requests.
        drop(state.lock().unwrap());
        drop(session);
        server.join().unwrap().unwrap();
    });
}

#[test]
fn opening_before_ready_reports_an_error() {
    let mut st = daemon(&[]);
    st.status = DaemonStatus::Indexing;
    assert!(
        Session::open(
            &mut st,
            &SessionOpen {
                raw: String::new(),
                sort: None
            },
            false
        )
        .is_err()
    );
}

#[test]
fn superseded_open_is_skipped_without_running_the_query() {
    let config = Config::default_config();
    let state = Mutex::new(daemon(&[("/r/a.mkv", 1)]));
    let (client, mut daemon_side) = UnixStream::pair().unwrap();
    client.shutdown(std::net::Shutdown::Both).unwrap();
    assert!(client_gone(&daemon_side));
    let dir = Path::new("/nonexistent");
    let env = SessionEnv::new(&config, dir, dir);
    let open = SessionOpen {
        raw: ".mkv".into(),
        sort: None,
    };
    serve_session(&mut daemon_side, &open, &env, &state).unwrap();
    // No session was built, so the shared order cache was never populated.
    assert!(format!("{:?}", state.lock().unwrap().orders).contains("name: None"));
    let (live, _peer) = UnixStream::pair().unwrap();
    assert!(!client_gone(&live));
}

#[test]
fn progressive_open_previews_before_sort_and_keeps_exact_final_results() {
    let paths: Vec<_> = (0..600)
        .rev()
        .map(|i| (format!("/r/{i:04}.txt"), i as u64))
        .collect();
    let refs: Vec<_> = paths
        .iter()
        .map(|(path, size)| (path.as_str(), *size))
        .collect();
    let mut st = daemon(&refs);
    let request = SessionOpen {
        raw: "txt".into(),
        sort: Some((SortKey::Name, true)),
    };
    let mut previews = Vec::new();
    let session = Session::open_preview(&mut st, &request, false, |index, ids| {
        assert!(ids.len() <= SESSION_PREVIEW_ROWS);
        if !ids.is_empty() {
            previews.push(index.entries[ids[0] as usize].path.clone());
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(previews[0], "/r/0599.txt");
    assert_eq!(session.state().total_count, 600);
    assert_eq!(session.state().total_size, (0..600u64).sum());
    assert_eq!(
        st.index.entries[session.ids[0] as usize].path,
        "/r/0000.txt"
    );
    assert_eq!(session.ids, open(&mut st, "txt", request.sort).ids);
    let mut calls = 0;
    let error = Session::open_preview(&mut st, &request, false, |_, _| {
        calls += 1;
        Err(io::Error::other("cancelled"))
    })
    .err()
    .unwrap();
    assert_eq!(error.to_string(), "cancelled");
    assert_eq!(calls, 1);
}

#[test]
fn progressive_wire_session_switches_from_preview_to_sorted_pages() {
    let config = Config::default_config();
    let state = Mutex::new(daemon(&[("/r/z.txt", 2), ("/r/a.txt", 1)]));
    std::thread::scope(|scope| {
        let (client, mut server) = UnixStream::pair().unwrap();
        let (state, config) = (&state, &config);
        let daemon = scope.spawn(move || {
            let bytes = read_frame(&mut server, MAX_SESSION_FRAME_SIZE)?.unwrap();
            let toge_core::ipc::Request::OpenSessionPreview(open) =
                toge_core::ipc::Request::decode(&bytes).unwrap()
            else {
                panic!("expected progressive open")
            };
            let dir = Path::new("/nonexistent");
            let env = SessionEnv::new(config, dir, dir);
            serve_session_with_preview(&mut server, &open, &env, state, true)
        });
        let mut previewed = false;
        let mut session = SessionClient::open_with_preview(
            client,
            SessionOpen {
                raw: "txt".into(),
                sort: Some((SortKey::Name, true)),
            },
            |rows| {
                assert_eq!(rows[0].path, "/r/z.txt");
                previewed = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(previewed);
        assert_eq!(session.state().total_count, 2);
        let SessionResponse::Rows { rows, .. } = session
            .request(&SessionRequest::Fetch { offset: 0, len: 2 })
            .unwrap()
        else {
            panic!("expected sorted rows")
        };
        assert_eq!(rows[0].path, "/r/a.txt");
        drop(session);
        daemon.join().unwrap().unwrap();
    });
}

#[test]
fn date_sessions_use_indexed_metadata_and_sync_watcher_changes() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "a").unwrap();
    fs::write(&b, "b").unwrap();
    let (a, b) = (a.to_str().unwrap(), b.to_str().unwrap());
    let mut st = daemon(&[(a, 1), (b, 2)]);
    // Deliberately distinct indexed timestamps: querying must not overwrite
    // watcher-maintained values with another full filesystem scan.
    st.index.insert_with_metadata(a, false, 1, 20, 30, 40);
    st.index.insert_with_metadata(b, false, 2, 10, 20, 30);
    let mut session = open(&mut st, "", Some((SortKey::Modified, true)));
    assert_eq!(
        st.index.entries[st.index.id_by_path(a).unwrap() as usize].modified,
        20
    );
    assert_eq!(session.ids[0], st.index.id_by_path(b).unwrap());
    let config = Config::default_config();
    let env = SessionEnv::new(&config, dir.path(), dir.path());
    session.resort(&mut st, Some((SortKey::Modified, false)), true);
    assert_eq!(session.ids[0], st.index.id_by_path(a).unwrap());
    // The watcher updates an existing entry rather than adding/removing it.
    st.index.insert_with_metadata(b, false, 2, 50, 20, 30);
    session.built_at = Instant::now() - Duration::from_secs(2);
    session.handle(&mut st, SessionRequest::Sync, &env);
    assert_eq!(session.ids[0], st.index.id_by_path(b).unwrap());
    let before = st.index.entries[st.index.id_by_path(a).unwrap() as usize].modified;
    Session::open_preview(
        &mut st,
        &SessionOpen {
            raw: "".into(),
            sort: Some((SortKey::Modified, true)),
        },
        true,
        |_, _| Ok(()),
    )
    .unwrap();
    assert_eq!(
        st.index.entries[st.index.id_by_path(a).unwrap() as usize].modified,
        before
    );
}
