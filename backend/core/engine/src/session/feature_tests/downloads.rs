// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn a_same_url_downloads_navigation_invalidates_its_previous_render_cache() {
    let mut tabs = TabManager::new(100.0, 100.0);
    let tab_id = tabs.default_tab();
    let mut refresher = DownloadsRefresher::default();
    refresher
        .seen_url
        .insert(tab_id, "about:downloads".to_string());
    refresher
        .rendered
        .insert(tab_id, "<p>old successful list</p>".to_string());
    refresher.visit.insert(tab_id, 41);
    refresher
        .due
        .insert(tab_id, Instant::now() + Duration::from_secs(1));

    let dir = temp_frame_dir("same-downloads-url");
    std::fs::create_dir_all(&dir).unwrap();
    let (tx, _rx) = mpsc::channel();
    let mut wire = Vec::new();
    let mut generation = 0;
    begin_gated_navigation(
        &mut tabs,
        &mut wire,
        &dir,
        &mut generation,
        Some(tab_id.as_u64()),
        None,
        tab_id,
        "about:downloads".to_string(),
        PendingKind::Navigate,
        &mut HashMap::new(),
        &mut refresher,
        &tx,
        Path::new("/unused"),
        None,
    )
    .unwrap();

    assert_eq!(tabs.get(tab_id).unwrap().url(), Some("about:downloads"));
    assert_eq!(refresher.visit[&tab_id], 42);
    assert!(!refresher.rendered.contains_key(&tab_id));
    assert!(!refresher.seen_url.contains_key(&tab_id));
    assert!(!refresher.due.contains_key(&tab_id));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn one_transient_downloads_failure_keeps_the_last_successful_render() {
    let mut tabs = TabManager::new(100.0, 100.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<p>last successful list</p>",
        Some("about:downloads".to_string()),
    );
    let mut refresher = DownloadsRefresher::default();
    refresher.visit.insert(tab_id, 1);
    refresher
        .rendered
        .insert(tab_id, "<p>last successful list</p>".to_string());
    let dir = temp_frame_dir("downloads-transient-failure");
    std::fs::create_dir_all(&dir).unwrap();
    let mut wire = Vec::new();
    let mut generation = 0;

    refresher
        .apply(
            &mut tabs,
            &mut wire,
            &dir,
            &mut generation,
            DownloadsListing {
                tab_id,
                visit: 1,
                url: "about:downloads".to_string(),
                outcome: Err("temporary timeout".to_string()),
            },
        )
        .unwrap();

    assert_eq!(generation, 0);
    assert!(tabs
        .get(tab_id)
        .unwrap()
        .dom_dump()
        .contains("last successful list"));

    refresher
        .apply(
            &mut tabs,
            &mut wire,
            &dir,
            &mut generation,
            DownloadsListing {
                tab_id,
                visit: 1,
                url: "about:downloads".to_string(),
                outcome: Err("temporary timeout".to_string()),
            },
        )
        .unwrap();

    assert_eq!(generation, 1);
    assert!(tabs
        .get(tab_id)
        .unwrap()
        .dom_dump()
        .contains("The downloads service is not running"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigating_to_about_downloads_replies_at_once_and_shows_the_live_list() {
    let dir = DownloadsScratch::new("sess-nav");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "alpha.iso",
            TransferState::Active,
            400,
        )])),
        ..FakeState::default()
    };
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-nav", dir.socket());

    navigate_to(&mut client, "about:downloads");
    let dom = dom_text(&mut client);
    assert!(
        dom.contains("alpha.iso") && dom.contains("Downloading"),
        "{dom}"
    );
    finish_session(client, handle);
}

