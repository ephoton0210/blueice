// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Each Native EOF+1 occurrence has an identity, a local correction and a CLI replay.
use super::{cli, decoded, inputs, javascript_maps};
use blueice_bluets::{lex, SourceMap, SourceMapLookupError};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn ledger() -> Value {
    serde_json::from_str(include_str!(
        "../fixtures/ge5_source_maps/native-map-defects.json"
    ))
    .unwrap()
}

fn map(text: &str) -> SourceMap {
    let value: Value = serde_json::from_str(text).unwrap();
    let strings = |name: &str| {
        value[name]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect()
    };
    SourceMap {
        file: value["file"].as_str().unwrap().into(),
        sources: strings("sources"),
        sources_content: strings("sourcesContent"),
        inline_sources: true,
        source_root: value["sourceRoot"].as_str().unwrap_or("").into(),
        names: strings("names"),
        mappings: value["mappings"].as_str().unwrap().into(),
    }
}

#[test]
fn native_defects_all_572_retain_stock_evidence_and_exact_local_corrections() {
    let ledger = ledger();
    let issues = ledger["defects"].as_array().unwrap();
    assert_eq!(issues.len(), 572);
    assert_eq!(ledger["counts"]["configurations"], 452);
    assert_eq!(
        ledger["counts"]["nativeSystemConfigurationsReplayedTwice"],
        516
    );
    assert_eq!(
        ledger["inputSha256"],
        inputs::sha(include_bytes!("../fixtures/ge5_source_maps/cases.json"))
    );
    let reference_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ge5_source_maps/reference.json");
    assert_eq!(
        ledger["referenceSha256"],
        inputs::sha(&std::fs::read(reference_path).unwrap())
    );
    let mut ids = BTreeSet::new();
    let mut forms = BTreeSet::new();
    let mut raw = BTreeSet::new();
    for (index, issue) in issues.iter().enumerate() {
        assert_eq!(issue["id"], format!("NATIVE-MAP-{:04}", index + 1));
        assert!(ids.insert(issue["id"].as_str().unwrap()));
        forms.insert(issue["form"].as_str().unwrap());
        raw.insert(issue["stockRawSha256"].as_str().unwrap());
        let case = &inputs::reference()["cases"][issue["caseIndex"].as_u64().unwrap() as usize];
        assert_eq!(case["id"], issue["caseId"]);
        let observation = &case["maps"][issue["map"].as_str().unwrap()];
        assert_eq!(observation["raw"], issue["stockRawSha256"]);
        assert_eq!(observation["coordinateIssues"], json!([issue["issue"]]));
        let original = inputs::blob(observation["raw"].as_str().unwrap());
        let corrected = ledger["patchedBlobs"][issue["patchedRawSha256"].as_str().unwrap()]
            .as_str()
            .unwrap();
        assert_eq!(inputs::sha(corrected.as_bytes()), issue["patchedRawSha256"]);
        let before = decoded::decoded(original);
        let after = decoded::decoded(corrected);
        assert_eq!(before["metadata"], after["metadata"]);
        let line = issue["issue"]["segment"][0].as_u64().unwrap();
        let segments = before["segments"].as_array().unwrap();
        let removed = segments
            .iter()
            .filter(|segment| segment[0] == line)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(json!(removed), issue["removedSegments"]);
        assert_eq!(removed.len(), 2);
        assert_eq!(
            after["segments"],
            json!(segments
                .iter()
                .filter(|segment| segment[0] != line)
                .collect::<Vec<_>>())
        );
        let javascript = inputs::blob(
            case["outputs"][issue["output"].as_str().unwrap()]
                .as_str()
                .unwrap(),
        );
        assert_eq!(
            map(original).original_position_at(javascript, 0, 0, 100_000, 32_000_000),
            Err(SourceMapLookupError::MalformedMap)
        );
        // Native allows ordered duplicate positions; preserve them. Check every
        // corrected segment's bounds independently of BlueTS's stricter producer.
        let metadata = &after["metadata"];
        let sources = metadata["sourcesContent"].as_array().unwrap();
        let widths = sources
            .iter()
            .map(|source| {
                source
                    .as_str()
                    .unwrap()
                    .split('\n')
                    .map(|line| line.encode_utf16().count())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let generated = javascript
            .split('\n')
            .map(|line| line.encode_utf16().count())
            .collect::<Vec<_>>();
        for segment in after["segments"].as_array().unwrap() {
            let fields = segment.as_array().unwrap();
            assert!(
                fields[1].as_u64().unwrap() as usize
                    <= generated[fields[0].as_u64().unwrap() as usize]
            );
            if fields.len() > 2 {
                let source = fields[2].as_u64().unwrap() as usize;
                let line = fields[3].as_u64().unwrap() as usize;
                assert!(fields[4].as_u64().unwrap() as usize <= widths[source][line]);
            }
        }
    }
    assert_eq!(forms.len(), 14);
    assert_eq!(raw.len(), 138);
}

fn position(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

fn execute_braces(javascript: &str) -> (usize, usize) {
    let tokens = lex("<emitted>", javascript).unwrap();
    let execute = tokens
        .windows(3)
        .position(|tokens| {
            tokens[0].text == "execute" && tokens[1].text == ":" && tokens[2].text == "function"
        })
        .expect("System execute function");
    let open = (execute + 3..tokens.len())
        .find(|index| tokens[*index].text == "{")
        .unwrap();
    let mut depth = 1;
    for token in &tokens[open + 1..] {
        if token.text == "{" {
            depth += 1;
        }
        if token.text == "}" {
            depth -= 1;
        }
        if depth == 0 {
            return (tokens[open].start, token.start);
        }
    }
    panic!("unclosed System execute function")
}

#[test]
fn native_defects_all_572_have_bounded_bluets_maps_and_unmapped_execute_braces() {
    let ledger = ledger();
    let mut cases = BTreeMap::<usize, Vec<&Value>>::new();
    for issue in ledger["defects"].as_array().unwrap() {
        cases
            .entry(issue["caseIndex"].as_u64().unwrap() as usize)
            .or_default()
            .push(issue);
    }
    assert_eq!(cases.len(), 452);
    let temporary = cli::Temporary::new("native-defects");
    let mut verified = Vec::new();
    for (index, issues) in cases {
        let specification = &inputs::input()["cases"][index];
        let row = inputs::resolve(specification);
        let expected = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(actual.success, "{}", specification["id"]);
        assert_eq!(actual.diagnostics, expected["diagnostics"]);
        assert_eq!(json!(actual.selected_inputs), expected["selectedInputs"]);
        assert_eq!(
            actual.outputs.keys().collect::<Vec<_>>(),
            expected["outputs"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>()
        );
        let failures = javascript_maps::compare(specification, &row, expected, &actual);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        for (name, hash) in expected["outputs"].as_object().unwrap() {
            if name.ends_with(".d.ts") || name.ends_with(".d.ts.map") {
                assert_eq!(
                    actual.outputs[name],
                    inputs::blob(hash.as_str().unwrap()),
                    "{}: {name}",
                    specification["id"]
                );
            }
        }
        for issue in issues {
            let javascript = &actual.outputs[issue["output"].as_str().unwrap()];
            let raw = &actual.outputs[issue["map"].as_str().unwrap()];
            let map = map(raw);
            let (open, close) = execute_braces(javascript);
            for offset in [open, open + 1, close, close + 1] {
                let (line, column) = position(javascript, offset);
                assert_eq!(
                    map.original_position_at(javascript, line, column, 100_000, 32_000_000)
                        .unwrap(),
                    None,
                    "{}: generated execute brace at {line}:{column}",
                    issue["id"]
                );
            }
            verified.push(json!({"id":issue["id"], "caseId":specification["id"],
                "map":issue["map"], "bluetsMapSha256":inputs::sha(raw.as_bytes()),
                "javascriptSha256":inputs::sha(javascript.as_bytes()), "repeatBuilds":2,
                "strictBounds":true,"generatedExecuteBracesUnmapped":true}));
        }
    }
    assert_eq!(verified.len(), 572);
    if let Some(filename) = std::env::var_os("BLUEICE_GE5_NATIVE_FIX_REPORT") {
        let binary = env!("CARGO_BIN_EXE_bluetsc");
        let report = json!({"schema":1, "ledgerSha256":inputs::sha(include_bytes!("../fixtures/ge5_source_maps/native-map-defects.json")),
            "bluetsBinarySha256":inputs::sha(&std::fs::read(binary).unwrap()),
            "configurations":452,"maps":572,"repeatBuilds":2,"verified":verified});
        std::fs::write(
            filename,
            serde_json::to_string_pretty(&report).unwrap() + "\n",
        )
        .unwrap();
    }
    eprintln!("G-E5 Native-defect replay: 452 configurations / 572 maps / two builds; strict bounds, raw declarations and generated execute braces pass");
}
