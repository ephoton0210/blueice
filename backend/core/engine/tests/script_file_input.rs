// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_dom::NodeId;
use blueice_engine::{script::handle_script_request, TabManager};
use blueice_ipc::file_input::FileData;
use blueice_ipc::script::{
    ScriptDocumentTarget, ScriptReply, ScriptRequest, SCRIPT_MAX_FILE_CHUNK_BYTES,
};
use blueice_ipc::NodeAction;

#[test]
fn selected_content_is_chunked_and_stale_revisions_cannot_read_successor_bytes() {
    let mut tabs = TabManager::new(800.0, 600.0);
    let tab = tabs.default_tab();
    tabs.get_mut(tab).unwrap().load_html_str(
        "<input id=upload type=file aria-label=Upload><div id=other>Other</div>",
        Some("https://example.test/".into()),
    );
    let target = ScriptDocumentTarget {
        tab_id: tab.as_u64(),
        // The fresh page's first loaded document uses generation 1.
        document_generation: 1,
    };
    let lookup = |tabs: &mut TabManager, id: &str| {
        let ScriptReply::Node { node: Some(node) } = handle_script_request(
            tabs,
            ScriptRequest::GetElementById {
                target,
                id: id.into(),
            },
        ) else {
            panic!("DOM node must resolve");
        };
        node
    };
    let handle = lookup(&mut tabs, "upload");
    let other = lookup(&mut tabs, "other");
    assert!(matches!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::GetInputFiles {
                target,
                node: other
            }
        ),
        ScriptReply::InputFiles { files: None, .. }
    ));
    assert!(
        matches!(handle_script_request(&mut tabs, ScriptRequest::GetInputFiles {target,node:handle}), ScriptReply::InputFiles { files:Some(files), .. } if files.is_empty())
    );
    let page = tabs.get_mut(tab).unwrap();
    let raw = page
        .snapshot(0, 1)
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Upload"))
        .unwrap()
        .id;
    let context = page
        .file_input_state(raw, 42, target.document_generation)
        .unwrap()
        .context;
    let bytes: Vec<u8> = (0..SCRIPT_MAX_FILE_CHUNK_BYTES + 3)
        .map(|index| (index % 256) as u8)
        .collect();
    page.set_file_input(
        &context,
        42,
        vec![FileData {
            name: "chosen-中文.bin".into(),
            media_type: "application/octet-stream".into(),
            last_modified: 1234,
            bytes: bytes.clone(),
        }],
    )
    .unwrap();
    let ScriptReply::InputFiles {
        revision,
        files: Some(files),
    } = handle_script_request(
        &mut tabs,
        ScriptRequest::GetInputFiles {
            target,
            node: handle,
        },
    )
    else {
        panic!("Selected metadata must be available");
    };
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "chosen-中文.bin");
    assert_eq!(files[0].last_modified, 1234);
    assert_eq!(files[0].size, bytes.len());
    let read = |tabs: &mut TabManager, offset, length| {
        handle_script_request(
            tabs,
            ScriptRequest::ReadInputFile {
                target,
                node: handle,
                revision,
                index: 0,
                offset,
                length,
            },
        )
    };
    assert_eq!(
        read(&mut tabs, 0, SCRIPT_MAX_FILE_CHUNK_BYTES),
        ScriptReply::InputFileBytes {
            bytes: bytes[..SCRIPT_MAX_FILE_CHUNK_BYTES].to_vec()
        }
    );
    assert_eq!(
        read(&mut tabs, SCRIPT_MAX_FILE_CHUNK_BYTES, 3),
        ScriptReply::InputFileBytes {
            bytes: bytes[SCRIPT_MAX_FILE_CHUNK_BYTES..].to_vec()
        }
    );
    assert!(matches!(
        read(&mut tabs, 0, SCRIPT_MAX_FILE_CHUNK_BYTES + 1),
        ScriptReply::Error { .. }
    ));
    assert!(matches!(
        read(&mut tabs, bytes.len() + 1, 1),
        ScriptReply::Error { .. }
    ));
    tabs.get_mut(tab)
        .unwrap()
        .act(NodeId::from_u64(raw), NodeAction::SetValue(String::new()));
    assert!(matches!(read(&mut tabs, 0, 1), ScriptReply::Error { .. }));
    assert!(
        matches!(handle_script_request(&mut tabs,ScriptRequest::GetInputFiles {target,node:handle}),ScriptReply::InputFiles {files:Some(files),..} if files.is_empty())
    );
    tabs.get_mut(tab).unwrap().load_html_str(
        "<input id=upload type=file>",
        Some("https://example.test/next".into()),
    );
    assert!(matches!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::GetInputFiles {
                target,
                node: handle
            }
        ),
        ScriptReply::Error { .. }
    ));
}

