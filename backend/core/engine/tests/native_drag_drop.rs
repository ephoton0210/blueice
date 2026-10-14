// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use serde_json::{json, Value};
#[allow(dead_code)]
#[path = "fixtures/native_drag_browser.rs"]
mod browser;
use browser::{scoped, text, Browser};

#[test]
fn native_source_start_publishes_core_selection_preview_geometry_only_on_start() {
    let mut browser = Browser::new();
    let point = browser.select_source("Preview 中文");
    let state = browser
        .drag(json!({"Start":{"drag_id":91,"point":point,"allowed_operations":["Copy"]}}))
        .unwrap();
    let regions = state["image"]["regions"]
        .as_array()
        .expect("Core must provide selected source pixels' geometry");
    assert!(!regions.is_empty());
    assert!(regions
        .iter()
        .all(|region| region["width"].as_f64().unwrap() > 0.0
            && region["height"].as_f64().unwrap() > 0.0));
    let ended = browser
        .drag(json!({"End":{"drag_id":91,"operation":"Copy"}}))
        .unwrap();
    assert!(
        ended["image"].is_null(),
        "Ending or observing a drag cannot export another preview"
    );
    assert_eq!(browser.value("Drop text"), "Preview 中文");
}

#[test]
fn native_drag_edits_only_on_drop_and_rejects_replayed_payloads() {
    let mut browser = Browser::new();
    let original = browser.snapshot();
    let point = browser.point("Drop text");
    let enter = browser.enter(41, point.clone(), vec!["text/plain"]);
    assert_eq!(enter["operation"], "Copy");
    assert_eq!(enter["active"], true);
    assert_eq!(browser.value("Drop text"), "");
    assert_eq!(
        browser.snapshot()["generation"],
        original["generation"],
        "Metadata-only drag entry must not publish an unchanged frame"
    );
    assert!(browser.snapshot()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node["state"]["focused"] != true));
    let payload = text("Dropped 中文 😀");
    let previous_frame = browser.snapshot()["generation"].as_u64().unwrap();
    let dropped = browser
        .drag(json!({"Drop":{"drag_id":41,"point":point,"items":[payload.clone()]}}))
        .unwrap();
    assert_eq!(dropped["operation"], "Copy");
    assert_eq!(dropped["active"], false);
    assert_eq!(dropped["allowed_operations"], json!(["Copy"]));
    assert_eq!(browser.value("Drop text"), "Dropped 中文 😀");
    assert!(
        browser.snapshot()["generation"].as_u64().unwrap() > previous_frame,
        "An accepted native drop must publish a new rendered frame"
    );
    assert_eq!(browser.snapshot()["url"], original["url"]);
    assert!(browser
        .drag(json!({"Drop":{"drag_id":41,"point":point,"items":[payload]}}))
        .is_err());
    assert_eq!(browser.value("Drop text"), "Dropped 中文 😀");
}

#[test]
fn native_drag_mouse_snapshots_echo_exactly_and_invalid_points_do_not_consume_drop() {
    let mut browser = Browser::new();
    let point = browser.point("Drop text");
    let mouse = json!({"client_point":point,"screen_point":{"x":-800.75,"y":1024.125},
        "buttons":1,"modifiers":{"alt":true,"control":false,"meta":true,"shift":false,"caps_lock":true}});
    let action = json!({"Enter":{"drag_id":63,"point":point,"types":["text/plain"],"allowed_operations":["Copy"]}});
    let mut command = browser.command(action);
    command["NativeDrag"]["mouse"] = mouse.clone();
    assert_eq!(
        browser
            .request(scoped(command, 1, 1), "NativeDragState")
            .unwrap()["mouse"],
        mouse
    );
    let drop = json!({"Drop":{"drag_id":63,"point":point,"items":[text("Owned drop")],"expected_operation":"Copy"}});
    for (field, value) in [
        ("screen_point", json!({"x":1e9,"y":0})),
        ("client_point", json!({"x":-1,"y":0})),
    ] {
        let mut bad = browser.command(drop.clone());
        bad["NativeDrag"]["mouse"] = mouse.clone();
        bad["NativeDrag"]["mouse"][field] = value;
        assert!(browser
            .request(scoped(bad, 1, 1), "NativeDragState")
            .is_err());
        assert_eq!(browser.value("Drop text"), "");
    }
    let mut overflow = browser.command(drop.clone());
    overflow["NativeDrag"]["mouse"] = mouse.clone();
    overflow["NativeDrag"]["mouse"]["buttons"] = json!(65536);
    assert!(serde_json::from_value::<blueice_ipc::ClientMessage>(overflow).is_err());
    let mut command = browser.command(drop);
    command["NativeDrag"]["mouse"] = mouse.clone();
    let state = browser
        .request(scoped(command, 1, 1), "NativeDragState")
        .unwrap();
    assert_eq!(state["mouse"], mouse);
    assert_eq!(state["operation"], "Copy");
    assert_eq!(browser.value("Drop text"), "Owned drop");
}

