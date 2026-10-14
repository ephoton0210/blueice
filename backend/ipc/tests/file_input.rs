// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ipc::file_input::{FileData, FileInputAction, FileInputContext, MAX_SELECTED_BYTES};
use blueice_ipc::{ClientMessage, ServerMessage};

#[test]
fn native_activation_round_trips_gesture_owner_and_optional_hint_without_paths() {
    let context = blueice_ipc::input::TextInputContext {
        version: 1,
        frame_source: 7,
        document_generation: 8,
        focus_generation: 9,
    };
    let command = ClientMessage::NativeActivate {
        gesture: 42,
        context,
        x: 12.0,
        y: 34.0,
    };
    let mut bytes = vec![];
    blueice_ipc::write_client_message_with_ids(&mut bytes, Some(3), Some(21), &command).unwrap();
    let (tab, request, decoded) =
        blueice_ipc::read_client_message_with_ids(&mut bytes.as_slice()).unwrap();
    assert_eq!((tab, request, decoded), (Some(3), Some(21), command));
    for file_input in [
        None,
        Some(blueice_ipc::file_input::FileInputState {
            context: FileInputContext {
                tab_id: 3,
                frame_source: 7,
                document_generation: 8,
                node_id: 11,
                revision: 2,
            },
            multiple: false,
            accept: ".txt".into(),
            names: vec!["selected.txt".into()],
        }),
    ] {
        let reply = ServerMessage::NativeActivationCompleted {
            gesture: 42,
            file_input,
        };
        let mut bytes = vec![];
        blueice_ipc::write_server_message_with_ids(&mut bytes, Some(3), Some(21), &reply).unwrap();
        let (tab, request, decoded) =
            blueice_ipc::read_server_message_with_ids(&mut bytes.as_slice()).unwrap();
        assert_eq!((tab, request, decoded), (Some(3), Some(21), reply));
    }
}

#[test]
fn file_content_has_no_path_shape_and_cannot_inject_multipart_headers() {
    let mut file = FileData {
        name: "中文\".txt".into(),
        media_type: "text/plain".into(),
        last_modified: 123,
        bytes: vec![0, 255, 13, 10],
    };
    assert!(file.valid());
    for bad in [
        "text/plain\r\nInjected:yes",
        "text/plain; charset=utf-8",
        "/plain",
        "text/",
        "text/plain/extra",
        "",
    ] {
        file.media_type = bad.into();
        assert!(!file.valid());
    }
    file.media_type = "text/plain".into();
    file.bytes = vec![0; MAX_SELECTED_BYTES + 1];
    assert!(!file.valid());
    assert!(serde_json::from_str::<FileData>(
        r#"{"name":"file.txt","media_type":"text/plain","bytes":[],"path":"/private/secret"}"#
    )
    .is_err());
    assert!(serde_json::from_str::<FileInputAction>(r#"{"Set":{"context":{"tab_id":1,"frame_source":4,"document_generation":1,"node_id":3,"revision":2,"path":"/private/secret"},"files":[]}}"#).is_err());
}

#[test]
fn bounded_file_messages_round_trip_as_contents_without_exposing_bytes_in_debug() {
    let request = ClientMessage::FileInput(FileInputAction::Set {
        context: FileInputContext {
            tab_id: 7,
            frame_source: 9,
            document_generation: 2,
            node_id: 12,
            revision: 3,
        },
        files: vec![FileData {
            name: "private-file.txt".into(),
            media_type: "text/plain".into(),
            last_modified: 123,
            bytes: b"private-file-body".to_vec(),
        }],
    });
    let mut bytes = vec![];
    blueice_ipc::write_client_message_with_ids(&mut bytes, Some(7), Some(99), &request).unwrap();
    let (tab, id, decoded) =
        blueice_ipc::read_client_message_with_ids(&mut bytes.as_slice()).unwrap();
    assert_eq!((tab, id), (Some(7), Some(99)));
    assert_eq!(decoded, request);
    assert!(!format!("{decoded:?}").contains("private-file"));
    assert!(serde_json::from_str::<ClientMessage>(
        r#"{"OpenNativeFilePicker":{"path":"/private/secret"}}"#
    )
    .is_err());
    assert!(serde_json::from_str::<ServerMessage>(
        r#"{"OpenNativeFilePicker":{"path":"/private/secret"}}"#
    )
    .is_err());
}