#[test]
fn script_clear_and_form_reset_reject_stale_picker_contexts_and_keep_other_form_content() {
    let mut tabs = TabManager::new(800.0, 600.0);
    let tab = tabs.default_tab();
    tabs.get_mut(tab).unwrap().load_html_str("<form id=form><input id=upload type=file aria-label=Upload></form><input id=other type=file aria-label=Other>",Some("https://example.test/".into()));
    let target = ScriptDocumentTarget {
        tab_id: tab.as_u64(),
        document_generation: 1,
    };
    let lookup = |tabs: &mut TabManager, id: &str| {
        let ScriptReply::Node { node: Some(node) } = handle_script_request(
            tabs,
            ScriptRequest::GetElementById {
                target,
                id: id.into(),
            },
        ) else {
            panic!("node");
        };
        node
    };
    let upload = lookup(&mut tabs, "upload");
    let other = lookup(&mut tabs, "other");
    let form = lookup(&mut tabs, "form");
    let raw = |tabs: &TabManager, name: &str| {
        tabs.get(tab)
            .unwrap()
            .snapshot(0, 1)
            .nodes
            .into_iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap()
            .id
    };
    let raw_upload = raw(&tabs, "Upload");
    let raw_other = raw(&tabs, "Other");
    for (node, name) in [(raw_upload, "chosen.bin"), (raw_other, "other.bin")] {
        let page = tabs.get_mut(tab).unwrap();
        let state = page
            .file_input_state(node, 42, target.document_generation)
            .unwrap();
        page.set_file_input(
            &state.context,
            42,
            vec![FileData {
                name: name.into(),
                media_type: "application/octet-stream".into(),
                last_modified: 5,
                bytes: vec![0, 255],
            }],
        )
        .unwrap();
    }
    assert_eq!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::GetFileValue {
                target,
                node: upload
            }
        ),
        ScriptReply::FileValue {
            value: Some("C:\\fakepath\\chosen.bin".into())
        }
    );
    let previous = tabs
        .get(tab)
        .unwrap()
        .file_input_state(raw_upload, 42, target.document_generation)
        .unwrap();
    assert_eq!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::ClearInputFiles {
                target,
                node: upload
            }
        ),
        ScriptReply::Ack
    );
    assert!(tabs
        .get(tab)
        .unwrap()
        .validate_file_input(&previous.context, 42)
        .is_err());
    assert_eq!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::GetFileValue {
                target,
                node: upload
            }
        ),
        ScriptReply::FileValue {
            value: Some(String::new())
        }
    );
    let page = tabs.get_mut(tab).unwrap();
    let state = page
        .file_input_state(raw_upload, 42, target.document_generation)
        .unwrap();
    page.set_file_input(
        &state.context,
        42,
        vec![FileData {
            name: "second.bin".into(),
            media_type: "application/octet-stream".into(),
            last_modified: 8,
            bytes: vec![7],
        }],
    )
    .unwrap();
    let previous = page
        .file_input_state(raw_upload, 42, target.document_generation)
        .unwrap();
    assert_eq!(
        handle_script_request(&mut tabs, ScriptRequest::ResetForm { target, node: form }),
        ScriptReply::Ack
    );
    assert!(tabs
        .get(tab)
        .unwrap()
        .validate_file_input(&previous.context, 42)
        .is_err());
    assert!(
        matches!(handle_script_request(&mut tabs,ScriptRequest::GetInputFiles {target,node:upload}),ScriptReply::InputFiles {files:Some(files),..} if files.is_empty())
    );
    assert!(
        matches!(handle_script_request(&mut tabs,ScriptRequest::GetInputFiles {target,node:other}),ScriptReply::InputFiles {files:Some(files),..} if files.len()==1&&files[0].name=="other.bin")
    );
    assert!(matches!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::ResetForm {
                target,
                node: other
            }
        ),
        ScriptReply::Error { .. }
    ));
}