#[test]
fn native_drag_fences_source_document_frame_window_and_context_before_mutation() {
    let mut browser = Browser::new();
    let point = browser.point("Drop text");
    assert_eq!(
        browser.enter(42, point.clone(), vec!["text/plain"])["operation"],
        "Copy"
    );
    let action = json!({"Drop":{"drag_id":42,"point":point,"items":[text("Stale payload")]}});
    for field in ["version", "frame_source", "document_generation"] {
        let mut command = browser.command(action.clone());
        command["NativeDrag"]["context"][field] =
            json!(command["NativeDrag"]["context"][field].as_u64().unwrap() + 1);
        assert!(browser
            .request(scoped(command, 1, 1), "NativeDragState")
            .is_err());
        assert_eq!(browser.value("Drop text"), "");
    }
    let mut stale_frame = browser.command(action.clone());
    stale_frame["NativeDrag"]["frame_generation"] = json!(
        stale_frame["NativeDrag"]["frame_generation"]
            .as_u64()
            .unwrap()
            + 1
    );
    assert!(browser
        .request(scoped(stale_frame, 1, 1), "NativeDragState")
        .is_err());
    for (context, window) in [(99, 1), (1, 99)] {
        let command = browser.command(action.clone());
        assert!(browser
            .request(scoped(command, context, window), "NativeDragState")
            .is_err());
    }
    assert_eq!(browser.value("Drop text"), "");
    assert_eq!(browser.drag(action).unwrap()["operation"], "Copy");
    assert_eq!(browser.value("Drop text"), "Stale payload");
}

#[test]
fn native_drag_leave_and_unavailable_controls_never_apply_a_payload() {
    let mut browser = Browser::new();
    let active = browser.point("Drop text");
    assert_eq!(
        browser.enter(43, active.clone(), vec!["text/plain"])["operation"],
        "Copy"
    );
    assert_eq!(
        browser.drag(json!({"Leave":{"drag_id":43}})).unwrap()["active"],
        false
    );
    assert!(browser
        .drag(json!({"Drop":{"drag_id":43,"point":active,"items":[text("After leave")]}}))
        .is_err());
    for (index, name) in ["Disabled drop", "Inert drop", "Read only drop"]
        .iter()
        .enumerate()
    {
        let original = browser.value(name);
        let point = browser.point(name);
        let drag_id = 50 + index as u64;
        assert_eq!(
            browser.enter(drag_id, point.clone(), vec!["text/plain"])["operation"],
            "None"
        );
        let result = browser.drag(
            json!({"Drop":{"drag_id":drag_id,"point":point,"items":[text("Rejected payload")]}}),
        );
        assert!(result.is_err() || result.unwrap()["operation"] == "None");
        assert_eq!(browser.value(name), original);
    }
    assert_eq!(browser.value("Drop text"), "");
}

#[test]
fn native_drag_file_default_uses_selected_bytes_and_refuses_unadvertised_file_payloads() {
    let mut browser = Browser::new();
    let plain = browser.point("Drop text");
    assert_eq!(
        browser.enter(61, plain.clone(), vec!["text/plain"])["operation"],
        "Copy"
    );
    let file = json!({"File":{"name":"chosen-中文.bin","media_type":"application/octet-stream","last_modified":123,"bytes":[0,255,7]}});
    assert!(browser
        .drag(json!({"Drop":{"drag_id":61,"point":plain,"items":[file.clone()]}}))
        .is_err());
    assert_eq!(browser.value("Drop text"), "");
    let point = browser.point("Drop files");
    assert_eq!(
        browser.enter(62, point.clone(), vec!["Files"])["operation"],
        "Copy"
    );
    let result = browser
        .drag(json!({"Drop":{"drag_id":62,"point":point,"items":[file]}}))
        .unwrap();
    assert_eq!(result["active"], false);
    assert_eq!(browser.value("Drop files"), "chosen-中文.bin");
    assert_eq!(browser.snapshot()["url"], "https://example.test/drag");
}

