// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::{script::handle_script_request, ObjectUrlPurpose, TabId, TabManager};
use blueice_ipc::script::{ScriptDocumentTarget, ScriptReply, ScriptRequest};

fn document(tabs: &mut TabManager, tab: TabId) -> ScriptDocumentTarget {
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<p>creator</p>", Some("https://example.test/".into()));
    let target = ScriptDocumentTarget {
        tab_id: tab.as_u64(),
        document_generation: 1,
    };
    assert!(matches!(
        handle_script_request(tabs, ScriptRequest::GetDocument { target }),
        ScriptReply::NodeCreated { .. }
    ));
    target
}

#[test]
fn script_blob_upload_is_document_scoped_and_publishes_complete_binary_data() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let tab = tabs.default_tab();
    let target = document(&mut tabs, tab);
    let ScriptReply::BlobUrlUpload { upload_id } = handle_script_request(
        &mut tabs,
        ScriptRequest::BeginBlobUrl {
            target,
            size: 3,
            media_type: "application/octet-stream".into(),
        },
    ) else {
        panic!("upload should begin")
    };
    assert!(matches!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::CommitBlobUrl { target, upload_id }
        ),
        ScriptReply::Error { .. }
    ));
    let peer = tabs.open_tab();
    let foreign = document(&mut tabs, peer);
    assert!(matches!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::AppendBlobUrl {
                target: foreign,
                upload_id,
                offset: 0,
                bytes: vec![7, 0, 255],
            }
        ),
        ScriptReply::Error { .. }
    ));
    assert_eq!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::AppendBlobUrl {
                target,
                upload_id,
                offset: 0,
                bytes: vec![7, 0, 255],
            }
        ),
        ScriptReply::Ack
    );
    let ScriptReply::BlobUrl { url } = handle_script_request(
        &mut tabs,
        ScriptRequest::CommitBlobUrl { target, upload_id },
    ) else {
        panic!("complete upload should publish")
    };
    assert_eq!(
        tabs.resolve_object_url(peer, &url, ObjectUrlPurpose::Fetch)
            .unwrap()
            .bytes(),
        &[7, 0, 255]
    );
    assert_eq!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::RevokeBlobUrl {
                target: foreign,
                url: url.clone()
            }
        ),
        ScriptReply::Ack
    );
    assert!(tabs
        .resolve_object_url(tab, &url, ObjectUrlPurpose::Fetch)
        .is_none());
}

#[test]
fn stale_document_and_oversized_chunks_are_rejected_before_storage_changes() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let tab = tabs.default_tab();
    let target = document(&mut tabs, tab);
    let ScriptReply::BlobUrlUpload { upload_id } = handle_script_request(
        &mut tabs,
        ScriptRequest::BeginBlobUrl {
            target,
            size: 1,
            media_type: "text/plain".into(),
        },
    ) else {
        panic!("upload should begin")
    };
    assert!(matches!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::AppendBlobUrl {
                target,
                upload_id,
                offset: 0,
                bytes: vec![0; 65_537],
            }
        ),
        ScriptReply::Error { .. }
    ));
    tabs.get_mut(tab).unwrap().load_html_str(
        "<p>replacement</p>",
        Some("https://example.test/new".into()),
    );
    for request in [
        ScriptRequest::BeginBlobUrl {
            target,
            size: 1,
            media_type: "".into(),
        },
        ScriptRequest::AppendBlobUrl {
            target,
            upload_id,
            offset: 0,
            bytes: vec![0],
        },
        ScriptRequest::CommitBlobUrl { target, upload_id },
        ScriptRequest::AbortBlobUrl { target, upload_id },
        ScriptRequest::RevokeBlobUrl {
            target,
            url: "blob:https://example.test/unknown".into(),
        },
    ] {
        assert!(matches!(
            handle_script_request(&mut tabs, request),
            ScriptReply::Error { .. }
        ));
    }
}
