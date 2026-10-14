// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::extension::{read_extension_reply, write_extension_request, DomWriteTarget};
use blueice_ipc::gatekeeper::{read_gatekeeper_request, write_gatekeeper_reply};
use std::collections::BTreeMap;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;

fn unique_gatekeeper_socket(_label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // The temp-dir prefix can already be long on Darwin, whose
    // Unix-domain socket path limit is small. The counter and pid
    // make this concise leaf sufficient for independent tests.
    blueice_ipc::local_socket::default_socket_dir().join(format!("extg-{}-{n}", std::process::id()))
}

fn start_gatekeeper(
    label: &str,
    reply: GatekeeperReply,
) -> (PathBuf, thread::JoinHandle<GatekeeperRequest>) {
    let socket = unique_gatekeeper_socket(label);
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_gatekeeper_request(&mut stream).unwrap();
        write_gatekeeper_reply(&mut stream, &reply).unwrap();
        request
    });
    (socket, handle)
}

fn start_gatekeeper_replies(
    label: &str,
    replies: Vec<GatekeeperReply>,
) -> (PathBuf, thread::JoinHandle<Vec<GatekeeperRequest>>) {
    let socket = unique_gatekeeper_socket(label);
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let handle = thread::spawn(move || {
        replies
            .into_iter()
            .map(|reply| {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_gatekeeper_request(&mut stream).unwrap();
                write_gatekeeper_reply(&mut stream, &reply).unwrap();
                request
            })
            .collect()
    });
    (socket, handle)
}

fn registry_with_dom_write_granted() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_WRITE);
    registry
}

fn registry_with_network_intercept_granted() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_NETWORK_INTERCEPT);
    registry
}

fn registry_with_storage_granted() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::minimal_slice();
    registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_STORAGE);
    registry
}

fn hello(extension_id: &str) -> ExtensionRequest {
    hello_with_capabilities(
        extension_id,
        [(CAPABILITY_DOM_READ, 1), (CAPABILITY_DOM_WRITE, 1)],
    )
}

fn hello_with_capabilities(
    extension_id: &str,
    capabilities: impl IntoIterator<Item = (&'static str, u32)>,
) -> ExtensionRequest {
    ExtensionRequest::Hello {
        extension_id: extension_id.to_string(),
        capability_versions: capabilities
            .into_iter()
            .map(|(capability, version)| (capability.to_string(), version))
            .collect(),
    }
}

fn empty_hello_ack() -> ExtensionReply {
    ExtensionReply::HelloAck {
        unsupported_capabilities: BTreeMap::new(),
    }
}

fn unused_write_delegate(
    _: Option<(u64, u64)>,
    _: String,
    _: &DomWriteTarget,
    _: u64,
) -> Result<(), String> {
    Ok(())
}

mod permissions;

mod permissions_2;

mod authentication;

mod authentication_2;

mod ui;

mod storage;

mod dom_write;

mod lifecycle;

mod network;