#[test]
fn native_drop_refuses_a_changed_negotiated_operation_before_disclosing_or_editing() {
    let mut browser = Browser::new();
    let point = browser.point("Drop text");
    assert_eq!(
        browser.enter(71, point.clone(), vec!["text/plain"])["operation"],
        "Copy"
    );
    let rejected = browser
        .drag(json!({"Drop":{
            "drag_id":71,"point":point,"items":[text("Rejected effect")],"expected_operation":"Move"
        }}))
        .unwrap();
    assert_eq!(rejected["active"], false);
    assert_eq!(rejected["operation"], "None");
    assert_eq!(browser.value("Drop text"), "");
    assert!(browser
        .drag(json!({"Drop":{
            "drag_id":71,"point":point,"items":[text("Replayed effect")],"expected_operation":"Copy"
        }}))
        .is_err());
    assert_eq!(
        browser.enter(72, point.clone(), vec!["text/plain"])["operation"],
        "Copy"
    );
    assert_eq!(browser.drag(json!({"Drop":{
        "drag_id":72,"point":point,"items":[text("Accepted effect")],"expected_operation":"Copy"
    }})).unwrap()["operation"], "Copy");
    assert_eq!(browser.value("Drop text"), "Accepted effect");
}

#[test]
fn native_drag_source_exports_only_core_selection_and_cancel_never_edits_or_replays() {
    let mut browser = Browser::new();
    let point = browser.select_source("Source😀");
    let started = browser
        .drag(json!({"Start":{"drag_id":81,"point":point,"allowed_operations":["Copy","Move"]}}))
        .unwrap();
    assert_eq!(started["active"], true);
    assert_eq!(started["items"], json!([text("Source😀")]));
    assert_eq!(browser.value("Drop text"), "Source😀");
    assert!(browser
        .drag(json!({"End":{"drag_id":81,"operation":"None","point":{"x":1e9,"y":0.0}}}))
        .is_err());
    let mut end = browser
        .command(json!({"End":{"drag_id":81,"operation":"None","point":{"x":-20.0,"y":-30.0}}}));
    end["NativeDrag"]["frame_generation"] = json!(0);
    let ended = browser
        .request(scoped(end, 1, 1), "NativeDragState")
        .unwrap();
    assert_eq!(ended["active"], false);
    assert_eq!(browser.value("Drop text"), "Source😀");
    assert!(browser
        .drag(json!({"End":{"drag_id":81,"operation":"Move"}}))
        .is_err());
    assert!(browser
        .drag(json!({"Start":{"drag_id":81,"point":point,"allowed_operations":["Copy"]}}))
        .is_err());
    assert_eq!(browser.value("Drop text"), "Source😀");
}

#[test]
fn native_source_move_deletes_the_owned_selection_once_and_undo_restores_it() {
    let mut browser = Browser::new();
    let point = browser.select_source("Owned 中文😀");
    let started = browser
        .drag(json!({"Start":{"drag_id":82,"point":point,"allowed_operations":["Move"]}}))
        .unwrap();
    assert_eq!(started["items"], json!([text("Owned 中文😀")]));
    let ended = browser
        .drag(json!({"End":{"drag_id":82,"operation":"Move"}}))
        .unwrap();
    assert_eq!(ended["operation"], "Move");
    assert_eq!(ended["allowed_operations"], json!(["Move"]));
    assert_eq!(browser.value("Drop text"), "");
    browser.input(json!("Undo"));
    assert_eq!(browser.value("Drop text"), "Owned 中文😀");
    assert!(browser
        .drag(json!({"End":{"drag_id":82,"operation":"Move"}}))
        .is_err());
    assert_eq!(browser.value("Drop text"), "Owned 中文😀");
}

#[test]
fn document_drag_uses_public_selection_geometry_and_advertises_static_source_metadata() {
    let mut browser = Browser::new();
    let state = browser.input(json!("DocumentSelectAll"));
    assert_eq!(state["document"]["selected_text"], "Drag document words");
    let rect = &state["document"]["selection_rects"][0];
    let point = json!({"x":rect["x"].as_f64().unwrap()+1.0,"y":rect["y"].as_f64().unwrap()+rect["height"].as_f64().unwrap()/2.0-state["scroll_y"].as_f64().unwrap()});
    let node = browser.node("Drag document words");
    assert_eq!(node["state"]["native_drag_source"], true);
    let started = browser
        .drag(json!({"Start":{"drag_id":83,"point":point,"allowed_operations":["Copy","Move"]}}))
        .unwrap();
    assert_eq!(started["items"], json!([text("Drag document words")]));
    assert_eq!(started["allowed_operations"], json!(["Copy"]));
    browser
        .drag(json!({"End":{"drag_id":83,"operation":"Copy"}}))
        .unwrap();
    assert_eq!(
        browser.node("Drag document words")["name"],
        "Drag document words"
    );
}

