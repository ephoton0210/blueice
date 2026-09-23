// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-ai-gatekeeper`: the deterministic rule-base component of
//! `phase-7-local-ai/PLAN.md`'s safety-gatekeeper process. It remains
//! deliberately independent of any future AI model: `rules` has no prompt or
//! model input and produces a stable decision from one request alone. The
//! process/IPC/concurrency/fail-closed mechanism remains owned by
//! `blueice-engine`'s `gatekeeper_client`/`session` modules.
//!
//! `core` opens a short-lived, per-check connection (connect -> request
//! -> reply -> disconnect) rather than multiplexing many checks over
//! one shared connection. The binary serves each accepted connection on a
//! separate bounded-time worker, so concurrent checks from different tabs
//! remain independent all the way through the gatekeeper.

mod rules;

pub use rules::{review, RULESET_VERSION};

use blueice_ipc::gatekeeper::{
    read_gatekeeper_request, read_gatekeeper_wire_request, write_gatekeeper_reply,
    write_gatekeeper_settings_reply, GatekeeperSettings, GatekeeperSettingsChange,
    GatekeeperSettingsReply, GatekeeperWireRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Reads one [`blueice_ipc::gatekeeper::GatekeeperRequest`] from
/// `stream` and replies with the independent deterministic rule-base
/// decision. A future model review is a separate second layer; it must not
/// replace or be able to modify this one.
pub fn handle_one_check<S: Read + Write>(stream: &mut S) -> io::Result<()> {
    let request = read_gatekeeper_request(stream)?;
    write_gatekeeper_reply(stream, &review(&request))
}

/// Persistent, user-adjustable state for the deterministic rule base. The
/// only adjustable field is an additive local blocklist; compiled baseline
/// rules and every workflow stage stay mandatory for the release.
pub struct GatekeeperService {
    settings_path: Option<PathBuf>,
    custom_blocked_hosts: RwLock<BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredSettings {
    schema_version: u32,
    custom_blocked_hosts: Vec<String>,
}

const SETTINGS_SCHEMA_VERSION: u32 = 1;

impl GatekeeperService {
    /// Constructs a service using `settings_path` for its user-managed,
    /// additive blocklist. Passing `None` is useful for tests and keeps all
    /// settings in memory.
    pub fn new(settings_path: Option<PathBuf>) -> Result<Self, String> {
        let custom_blocked_hosts = match settings_path.as_deref() {
            Some(path) if path.exists() => load_custom_blocked_hosts(path)?,
            _ => BTreeSet::new(),
        };
        Ok(Self {
            settings_path,
            custom_blocked_hosts: RwLock::new(custom_blocked_hosts),
        })
    }

    /// Runs one review or settings-control exchange. The two protocol forms
    /// remain distinct at the wire level so a settings read can never clear a
    /// URL/content/download/extension action by accident.
    pub fn handle_connection<S: Read + Write>(&self, stream: &mut S) -> io::Result<()> {
        match read_gatekeeper_wire_request(stream)? {
            GatekeeperWireRequest::Review(request) => {
                let hosts = self
                    .custom_blocked_hosts
                    .read()
                    .expect("gatekeeper settings lock must not be poisoned")
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>();
                write_gatekeeper_reply(
                    stream,
                    &rules::review_with_custom_blocked_hosts(&request, &hosts),
                )
            }
            GatekeeperWireRequest::Settings(request) => {
                let reply = match request {
                    blueice_ipc::gatekeeper::GatekeeperSettingsRequest::Read => {
                        GatekeeperSettingsReply::Settings(self.settings())
                    }
                    blueice_ipc::gatekeeper::GatekeeperSettingsRequest::Update { change } => {
                        match self.apply_change(change) {
                            Ok(settings) => GatekeeperSettingsReply::Settings(settings),
                            Err(reason) => GatekeeperSettingsReply::Rejected { reason },
                        }
                    }
                };
                write_gatekeeper_settings_reply(stream, &reply)
            }
        }
    }

    pub fn settings(&self) -> GatekeeperSettings {
        let hosts = self
            .custom_blocked_hosts
            .read()
            .expect("gatekeeper settings lock must not be poisoned")
            .iter()
            .cloned()
            .collect();
        rules::settings(hosts)
    }

    fn apply_change(&self, change: GatekeeperSettingsChange) -> Result<GatekeeperSettings, String> {
        let mut hosts = self
            .custom_blocked_hosts
            .write()
            .map_err(|_| "gatekeeper settings are unavailable".to_string())?;
        let host = match &change {
            GatekeeperSettingsChange::AddBlockedHost { host }
            | GatekeeperSettingsChange::RemoveBlockedHost { host } => normalize_host(host)?,
        };
        let mut next = hosts.clone();
        let changed = match change {
            GatekeeperSettingsChange::AddBlockedHost { .. } => next.insert(host),
            GatekeeperSettingsChange::RemoveBlockedHost { .. } => next.remove(&host),
        };
        if changed {
            if let Some(path) = self.settings_path.as_deref() {
                persist_custom_blocked_hosts(path, &next)?;
            }
        }
        *hosts = next;
        Ok(rules::settings(hosts.iter().cloned().collect()))
    }
}

/// Default persistent location for user-managed gatekeeper additions. It is
/// intentionally separate from the private socket directory: socket files are
/// ephemeral transport state, while a user's policy must survive a restart.
pub fn default_settings_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("blueice").join("gatekeeper-settings.json")
}

fn load_custom_blocked_hosts(path: &Path) -> Result<BTreeSet<String>, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("reading gatekeeper settings {}: {error}", path.display()))?;
    let stored: StoredSettings = serde_json::from_str(&raw)
        .map_err(|error| format!("parsing gatekeeper settings {}: {error}", path.display()))?;
    if stored.schema_version != SETTINGS_SCHEMA_VERSION {
        return Err(format!(
            "gatekeeper settings {} use unsupported schema version {}",
            path.display(),
            stored.schema_version
        ));
    }
    stored
        .custom_blocked_hosts
        .into_iter()
        .map(|host| normalize_host(&host))
        .collect()
}

