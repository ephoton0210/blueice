// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Real owner bootstrap, compiler IPC, complete roots and stale metadata handles.

use blueice_ipc::compiler::{
    self, CompilerErrorCode, CompilerReply, CompilerRequest, CompilerStaticMetadataKind,
    CompilerWorkSetKind,
};
use blueice_ipc::compiler_catalog::{
    write_compiler_catalog, CompilerCatalogBootstrap, CompilerCatalogModule,
    CompilerCatalogOptions, CompilerCatalogProject, COMPILER_CATALOG_BOOTSTRAP_VERSION,
};
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Core(Child);
impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait(core: &mut Core, path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        if let Some(status) = core.0.try_wait().unwrap() {
            let mut error = String::new();
            core.0
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut error)
                .unwrap();
            panic!("owner core exited {status}: {error}");
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "core did not bind {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn query(stream: &mut UnixStream, request: CompilerRequest) -> CompilerReply {
    compiler::write_compiler_request(stream, &request).unwrap();
    compiler::read_compiler_reply(stream).unwrap()
}

#[test]
fn an_owner_bootstraps_two_roots_without_promoting_unselected_sources_or_write_grants() {
    let directory = PathBuf::from("/tmp").join(format!(
        "bpr-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let socket = directory.join("core.sock");
    let compiler_socket = directory.join("query.sock");
    let entry = "project:///owner/main.ts";
    let consumer = "project:///owner/consumer.ts";
    let catalog = CompilerCatalogBootstrap {
        version: COMPILER_CATALOG_BOOTSTRAP_VERSION,
        projects: vec![CompilerCatalogProject {
            canonical_project_root: "project:///owner".into(),
            canonical_config_root: "project:///owner/tsconfig.json".into(),
            canonical_output_root: "project:///owner-output".into(),
            entry_module: entry.into(),
            program_roots: Some(vec![entry.into(), consumer.into()]),
            modules: vec![
                CompilerCatalogModule {
                    canonical_id: entry.into(),
                    text: "const value: number = 1;".into(),
                },
                CompilerCatalogModule {
                    canonical_id: consumer.into(),
                    text: "const result: number = value;".into(),
                },
                CompilerCatalogModule {
                    canonical_id: "project:///owner/unselected.ts".into(),
                    text: "const value: string = 'poison';".into(),
                },
            ],
            expose_to_compiler_ipc: true,
            grant_output_write: false,
            resolutions: Vec::new(),
            options: CompilerCatalogOptions {
                declaration: true,
                ..CompilerCatalogOptions::default()
            },
        }],
    };
    let mut core = Core(
        Command::new(env!("CARGO_BIN_EXE_blueice-core"))
            .arg("--socket")
            .arg(&socket)
            .arg("--compiler-socket")
            .arg(&compiler_socket)
            .arg("--compiler-catalog-stdin")
            .arg("--frame-dir")
            .arg(directory.join("frames"))
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    write_compiler_catalog(&mut core.0.stdin.take().unwrap(), &catalog).unwrap();
    wait(&mut core, &socket);
    wait(&mut core, &compiler_socket);
    let mut frontend = UnixStream::connect(&socket).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    let mut stream = UnixStream::connect(&compiler_socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    assert!(matches!(
        query(
            &mut stream,
            CompilerRequest::Hello {
                protocol_version: compiler::COMPILER_PROTOCOL_VERSION
            }
        ),
        CompilerReply::HelloAck { .. }
    ));
    let CompilerReply::Projects(inventory) = query(&mut stream, CompilerRequest::ListProjects)
    else {
        panic!("owner program must enter this stream's inventory");
    };
    assert_eq!(inventory.projects.len(), 1);
    let project = inventory.projects[0];
    let CompilerReply::Check(first) = query(&mut stream, CompilerRequest::Check { project }) else {
        panic!("registered roots must produce a check generation");
    };
    assert!(!first.has_errors, "{:?}", first.diagnostics);
    assert_eq!(first.static_metadata.as_ref().unwrap().source_count, 2);
    assert_eq!(
        first.artifact_fingerprint.as_deref(),
        Some(first.project_fingerprint.as_str())
    );
    let CompilerReply::WorkSetPage(work) = query(
        &mut stream,
        CompilerRequest::ListWorkSet {
            generation: first.generation,
            kind: CompilerWorkSetKind::Parsed,
            cursor: None,
            limit: Some(10),
        },
    ) else {
        panic!("the exact root generation must expose its bounded work set");
    };
    assert_eq!(work.entries, vec![consumer.to_string(), entry.to_string()]);
    let CompilerReply::StaticMetadataPage(sources) = query(
        &mut stream,
        CompilerRequest::ListStaticMetadata {
            generation: first.generation,
            kind: CompilerStaticMetadataKind::Sources,
            cursor: None,
            limit: Some(10),
        },
    ) else {
        panic!("successful roots must have source provenance handles");
    };
    assert_eq!(sources.ids.len(), 2);
    let mut owners = Vec::new();
    for source_id in &sources.ids {
        let CompilerReply::StaticProvenance(source) = query(
            &mut stream,
            CompilerRequest::GetStaticProvenance {
                generation: first.generation,
                source_id: *source_id,
            },
        ) else {
            panic!("inventoried source must retain its owner");
        };
        owners.push(source.module);
    }
    owners.sort();
    assert_eq!(owners, vec![consumer, entry]);
    let CompilerReply::Check(warm) = query(&mut stream, CompilerRequest::Check { project }) else {
        panic!("same immutable program must check again");
    };
    assert!(warm.cache_hit);
    assert_eq!(warm.generation.sequence, first.generation.sequence + 1);
    assert!(matches!(
        query(
            &mut stream,
            CompilerRequest::GetStaticProvenance {
                generation: first.generation,
                source_id: sources.ids[0],
            }
        ),
        CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.0.wait().unwrap().success());
    drop(core);
    std::fs::remove_dir_all(directory).unwrap();
}