#[test]
fn native_source_motion_keeps_payload_protected_and_fences_frames_before_cancel() {
    let mut browser = Browser::new();
    let point = browser.select_source("Motion source");
    browser
        .drag(json!({"Start":{"drag_id":84,"point":point,"allowed_operations":["Copy","Move"]}}))
        .unwrap();
    let action = json!({"SourceMove":{"drag_id":84,"point":{"x":-10.0,"y":-20.0}}});
    let mut stale = browser.command(action.clone());
    stale["NativeDrag"]["frame_generation"] =
        json!(stale["NativeDrag"]["frame_generation"].as_u64().unwrap() + 1);
    assert!(browser
        .request(scoped(stale, 1, 1), "NativeDragState")
        .is_err());
    let moved = browser.drag(action).unwrap();
    assert_eq!(moved["active"], true);
    assert!(moved.get("items").is_none());
    assert_eq!(browser.value("Drop text"), "Motion source");
    browser
        .drag(json!({"End":{"drag_id":84,"operation":"None"}}))
        .unwrap();
    assert_eq!(browser.value("Drop text"), "Motion source");
    assert!(browser
        .drag(json!({"SourceMove":{"drag_id":84,"point":point}}))
        .is_err());
}

#[test]
fn readonly_drag_source_allows_copy_and_refuses_move_without_consuming_the_source() {
    let mut browser = Browser::new();
    browser.input(json!({"Key":{"key":"Tab","shift":false}}));
    browser.input(json!({"Key":{"key":"Tab","shift":false}}));
    let state = browser.input(json!("SelectAll"));
    assert_eq!(state["focused"]["text"], "read only retained");
    let rect = &state["focused"]["selection_rects"][0];
    let point = json!({"x":rect["x"].as_f64().unwrap()+1.0,"y":rect["y"].as_f64().unwrap()+rect["height"].as_f64().unwrap()/2.0-state["scroll_y"].as_f64().unwrap()});
    let started = browser
        .drag(json!({"Start":{"drag_id":85,"point":point,"allowed_operations":["Copy","Move"]}}))
        .unwrap();
    assert_eq!(started["allowed_operations"], json!(["Copy"]));
    assert_eq!(started["items"], json!([text("read only retained")]));
    assert!(browser
        .drag(json!({"End":{"drag_id":85,"operation":"Move"}}))
        .is_err());
    assert_eq!(browser.value("Read only drop"), "read only retained");
    browser
        .drag(json!({"End":{"drag_id":85,"operation":"Copy"}}))
        .unwrap();
    assert_eq!(browser.value("Read only drop"), "read only retained");
}

#[test]
fn password_selection_never_becomes_an_ordinary_drag_payload() {
    let mut browser = Browser::new();
    let node = browser.node("Protected source");
    let mut state = Value::Null;
    for _ in 0..8 {
        state = browser.input(json!({"Key":{"key":"Tab","shift":false}}));
        if state["focused_node"] == node["id"] {
            break;
        }
    }
    assert_eq!(state["focused_node"], node["id"]);
    let state = browser.input(json!("SelectAll"));
    assert_eq!(state["focused"]["protected"], true);
    assert!(state["focused"]["text"].is_null());
    let point = browser.point("Protected source");
    assert!(browser
        .drag(json!({"Start":{"drag_id":86,"point":point,"allowed_operations":["Copy","Move"]}}))
        .is_err());
    assert!(browser.node("Protected source")["state"]["value"].is_null());
}

#[test]
fn empty_draggable_sources_keep_lifetimes_without_inventing_payload_content() {
    let mut browser = Browser::new();
    let point = browser.point("Drag document words");
    for id in [87, 88] {
        let source = browser
            .drag(json!({"Start":{"drag_id":id,"point":point,"allowed_operations":["Copy"]}}))
            .unwrap();
        assert_eq!(source["active"], true);
        assert_eq!(source["items"], json!([]));
        let moved = browser
            .drag(json!({"SourceMove":{"drag_id":id,"point":point}}))
            .unwrap();
        assert_eq!(moved["active"], true);
        assert!(moved.get("items").is_none());
        browser
            .drag(json!({"End":{"drag_id":id,"operation":"None"}}))
            .unwrap();
    }
    assert_eq!(
        browser.node("Drag document words")["name"],
        "Drag document words"
    );
}
