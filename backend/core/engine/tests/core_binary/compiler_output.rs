// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::compiler::{self, CompilerReply, CompilerRequest};
use blueice_ipc::compiler_catalog::{
    write_compiler_catalog, CompilerCatalogBootstrap, CompilerCatalogModule,
    CompilerCatalogOptions, CompilerCatalogProject, COMPILER_CATALOG_BOOTSTRAP_VERSION,
};
use blueice_ipc::compiler_output::{
    self, CompilerOutputErrorCode, CompilerOutputReply, CompilerOutputRequest,
    CompilerOutputSessionReceipt, COMPILER_OUTPUT_PROTOCOL_VERSION,
};

#[test]
fn real_core_output_socket_requires_a_separate_owner_grant_and_receipt() {
    let directory = std::env::temp_dir().join(format!(
        "blueice-core-output-process-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let project_root = directory.join("project");
    let output_root = directory.join("output");
    std::fs::create_dir(&project_root).unwrap();
    std::fs::create_dir(&output_root).unwrap();
    let config = project_root.join("blue-ts.json");
    let entry = project_root.join("main.ts");
    std::fs::write(&config, "{}").unwrap();
    std::fs::write(&entry, "export const answer: number = 42;").unwrap();
    let catalog = CompilerCatalogBootstrap {
        version: COMPILER_CATALOG_BOOTSTRAP_VERSION,
        projects: vec![CompilerCatalogProject {
            canonical_project_root: project_root.to_str().unwrap().into(),
            canonical_config_root: config.to_str().unwrap().into(),
            canonical_output_root: output_root.to_str().unwrap().into(),
            entry_module: entry.to_str().unwrap().into(),
            modules: vec![CompilerCatalogModule {
                canonical_id: entry.to_str().unwrap().into(),
                text: "export const answer: number = 42;".into(),
            }],
            expose_to_compiler_ipc: true,
            grant_output_write: true,
            resolutions: Vec::new(),
            options: CompilerCatalogOptions::default(),
        }],
    };
    let socket = unique_socket_path("out-core");
    let query = unique_socket_path("out-query");
    let output = unique_socket_path("out-write");
    let frames = directory.join("frames");
    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket.to_str().unwrap(),
            "--compiler-socket",
            query.to_str().unwrap(),
            "--compiler-output-socket",
            output.to_str().unwrap(),
            "--compiler-catalog-stdin",
            "--frame-dir",
            frames.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write_compiler_catalog(&mut child.stdin.take().unwrap(), &catalog).unwrap();
    assert!(wait_for(&socket, Duration::from_secs(5)));
    assert!(wait_for(&query, Duration::from_secs(5)));
    assert!(wait_for(&output, Duration::from_secs(5)));
    assert_eq!(
        std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let mut frontend = connect_with_retry(&socket, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();

    let mut query_stream = UnixStream::connect(&query).unwrap();
    compiler::write_compiler_request(
        &mut query_stream,
        &CompilerRequest::Hello {
            protocol_version: compiler::COMPILER_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let CompilerReply::HelloAck {
        session_attestation,
        ..
    } = compiler::read_compiler_reply(&mut query_stream).unwrap()
    else {
        panic!("query socket must give only a query attestation")
    };

    let mut output_stream = UnixStream::connect(&output).unwrap();
    compiler_output::write_compiler_output_request(
        &mut output_stream,
        &CompilerOutputRequest::Hello {
            protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let CompilerOutputReply::HelloAck { receipt, .. } =
        compiler_output::read_compiler_output_reply(&mut output_stream).unwrap()
    else {
        panic!("output socket must mint its independent write receipt")
    };
    assert!(receipt.is_well_formed());
    assert_ne!(receipt.id, session_attestation.id);
    compiler_output::write_compiler_output_request(
        &mut output_stream,
        &CompilerOutputRequest::ListProjects {
            receipt: CompilerOutputSessionReceipt {
                id: session_attestation.id.clone(),
            },
        },
    )
    .unwrap();
    assert!(matches!(
        compiler_output::read_compiler_output_reply(&mut output_stream).unwrap(),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::InvalidReceipt,
            ..
        }
    ));
    compiler_output::write_compiler_output_request(
        &mut output_stream,
        &CompilerOutputRequest::ListProjects {
            receipt: receipt.clone(),
        },
    )
    .unwrap();
    let CompilerOutputReply::Projects(inventory) =
        compiler_output::read_compiler_output_reply(&mut output_stream).unwrap()
    else {
        panic!("the owner grant must enter this output stream's inventory")
    };
    assert_eq!(inventory.projects.len(), 1);
    compiler_output::write_compiler_output_request(
        &mut output_stream,
        &CompilerOutputRequest::Build {
            receipt: CompilerOutputSessionReceipt {
                id: session_attestation.id,
            },
            project: inventory.projects[0],
        },
    )
    .unwrap();
    assert!(matches!(
        compiler_output::read_compiler_output_reply(&mut output_stream).unwrap(),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::InvalidReceipt,
            ..
        }
    ));
    assert_eq!(std::fs::read_dir(&output_root).unwrap().count(), 0);
    compiler_output::write_compiler_output_request(
        &mut output_stream,
        &CompilerOutputRequest::Build {
            receipt,
            project: inventory.projects[0],
        },
    )
    .unwrap();
    let CompilerOutputReply::Build(result) =
        compiler_output::read_compiler_output_reply(&mut output_stream).unwrap()
    else {
        panic!("granted real core build must publish")
    };
    assert!(result.published);
    assert!(!result.has_errors);
    assert_eq!(std::fs::read_dir(&output_root).unwrap().count(), 1);
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!output.exists());

    let mut ungranted = catalog;
    ungranted.projects[0].grant_output_write = false;
    let mut denied = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket.to_str().unwrap(),
            "--compiler-socket",
            query.to_str().unwrap(),
            "--compiler-output-socket",
            output.to_str().unwrap(),
            "--compiler-catalog-stdin",
            "--frame-dir",
            frames.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write_compiler_catalog(&mut denied.stdin.take().unwrap(), &ungranted).unwrap();
    let denied_status = denied.wait().unwrap();
    assert!(!denied_status.success());
    assert!(
        !output.exists(),
        "no write grant must mean no output listener"
    );
    std::fs::remove_dir_all(directory).unwrap();
}
