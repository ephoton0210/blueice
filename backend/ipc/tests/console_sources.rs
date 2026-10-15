// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ipc::console::ConsoleMessages;
use serde_json::{json, Value};

fn packet(source: Option<Value>, count: usize, message: &str) -> Value {
    let records = (1..=count)
        .map(|sequence| {
            let mut record = json!({
                "sequence":sequence, "timestamp_ms":0, "method":"error",
                "level":"error", "group_depth":0, "message":message, "truncated":false
            });
            if let Some(source) = &source {
                record
                    .as_object_mut()
                    .unwrap()
                    .insert("source".into(), source.clone());
            }
            record
        })
        .collect::<Vec<_>>();
    json!({"tab_id":1,"document_generation":7,"records":records,"dropped_records":0})
}

#[test]
fn copied_console_sources_round_trip_and_legacy_records_remain_unlocated() {
    let source = json!({"filename":"page:///original.js","line":4,"column":25});
    let wire = packet(Some(source.clone()), 1, "Uncaught probe");
    let decoded: ConsoleMessages = serde_json::from_value(wire.clone()).unwrap();
    assert!(decoded.is_well_formed());
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    for absent in [None, Some(Value::Null)] {
        let decoded: ConsoleMessages = serde_json::from_value(packet(absent, 1, "legacy")).unwrap();
        assert!(decoded.is_well_formed());
        assert!(serde_json::to_value(decoded).unwrap()["records"][0]
            .get("source")
            .is_none());
    }
}

#[test]
fn copied_console_sources_refuse_invalid_coordinates_names_and_shapes() {
    let mut accepted = Vec::new();
    for (index, source) in [
        json!({"filename":"page:///a.js","line":0,"column":1}),
        json!({"filename":"page:///a.js","line":1,"column":0}),
        json!({"filename":"","line":1,"column":1}),
        json!({"filename":"x".repeat(8193),"line":1,"column":1}),
        json!({"filename":"é".repeat(4097),"line":1,"column":1}),
        json!({"filename":"page:///a.js","line":-1,"column":1}),
        json!({"filename":"page:///a.js","line":4294967296u64,"column":1}),
        json!({"filename":"page:///a.js","line":1,"column":1.5}),
        json!({"filename":7,"line":1,"column":1}),
        json!({"filename":"page:///a.js","line":1}),
        json!("forged"),
    ]
    .into_iter()
    .enumerate()
    {
        if let Ok(decoded) =
            serde_json::from_value::<ConsoleMessages>(packet(Some(source.clone()), 1, "error"))
        {
            if decoded.is_well_formed() {
                accepted.push(index);
            }
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted malformed locations: {accepted:?}"
    );
}

#[test]
fn copied_console_source_bytes_share_the_existing_batch_payload_limit() {
    let source = json!({"filename":"f".repeat(4096),"line":1,"column":1});
    let full: ConsoleMessages =
        serde_json::from_value(packet(Some(source.clone()), 32, &"m".repeat(4096))).unwrap();
    assert!(full.is_well_formed());
    let excess: ConsoleMessages =
        serde_json::from_value(packet(Some(source), 33, &"m".repeat(4096))).unwrap();
    assert!(
        !excess.is_well_formed(),
        "source names must participate in the 256 KiB text budget"
    );
    let maximum_name = json!({"filename":"é".repeat(4096),"line":u32::MAX,"column":u32::MAX});
    let valid: ConsoleMessages = serde_json::from_value(packet(Some(maximum_name), 1, "")).unwrap();
    assert!(valid.is_well_formed());
}
