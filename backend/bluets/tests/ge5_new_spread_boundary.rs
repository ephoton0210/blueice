// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public CLI construction completions retain logical operand boundaries.
#![allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
use blueice_bluets::{lex, SourceMap};
use serde_json::Value;

fn map(raw: &str) -> SourceMap {
    let value: Value = serde_json::from_str(raw).unwrap();
    assert_eq!(value["version"], 3);
    let strings = |key: &str| {
        value[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect()
    };
    SourceMap {
        file: value["file"].as_str().unwrap().to_owned(),
        source_root: value["sourceRoot"].as_str().unwrap().to_owned(),
        sources: strings("sources"),
        sources_content: strings("sourcesContent"),
        names: strings("names"),
        inline_sources: true,
        mappings: value["mappings"].as_str().unwrap().to_owned(),
    }
}
fn coordinate(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

#[test]
fn construction_spread_completion_keeps_operand_end_and_following_property() {
    inputs::verify_authorities();
    let temporary = cli::Temporary::new("new-spread-endpoint");
    let mut failures = Vec::new();
    let mut products = 0;
    for index in [448, 496] {
        for variant in ["original", "crlf", "lfAstral", "crlfAstral"] {
            for spread in [true, false] {
                let mut specification = inputs::input()["cases"][index].clone();
                specification["variant"] = variant.into();
                let mut row = inputs::resolve(&specification);
                if !spread {
                    let source = row["files"]["main.ts"]
                        .as_str()
                        .unwrap()
                        .replace("new Derived(...values)", "new Derived(40,2)")
                        .replace("new (choose())(...values())", "new (choose())(40,2)");
                    row["files"]["main.ts"] = source.into();
                }
                let original = row["files"]["main.ts"].as_str().unwrap();
                let source_tokens = lex("main.ts", original).unwrap();
                let original_dot = source_tokens
                    .windows(2)
                    .rposition(|pair| pair[0].is(".") && pair[1].is("value"))
                    .unwrap();
                assert!(source_tokens[original_dot - 1].is(")"));
                // Frozen Native 448/496 use the final spread operand's end;
                // a retained ordinary construction uses the whole expression.
                let expected = coordinate(
                    original,
                    if spread {
                        source_tokens[original_dot - 2].end
                    } else {
                        source_tokens[original_dot - 1].end
                    },
                );
                let expected_property = coordinate(original, source_tokens[original_dot + 1].start);
                let actual = cli::observe_builds(&temporary, index, &row, 2);
                assert!(actual.success, "{}", actual.diagnostics);
                assert_eq!(actual.diagnostics, serde_json::json!([]));
                assert_eq!(actual.outputs.len(), 4);
                let js = &actual.outputs["out/main.js"];
                let map = map(&actual.outputs["out/main.js.map"]);
                let generated_tokens = lex("out/main.js", js).unwrap();
                let generated_dot = generated_tokens
                    .windows(2)
                    .rposition(|pair| pair[0].is(".") && pair[1].is("value"))
                    .unwrap();
                let lookup = |offset| {
                    let (line, column) = coordinate(js, offset);
                    map.original_position_at(js, line, column, 100_000, 1_000_000)
                        .unwrap()
                        .map(|owner| (owner.line, owner.column_utf16))
                };
                let observed = lookup(generated_tokens[generated_dot].start);
                if observed != Some(expected) {
                    failures.push(format!("index{index}/{variant}/spread={spread}: dot {observed:?}, Native {expected:?}"));
                }
                let property = &generated_tokens[generated_dot + 1];
                for offset in property.start..property.end {
                    assert_eq!(
                        lookup(offset),
                        Some(expected_property),
                        "property interior index{index}/{variant}/spread={spread}"
                    );
                }
                products += 1;
            }
        }
    }
    assert_eq!(products, 16);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
