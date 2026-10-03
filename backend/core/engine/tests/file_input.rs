// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_dom::NodeId;
use blueice_engine::Page;
use blueice_ipc::file_input::{FileData, MAX_SELECTED_BYTES};
use blueice_ipc::{NodeAction, Role};
use blueice_paint::PaintCommand;

fn file(name: &str) -> FileData {
    FileData {
        name: name.into(),
        media_type: "application/octet-stream".into(),
        bytes: vec![0, 255, 13, 10, 7],
    }
}
fn node(page: &Page, name: &str) -> u64 {
    page.snapshot(0, 1)
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some(name))
        .unwrap()
        .id
}

#[test]
fn selected_files_use_basename_only_in_shared_pixels_and_semantics() {
    let mut page = Page::new(800.0, 600.0);
    page.load_html_str("<input type=file aria-label=Upload value='/etc/passwd' multiple accept='.txt,application/pdf'><input type=file aria-label=Other>",None);
    let id = node(&page, "Upload");
    let state = page.file_input_state(id, 42, 1).unwrap();
    assert!(state.multiple);
    assert_eq!(state.accept, ".txt,application/pdf");
    assert!(state.names.is_empty());
    page.set_file_input(
        &state.context,
        42,
        vec![file("中文.bin"), file("second.bin")],
    )
    .unwrap();
    let snapshot = page.snapshot(0, 1);
    let input = snapshot.nodes.iter().find(|n| n.id == id).unwrap();
    assert_eq!(input.role, Role::Button);
    assert!(input.state.file_input);
    assert_eq!(input.state.value.as_deref(), Some("中文.bin, second.bin"));
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains("/etc/passwd"));
    assert!(page
        .render()
        .commands
        .iter()
        .any(|c| matches!(c,PaintCommand::Text{text,..} if text.contains("中文.bin"))));
    assert!(page
        .file_input_state(node(&page, "Other"), 42, 1)
        .unwrap()
        .names
        .is_empty());
    page.act(
        NodeId::from_u64(id),
        NodeAction::SetValue("/private/secret".into()),
    );
    assert_eq!(
        page.file_input_state(id, 42, 1).unwrap().names,
        vec!["中文.bin", "second.bin"]
    );
    let selected = page.file_input_state(id, 42, 1).unwrap();
    page.act(NodeId::from_u64(id), NodeAction::SetValue(String::new()));
    assert!(page.file_input_state(id, 42, 1).unwrap().names.is_empty());
    assert!(page
        .set_file_input(&selected.context, 42, vec![file("late-after-clear")])
        .is_err());
}

#[test]
fn selection_is_atomic_bounded_and_invalidated_by_navigation() {
    let mut page = Page::new(800.0, 600.0);
    page.load_html_str("<form><input type=file aria-label=Upload><button type=reset aria-label=Reset>Reset</button></form>",None);
    let id = node(&page, "Upload");
    let state = page.file_input_state(id, 42, 1).unwrap();
    assert!(page
        .set_file_input(&state.context, 42, vec![file("one"), file("two")])
        .is_err());
    for bad in [
        "../outside",
        "/etc/passwd",
        "C:\\secret",
        ".",
        "..",
        "bad\nname",
    ] {
        assert!(page
            .set_file_input(&state.context, 42, vec![file(bad)])
            .is_err());
    }
    let mut huge = file("too-large");
    huge.bytes = vec![0; MAX_SELECTED_BYTES + 1];
    assert!(page.set_file_input(&state.context, 42, vec![huge]).is_err());
    page.set_file_input(&state.context, 42, vec![file("retained")])
        .unwrap();
    assert!(page
        .set_file_input(&state.context, 42, vec![file("late")])
        .is_err());
    let selected = page.file_input_state(id, 42, 1).unwrap();
    page.load_html_str("<input type=file aria-label=Upload>", None);
    assert!(page
        .set_file_input(&selected.context, 42, vec![file("old-document")])
        .is_err());
}

#[test]
fn disabled_hidden_non_file_and_wrong_source_targets_never_accept_files() {
    let mut page = Page::new(800.0, 600.0);
    page.load_html_str("<input type=file aria-label=Upload><fieldset disabled><input type=file aria-label=Disabled></fieldset><input aria-label=Text><input type=file hidden aria-label=Hidden>",None);
    for name in ["Disabled", "Text"] {
        assert!(page.file_input_state(node(&page, name), 42, 1).is_err());
    }
    assert!(page.file_input_state(node(&page, "Upload"), 42, 2).is_err());
    let state = page.file_input_state(node(&page, "Upload"), 42, 1).unwrap();
    assert!(page
        .set_file_input(&state.context, 43, vec![file("wrong-source")])
        .is_err());
    assert!(!format!("{:?}", file("private-basename")).contains("private-basename"));
}

#[test]
fn document_content_and_metadata_budgets_are_atomic_and_replacements_release_capacity() {
    let mut page = Page::new(800.0, 600.0);
    page.load_html_str(
        &(0..5)
            .map(|i| format!("<input type=file multiple aria-label=Files{i}>"))
            .collect::<String>(),
        None,
    );
    for i in 0..4 {
        let state = page
            .file_input_state(node(&page, &format!("Files{i}")), 42, 1)
            .unwrap();
        let mut large = file("large.bin");
        large.bytes = vec![0; MAX_SELECTED_BYTES];
        page.set_file_input(&state.context, 42, vec![large])
            .unwrap();
    }
    let fifth = page.file_input_state(node(&page, "Files4"), 42, 1).unwrap();
    assert!(page
        .set_file_input(&fifth.context, 42, vec![file("over-budget")])
        .is_err());
    let first = page.file_input_state(node(&page, "Files0"), 42, 1).unwrap();
    page.set_file_input(&first.context, 42, vec![]).unwrap();
    let fifth = page.file_input_state(node(&page, "Files4"), 42, 1).unwrap();
    page.set_file_input(&fifth.context, 42, vec![file("released-capacity")])
        .unwrap();
    page.load_html_str(
        &(0..5)
            .map(|i| format!("<input type=file multiple aria-label=Files{i}>"))
            .collect::<String>(),
        None,
    );
    for i in 0..4 {
        let state = page
            .file_input_state(node(&page, &format!("Files{i}")), 42, 2)
            .unwrap();
        let mut empty = file("empty.bin");
        empty.bytes.clear();
        page.set_file_input(&state.context, 42, vec![empty; 16])
            .unwrap();
    }
    let fifth = page.file_input_state(node(&page, "Files4"), 42, 2).unwrap();
    assert!(page
        .set_file_input(&fifth.context, 42, vec![file("metadata-over-budget")])
        .is_err());
    assert!(page
        .file_input_state(node(&page, "Files4"), 42, 2)
        .unwrap()
        .names
        .is_empty());
}
