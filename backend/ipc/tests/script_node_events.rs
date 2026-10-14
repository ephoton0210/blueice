// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]
use blueice_ipc::script::*;
use std::os::unix::net::UnixStream;

#[test]
fn event_path_operations_round_trip_with_exact_call_and_document_identity() {
    let target = ScriptDocumentTarget {
        tab_id: 9,
        document_generation: 23,
    };
    let inner = ScriptRequest::GetEventPath { target, node: 17 };
    assert_eq!(inner.document_target(), Some(target));
    let request = ScriptRequest::Call {
        request_id: 31,
        request: Box::new(inner),
    };
    let reply = ScriptReply::CallResult {
        request_id: 31,
        target,
        reply: Box::new(ScriptReply::EventPath {
            nodes: vec![17, 19, 3],
        }),
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_script_request(&mut writer, &request).unwrap();
    assert_eq!(read_script_request(&mut reader).unwrap(), request);
    write_script_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_script_reply(&mut reader).unwrap(), reply);
}
