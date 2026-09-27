// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use std::io::Cursor;

fn output_receipt() -> CompilerOutputSessionReceipt {
    CompilerOutputSessionReceipt {
        id: format!("ow-{}", "a1".repeat(32)),
    }
}

fn decode_request(value: serde_json::Value) -> io::Result<CompilerOutputRequest> {
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut frame = Vec::new();
    frame.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_le_bytes());
    frame.extend_from_slice(&bytes);
    read_compiler_output_request(&mut Cursor::new(frame))
}

#[test]
fn output_hello_requires_its_own_exact_receipt_and_version() {
    let hello = CompilerOutputRequest::Hello {
        protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
    };
    assert_eq!(
        negotiate(&hello, Some(output_receipt())),
        CompilerOutputReply::HelloAck {
            protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
            receipt: output_receipt(),
        }
    );
    assert!(!CompilerOutputSessionReceipt {
        id: "a1".repeat(32),
    }
    .is_well_formed());
    assert!(matches!(
        negotiate(
            &hello,
            Some(CompilerOutputSessionReceipt {
                id: "a1".repeat(32),
            })
        ),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::InvalidReceipt,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &CompilerOutputRequest::Hello {
                protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION + 1,
            },
            Some(output_receipt())
        ),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::ProtocolVersion,
            ..
        }
    ));
}

#[test]
fn output_requests_reject_client_sources_options_and_paths() {
    for injected in [
        serde_json::json!({ "path": "/tmp/escape.js" }),
        serde_json::json!({ "source": "export const changed = 1" }),
        serde_json::json!({ "options": { "target": "es2020" } }),
        serde_json::json!({ "resolver_edges": [] }),
    ] {
        let mut build = serde_json::json!({
            "Build": {
                "receipt": { "id": output_receipt().id },
                "project": { "id": 7 }
            }
        });
        let (key, value) = injected.as_object().unwrap().iter().next().unwrap();
        build["Build"][key] = value.clone();
        assert!(decode_request(build).is_err());
    }
    assert!(decode_request(serde_json::json!({
        "Build": {
            "receipt": { "id": output_receipt().id, "query_receipt": "forged" },
            "project": { "id": 7 }
        }
    }))
    .is_err());
    assert!(decode_request(serde_json::json!({
        "Build": {
            "receipt": { "id": output_receipt().id },
            "project": { "id": 7, "output_root": "/tmp/elsewhere" }
        }
    }))
    .is_err());
}

#[test]
fn output_build_result_is_bounded_and_writes_no_artifact_or_path() {
    let project = CompilerProject { id: 7 };
    let result = CompilerOutputBuildResult {
        generation: CompilerGeneration {
            project,
            sequence: 3,
        },
        project_fingerprint: "bts-graph-v1".into(),
        has_errors: false,
        published: true,
    };
    assert!(result.is_well_formed_for_project(project));
    assert!(!result.is_well_formed_for_project(CompilerProject { id: 8 }));
    let reply = CompilerOutputReply::Build(result);
    let mut bytes = Vec::new();
    write_compiler_output_reply(&mut bytes, &reply).unwrap();
    assert_eq!(
        read_compiler_output_reply(&mut Cursor::new(&bytes)).unwrap(),
        reply
    );
    let payload = String::from_utf8(bytes[4..].to_vec()).unwrap();
    assert!(!payload.contains("output_root"));
    assert!(!payload.contains("javascript"));
    assert!(!payload.contains("source"));

    let mut oversized = Cursor::new(
        u32::try_from(MAX_COMPILER_OUTPUT_MESSAGE_BYTES + 1)
            .unwrap()
            .to_le_bytes(),
    );
    assert!(read_compiler_output_request(&mut oversized).is_err());
}