fn persist_custom_blocked_hosts(path: &Path, hosts: &BTreeSet<String>) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("gatekeeper settings path {} has no parent", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "creating gatekeeper settings directory {}: {error}",
            parent.display()
        )
    })?;
    let stored = StoredSettings {
        schema_version: SETTINGS_SCHEMA_VERSION,
        custom_blocked_hosts: hosts.iter().cloned().collect(),
    };
    let json = serde_json::to_vec_pretty(&stored)
        .map_err(|error| format!("encoding gatekeeper settings: {error}"))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, json).map_err(|error| {
        format!(
            "writing gatekeeper settings {}: {error}",
            temporary.display()
        )
    })?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("replacing gatekeeper settings {}: {error}", path.display())
    })
}

fn normalize_host(raw: &str) -> Result<String, String> {
    let host = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 {
        return Err("a blocked host must be 1 to 253 characters".to_string());
    }
    if host.split('.').any(|label| {
        label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return Err(
            "a blocked host must be a plain DNS hostname without a scheme, path, or wildcard"
                .to_string(),
        );
    }
    Ok(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::gatekeeper::{
        read_gatekeeper_reply, read_gatekeeper_settings_reply, write_gatekeeper_request,
        write_gatekeeper_settings_request, GatekeeperReply, GatekeeperRequest,
        GatekeeperSettingsChange, GatekeeperSettingsReply, GatekeeperSettingsRequest,
    };
    use std::os::unix::net::UnixStream;
    use std::thread;

    #[test]
    fn clears_a_safe_check_url_request() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_one_check(&mut server));

        write_gatekeeper_request(
            &mut client,
            &GatekeeperRequest::CheckUrl {
                url: "https://example.com".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_gatekeeper_reply(&mut client).unwrap(),
            GatekeeperReply::Cleared
        );

        handle.join().unwrap().unwrap();
    }

    #[test]
    fn clears_a_safe_check_content_request() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_one_check(&mut server));

        write_gatekeeper_request(
            &mut client,
            &GatekeeperRequest::CheckContent {
                url: "https://example.com".to_string(),
                html: "<p>hi</p>".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            read_gatekeeper_reply(&mut client).unwrap(),
            GatekeeperReply::Cleared
        );

        handle.join().unwrap().unwrap();
    }

    #[test]
    fn rejects_an_executable_check_download_request() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_one_check(&mut server));

        write_gatekeeper_request(
            &mut client,
            &GatekeeperRequest::CheckDownload {
                url: "https://example.com/setup.exe".to_string(),
                file_name: "setup.exe".to_string(),
                content_type: Some("application/x-msdownload".to_string()),
                total_bytes: Some(4096),
            },
        )
        .unwrap();
        assert!(matches!(
            read_gatekeeper_reply(&mut client).unwrap(),
            GatekeeperReply::Rejected { category, .. } if category == "dangerous-file-type"
        ));

        handle.join().unwrap().unwrap();
    }

    #[test]
    fn a_malformed_request_is_an_io_error_not_a_panic() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || handle_one_check(&mut server));

        // A truncated frame: a length prefix promising more bytes than
        // are ever sent.
        client.write_all(&100u32.to_le_bytes()).unwrap();
        client.write_all(b"short").unwrap();
        drop(client);

        assert!(handle.join().unwrap().is_err());
    }

    #[test]
    fn handles_two_connections_in_sequence() {
        // Proves the accept-loop-friendly shape (`handle_one_check`
        // handles exactly one connection's one request/reply and
        // returns, never blocking on a second) that `src/bin/blueice-
        // ai-gatekeeper.rs`'s sequential accept loop depends on.
        for _ in 0..2 {
            let (mut client, mut server) = UnixStream::pair().unwrap();
            let handle = thread::spawn(move || handle_one_check(&mut server));
            write_gatekeeper_request(
                &mut client,
                &GatekeeperRequest::CheckUrl {
                    url: "https://example.com".to_string(),
                },
            )
            .unwrap();
            assert_eq!(
                read_gatekeeper_reply(&mut client).unwrap(),
                GatekeeperReply::Cleared
            );
            handle.join().unwrap().unwrap();
        }
    }

    fn settings_exchange(
        service: &GatekeeperService,
        request: GatekeeperSettingsRequest,
    ) -> GatekeeperSettingsReply {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::scope(|scope| {
            let worker = scope.spawn(|| service.handle_connection(&mut server));
            write_gatekeeper_settings_request(&mut client, &request).unwrap();
            let reply = read_gatekeeper_settings_reply(&mut client).unwrap();
            worker.join().unwrap().unwrap();
            reply
        });
        worker
    }

    #[test]
    fn settings_show_the_locked_baseline_and_allow_only_an_additive_host_rule() {
        let service = GatekeeperService::new(None).unwrap();
        let GatekeeperSettingsReply::Settings(initial) =
            settings_exchange(&service, GatekeeperSettingsRequest::Read)
        else {
            panic!("settings read must succeed")
        };
        assert_eq!(initial.ruleset_version, RULESET_VERSION);
        assert!(initial.baseline_rules.iter().all(|rule| rule.mandatory));
        assert!(initial.workflow.iter().all(|step| step.mandatory));
        assert!(initial.custom_blocked_hosts.is_empty());

        let GatekeeperSettingsReply::Settings(updated) = settings_exchange(
            &service,
            GatekeeperSettingsRequest::Update {
                change: GatekeeperSettingsChange::AddBlockedHost {
                    host: "Tracker.Example.".to_string(),
                },
            },
        ) else {
            panic!("valid addition must succeed")
        };
        assert_eq!(updated.custom_blocked_hosts, vec!["tracker.example"]);

        let (mut client, mut server) = UnixStream::pair().unwrap();
        thread::scope(|scope| {
            let worker = scope.spawn(|| service.handle_connection(&mut server));
            write_gatekeeper_request(
                &mut client,
                &GatekeeperRequest::CheckUrl {
                    url: "https://sub.tracker.example/path".to_string(),
                },
            )
            .unwrap();
            assert!(matches!(
                read_gatekeeper_reply(&mut client).unwrap(),
                GatekeeperReply::Rejected { category, .. } if category == "custom-blocked-domain"
            ));
            worker.join().unwrap().unwrap();
        });
    }

    #[test]
    fn invalid_adjustments_are_rejected_and_persisted_hosts_survive_restart() {
        let path = std::env::temp_dir().join(format!(
            "blueice-gatekeeper-settings-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_file(&path);
        let service = GatekeeperService::new(Some(path.clone())).unwrap();
        assert!(matches!(
            settings_exchange(
                &service,
                GatekeeperSettingsRequest::Update {
                    change: GatekeeperSettingsChange::AddBlockedHost {
                        host: "https://not-a-host.example/path".to_string(),
                    },
                },
            ),
            GatekeeperSettingsReply::Rejected { .. }
        ));
        let _ = settings_exchange(
            &service,
            GatekeeperSettingsRequest::Update {
                change: GatekeeperSettingsChange::AddBlockedHost {
                    host: "blocked.example".to_string(),
                },
            },
        );
        drop(service);
        let restored = GatekeeperService::new(Some(path.clone())).unwrap();
        assert_eq!(
            restored.settings().custom_blocked_hosts,
            vec!["blocked.example"]
        );
        let _ = fs::remove_file(path);
    }
}
