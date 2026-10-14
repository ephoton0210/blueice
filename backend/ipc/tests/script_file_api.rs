// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_ipc::{page_host::*, script::*};
use std::os::unix::net::UnixStream;

#[test]
fn file_operations_round_trip_with_their_exact_document_target() {
    let target = ScriptDocumentTarget {
        tab_id: 9,
        document_generation: 23,
    };
    for request in [
        ScriptRequest::GetDocument { target },
        ScriptRequest::GetInputFiles { target, node: 17 },
        ScriptRequest::ReadInputFile {
            target,
            node: 17,
            revision: 8,
            index: 0,
            offset: 65536,
            length: 3,
        },
        ScriptRequest::GetFileValue { target, node: 17 },
        ScriptRequest::ClearInputFiles { target, node: 17 },
        ScriptRequest::ResetForm { target, node: 19 },
    ] {
        assert_eq!(request.document_target(), Some(target));
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_script_request(&mut writer, &request).unwrap();
        assert_eq!(read_script_request(&mut reader).unwrap(), request);
    }
}

#[test]
fn selected_metadata_and_binary_chunks_round_trip_without_native_paths() {
    for reply in [
        ScriptReply::InputFiles {
            revision: 8,
            files: Some(vec![ScriptFileMetadata {
                name: "chosen-中文.bin".into(),
                media_type: "application/octet-stream".into(),
                last_modified: -123,
                size: SCRIPT_MAX_FILE_CHUNK_BYTES + 3,
            }]),
        },
        ScriptReply::InputFiles {
            revision: 9,
            files: Some(vec![]),
        },
        ScriptReply::InputFiles {
            revision: 9,
            files: None,
        },
        ScriptReply::InputFileBytes {
            bytes: (0..SCRIPT_MAX_FILE_CHUNK_BYTES)
                .map(|i| (i % 256) as u8)
                .collect(),
        },
        ScriptReply::FileValue {
            value: Some("C:\\fakepath\\chosen-中文.bin".into()),
        },
        ScriptReply::FileValue { value: None },
    ] {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        // Large chunks can exceed a socket's write buffer, so read concurrently.
        std::thread::scope(|scope| {
            scope.spawn(|| write_script_reply(&mut writer, &reply).unwrap());
            assert_eq!(read_script_reply(&mut reader).unwrap(), reply);
        });
    }
}

#[test]
fn native_file_events_round_trip_with_correlated_selection_replies() {
    for event in [
        PageHostFileSelectionEvent::Input,
        PageHostFileSelectionEvent::Change,
        PageHostFileSelectionEvent::Cancel,
    ] {
        let request = PageHostRequest::DispatchFileSelection {
            tab_id: 9,
            document_generation: 23,
            nodes: vec![17, 19, 3],
            event,
        };
        let reply = PageHostReply::FileSelectionDispatched {
            tab_id: 9,
            document_generation: 23,
            event,
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_request(&mut writer, &request).unwrap();
        assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    }
}