#[test]
fn the_page_updates_itself_and_pushes_a_frame_when_a_transfer_changes() {
    let dir = DownloadsScratch::new("sess-live");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "alpha.iso",
            TransferState::Active,
            400,
        )])),
        ..FakeState::default()
    };
    let live = state.transfers.clone();
    let lists = state.lists.clone();
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-live", dir.socket());
    let initial_lists = lists.load(Ordering::SeqCst);
    let first = navigate_to(&mut client, "about:downloads");
    let initial_refresh = next_refresh(&mut client);
    assert!(initial_refresh > first);
    assert!(lists.load(Ordering::SeqCst) > initial_lists);

    *live.lock().unwrap() = vec![
        dl(1, "alpha.iso", TransferState::Completed, 1000),
        dl(2, "beta.zip", TransferState::Active, 100),
    ];
    let pushed = next_refresh(&mut client);
    assert!(
        pushed > first,
        "the pushed frame is newer: {pushed} vs {first}"
    );

    // The human-visible frame and the AI-facing representation come from the same render pass.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert_eq!(
        snapshot.generation, pushed,
        "the frame just pushed and the representation share one generation"
    );
    let dom = dom_text(&mut client);
    assert!(
        dom.contains("Completed") && dom.contains("beta.zip"),
        "{dom}"
    );
    finish_session(client, handle);
}

#[test]
fn a_busy_downloads_tab_cannot_age_out_another_tabs_frame_or_generation() {
    let dir = DownloadsScratch::new("sess-tab-frame-isolation");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "progressing.iso",
            TransferState::Active,
            400,
        )])),
        ..FakeState::default()
    };
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-tab-frame-isolation", dir.socket());

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    let ServerMessage::FrameReady {
        shm_path: mut quiet_path,
        generation: mut quiet_generation,
        ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected the credits tab's initial frame")
    };
    assert_eq!(quiet_generation, 1);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some("about:downloads".to_string()),
        },
    )
    .unwrap();
    let ServerMessage::TabOpened {
        tab_id: downloads_tab,
        ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected downloads tab")
    };
    loop {
        let (tab_id, _, message) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        if matches!(message, ServerMessage::FrameReady { .. }) && tab_id == Some(downloads_tab) {
            break;
        }
    }

    // Resizing the one physical window now eagerly reflows both tabs.
    // More than the old global retention window's worth of downloads-tab
    // renders must still leave tab one's *latest* frame on disk. The fake
    // process has an active transfer, so this is the same two-tab shape
    // as a live panel; deterministic resizes avoid a wall-clock wait for
    // five poll ticks.
    for width in 201..=205 {
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(downloads_tab),
            None,
            &ClientMessage::Resize { width, height: 300 },
        )
        .unwrap();
        let mut saw_downloads_frame = false;
        let mut saw_quiet_frame = false;
        while !saw_downloads_frame || !saw_quiet_frame {
            let (tab_id, _, message) =
                blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
            match message {
                ServerMessage::FrameReady {
                    width: rendered_width,
                    height: 300,
                    ..
                } if tab_id == Some(downloads_tab) && rendered_width == width => {
                    saw_downloads_frame = true;
                }
                ServerMessage::FrameReady {
                    shm_path,
                    width: rendered_width,
                    height: 300,
                    generation,
                } if tab_id == Some(1) && rendered_width == width => {
                    quiet_path = shm_path;
                    quiet_generation = generation;
                    saw_quiet_frame = true;
                }
                ServerMessage::FrameReady { .. } => {}
                other => panic!("unexpected message while resizing tabs: {other:?}"),
            }
        }
    }

    assert!(
        blueice_ipc::shm::map_frame(Path::new(&quiet_path)).is_ok(),
        "another tab's frame must survive its busy neighbor"
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    loop {
        match blueice_ipc::read_server_message(&mut client).unwrap() {
            ServerMessage::Representation(snapshot) => {
                assert_eq!(
                    snapshot.generation, quiet_generation,
                    "tab one's snapshot must still name its own last frame"
                );
                break;
            }
            ServerMessage::FrameReady { .. } => {} // a downloads refresh may arrive first
            other => panic!("unexpected message {other:?}"),
        }
    }

    finish_session(client, handle);
}

