// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source Map v3 decoder and bounded-publication regressions.

use super::*;
use blueice_bluets::{CompilerLimits, DiagnosticCode};

#[test]
fn mapping_decoder_rejects_invalid_segment_order_and_encoding() {
    let overflowing_vlq = format!("g{}A", "/".repeat(13));
    for invalid in ["AAAA,AAAA", "D", "AAAA,", "!", &overflowing_vlq] {
        assert!(
            std::panic::catch_unwind(|| decode_mappings(invalid)).is_err(),
            "invalid map was accepted: {invalid}"
        );
    }
    assert_eq!(decode_mappings("AAAA;A").len(), 2);
}

#[test]
fn mapping_decoder_rejects_generated_positions_past_javascript() {
    let map = decode_mappings("KAAA;AAAA");
    assert!(
        std::panic::catch_unwind(|| super::decoding::validate_generated_bounds("x", &map)).is_err()
    );
    let map = decode_mappings("CAAA");
    assert!(
        std::panic::catch_unwind(|| super::decoding::validate_generated_bounds("", &map)).is_err()
    );
    let map = decode_mappings("AAAA;AAAA");
    super::decoding::validate_generated_bounds("x\ny", &map);
}

#[test]
fn mapping_decoder_rejects_out_of_range_table_indices() {
    for encoded in ["ACAA", "AAAAA"] {
        let map = serde_json::json!({
            "version": 3,
            "sources": ["main.ts"],
            "names": [],
            "mappings": encoded
        });
        assert!(
            std::panic::catch_unwind(|| decode_map(&map)).is_err(),
            "invalid map was accepted: {encoded}"
        );
    }
}

#[test]
fn mapping_decoder_rejects_original_positions_past_inline_source() {
    for encoded in ["AAEA", "AAAG"] {
        let map = serde_json::json!({
            "version": 3,
            "sources": ["main.ts"],
            "sourcesContent": ["a\n😀b"],
            "names": [],
            "mappings": encoded
        });
        assert!(
            std::panic::catch_unwind(|| decode_map(&map)).is_err(),
            "out-of-bounds original location was accepted: {encoded}"
        );
    }
    let missing_content = serde_json::json!({
        "version": 3,
        "sources": ["first.ts", "second.ts"],
        "sourcesContent": ["a"],
        "names": [],
        "mappings": "AAAA"
    });
    assert!(std::panic::catch_unwind(|| decode_map(&missing_content)).is_err());
    let valid = serde_json::json!({
        "version": 3,
        "sources": ["main.ts"],
        "sourcesContent": ["a\r\n😀b"],
        "names": [],
        "mappings": "AACA,EAAG"
    });
    assert_eq!(
        decode_map(&valid).len(),
        2,
        "astral columns are UTF-16 units"
    );
}

#[test]
fn mapping_decoder_preserves_named_segment_indices() {
    let map = serde_json::json!({
        "version": 3,
        "file": "named.js",
        "sourceRoot": "",
        "sources": ["named.ts"],
        "names": ["first", "second"],
        "mappings": "AAAAA,MAAOC"
    });
    assert_eq!(
        decode_map(&map).into_iter().collect::<Vec<_>>(),
        vec![
            Mapping {
                generated_line: 0,
                generated_column: 0,
                source_index: 0,
                source_line: 0,
                source_column: 0,
                name_index: Some(0),
            },
            Mapping {
                generated_line: 0,
                generated_column: 6,
                source_index: 0,
                source_line: 0,
                source_column: 7,
                name_index: Some(1),
            },
        ]
    );
}

#[test]
fn source_map_budget_rejects_without_partial_artifact() {
    let module = "memory:///limited.ts";
    let source = include_str!("../fixtures/k11_source_maps/basic.ts");
    let result = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            source_map: true,
            limits: CompilerLimits {
                max_source_map_segments: 1,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(
        result.output.is_none(),
        "no map or JavaScript may be published"
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("segment limit")
    }));
}

#[test]
fn copied_comments_and_const_keep_measured_segment_boundaries() {
    let module = "memory:///main.ts";
    let source = include_str!("../fixtures/k11_source_maps/basic.ts");
    let result = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            source_map: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let map: Value = serde_json::from_str(
        &result.output.unwrap().artifacts[module]
            .source_map
            .as_ref()
            .unwrap()
            .to_json(),
    )
    .unwrap();
    let segments = decode_map(&map);
    for (line, end) in [(0, 70), (1, 70), (2, 60)] {
        assert!(segments.contains(&Mapping {
            generated_line: line,
            generated_column: end,
            source_index: 0,
            source_line: line as i64,
            source_column: end,
            name_index: None,
        }));
    }
    assert!(
        !segments
            .iter()
            .any(|segment| segment.generated_line == 3 && segment.generated_column == 12),
        "Native has no separate marker after the copied `const` keyword"
    );
    assert!(
        !segments.iter().any(|segment| segment.generated_line == 6),
        "terminal newline cannot create an empty mapped line"
    );
}

#[test]
fn emitted_token_oracle_groups_strings_and_operators() {
    let javascript = "const value = \"astral 😀 text\"; value === 1; // map trailer\n";
    let tokens = mapped_tokens(javascript, &decode_mappings("AAAA"));
    assert_eq!(
        tokens
            .iter()
            .map(|token| token.token.as_str())
            .collect::<Vec<_>>(),
        [
            "const",
            "value",
            "=",
            "\"astral 😀 text\"",
            ";",
            "value",
            "===",
            "1",
            ";",
        ]
    );
    assert!(tokens.iter().all(|token| token.source_index == 0));
}

#[test]
fn emitted_token_lookup_counts_astral_utf16_columns() {
    // `next` begins at UTF-8 byte 7 but UTF-16 column 5.
    let javascript = "\"😀\";next";
    let tokens = mapped_tokens(javascript, &decode_mappings("AAAA,KAAK"));
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[2].token, "next");
    assert_eq!(tokens[2].source_column, 5);
}
