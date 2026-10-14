// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(not(unix))]

use blueice_engine::assistant_client::{
    organize_text, summarize_text, translate_html, AssistantConfig,
};
use blueice_engine::downloads_page::DownloadsSource;
use blueice_engine::gatekeeper_settings_page::GatekeeperSettingsSource;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

#[test]
fn unsupported_services_return_without_spawning_or_translating_page_content() {
    let socket = std::env::temp_dir().join("blueice-platform-test.sock");
    let spawned = Arc::new(AtomicBool::new(false));
    let observed = spawned.clone();
    let downloads = DownloadsSource::with_spawner(
        socket.clone(),
        Box::new(move || {
            observed.store(true, Ordering::SeqCst);
            Ok(())
        }),
        Duration::from_secs(1),
    );
    assert!(downloads.fetch_spawning().is_err());
    assert!(downloads.fetch_quick().is_err());
    assert!(!spawned.load(Ordering::SeqCst));
    assert!(GatekeeperSettingsSource::at(socket.clone())
        .fetch()
        .is_err());
    let config = AssistantConfig {
        socket: socket.clone(),
        target_language: "zh-TW".into(),
        deadline: Duration::from_secs(1),
    };
    assert_eq!(translate_html(&config, "<p>Original page</p>"), None);
    assert!(summarize_text(&socket, config.deadline, "Original page").is_err());
    assert!(organize_text(&socket, config.deadline, "Original page", "Make a table").is_err());
}