#[test]
fn leaving_the_downloads_page_forgets_its_future_polling_state() {
    let tabs = TabManager::new(100.0, 100.0);
    let tab_id = tabs.default_tab();
    let mut refresher = DownloadsRefresher::default();
    refresher
        .seen_url
        .insert(tab_id, "about:downloads".to_string());
    refresher.due.insert(tab_id, Instant::now());
    refresher
        .rendered
        .insert(tab_id, "<p>last render</p>".to_string());
    refresher.fresh_visit.insert(tab_id);
    refresher.visit.insert(tab_id, 1);
    refresher.consecutive_failures.insert(tab_id, 1);
    let (tx, _rx) = mpsc::channel();

    // The default page is about:blank. No helper thread or sleep is
    // needed to prove it cannot schedule another socket read.
    refresher.tick(&tabs, &tx, Instant::now());

    assert!(!refresher.seen_url.contains_key(&tab_id));
    assert!(!refresher.due.contains_key(&tab_id));
    assert!(!refresher.rendered.contains_key(&tab_id));
    assert!(!refresher.fresh_visit.contains(&tab_id));
    assert!(!refresher.consecutive_failures.contains_key(&tab_id));
}

#[test]
fn a_hung_downloads_service_cannot_stall_the_session() {
    let dir = DownloadsScratch::new("sess-hung");
    let state = FakeState::default();
    let stalls = state.stalls.clone();
    let _server = fake_downloads_live(&dir.socket(), state, true, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-hung", dir.socket());
    navigate_to(&mut client, "about:credits");

    navigate_to(&mut client, "about:downloads");
    assert!(
        dom_text(&mut client).contains("not running"),
        "a service that does not answer is shown as unavailable"
    );

    // One connection is the navigation's short read; the second is the
    // open page's background poll. Wait until the latter has genuinely
    // reached its non-responsive peer before asking the session to
    // resize. This proves the non-blocking boundary without making an
    // assertion about wall-clock scheduling or font-loading speed.
    let deadline = Instant::now() + Duration::from_secs(6);
    while stalls.load(Ordering::SeqCst) < 2 {
        assert!(
            Instant::now() < deadline,
            "the downloads refresher never reached the hung service"
        );
        thread::sleep(Duration::from_millis(10));
    }

    // The background fetch is now known to be blocked in the fake
    // service. An ordinary request still gets its normal, correlated
    // frame reply from the session loop.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 123,
            height: 234,
        },
    )
    .unwrap();
    loop {
        match blueice_ipc::read_server_message(&mut client).unwrap() {
            ServerMessage::FrameReady {
                width: 123,
                height: 234,
                ..
            } => break,
            ServerMessage::FrameReady { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    finish_session(client, handle);
}

#[test]
fn a_link_to_about_downloads_is_followed_like_any_built_in_page() {
    let dir = DownloadsScratch::new("sess-link");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "alpha.iso",
            TransferState::Completed,
            1000,
        )])),
        ..FakeState::default()
    };
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let frame_dir = temp_frame_dir("sess-link");
    let gatekeeper = clearing_gatekeeper("sess-link");
    let (mut client, mut server) = client_pair();
    let socket = dir.socket();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(400.0, 300.0);
        tabs.set_downloads_source(Arc::new(DownloadsSource::without_spawner(socket)));
        default_page(&mut tabs)
            .load_html_str(r#"<a href="about:downloads">Open downloads</a>"#, None);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &frame_dir,
            &mut generation,
            &gatekeeper,
        )
        .unwrap();
    });
    handshake(&mut client);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 2.0, y: 2.0 })
        .unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let ServerMessage::Navigated { url, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Navigated")
    };
    assert_eq!(url, "about:downloads");
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetDom).unwrap();
    loop {
        match blueice_ipc::read_server_message(&mut client).unwrap() {
            ServerMessage::Dom(text) => {
                assert!(
                    text.contains("alpha.iso"),
                    "the built-in link shows the downloads list: {text}"
                );
                break;
            }
            ServerMessage::FrameReady { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    finish_session(client, handle);
}
