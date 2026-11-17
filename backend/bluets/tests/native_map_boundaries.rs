// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native factory events and generated prologues retain bounded lookup semantics.

use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, JavaScriptLayout, MapLoader, ModuleKind, ModuleSource,
    NewLine, SourceMap, SourceMapLookupError, SourceMapOriginalPosition,
};

#[path = "oracle_support/gd2_maps.rs"]
mod maps;
use maps as decoded;

#[allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[allow(dead_code)]
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[allow(dead_code)]
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

fn artifact(source: &str, target: EcmaTarget, newline: NewLine) -> (String, SourceMap) {
    let id = "memory:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(id, source)]),
        CompilerOptions {
            target,
            new_line: newline,
            module_kind: ModuleKind::CommonJs,
            source_map: true,
            inline_sources: true,
            javascript_layout: JavaScriptLayout::TypeScript,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let artifact = &result.output.as_ref().unwrap().artifacts[id];
    (
        artifact.javascript.clone(),
        artifact.source_map.clone().unwrap(),
    )
}

#[test]
fn native_factory_coincidence_is_last_wins_and_counts_both_events() {
    let (code, map) = artifact(
        "class Item { value: number = 1; }",
        EcmaTarget::Es5,
        NewLine::Lf,
    );
    let decoded = maps::decoded(&map.to_json());
    let segments = decoded["segments"].as_array().unwrap();
    let pair = segments
        .windows(2)
        .find(|pair| pair[0][0] == pair[1][0] && pair[0][1] == pair[1][1])
        .expect("the pinned Native factory retains distinct coincident source events");
    let last = &pair[1];
    let line = last[0].as_u64().unwrap() as usize;
    let column = last[1].as_u64().unwrap() as usize;
    assert_eq!(
        map.original_position_at(&code, line, column, segments.len(), 1_000_000),
        Ok(Some(SourceMapOriginalPosition {
            source_index: last[2].as_u64().unwrap() as usize,
            line: last[3].as_u64().unwrap() as usize,
            column_utf16: last[4].as_u64().unwrap() as usize,
            name_index: None,
        }))
    );
    assert_eq!(
        map.original_position_at(&code, line, column, segments.len() - 1, 1_000_000),
        Err(SourceMapLookupError::SegmentLimit)
    );
}

#[test]
fn redundant_or_hidden_source_coincidences_remain_invalid() {
    let (code, map) = artifact("class Item {}", EcmaTarget::Es5, NewLine::Lf);
    for encoded in ["AAAA,AAAA", "AAAA,A", "A,AAAA"] {
        let mut changed = map.clone();
        changed.mappings = encoded.into();
        assert_eq!(
            changed.original_position_at(&code, 0, 0, 100, 1_000_000),
            Err(SourceMapLookupError::MalformedMap),
            "{encoded}"
        );
    }
}

#[test]
fn generated_async_prologue_starts_unmapped_at_the_entry_line_break() {
    let source =
        "async function read(n: number): Promise<number> { return await Promise.resolve(n); }";
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, map) = artifact(source, EcmaTarget::Es2015, newline);
        let guard = code.find("if (new.target)").unwrap();
        let start = code[..guard].rfind('{').unwrap() + 1;
        assert!(code[start..guard].chars().all(char::is_whitespace));
        for offset in start..guard {
            let prefix = &code[..offset];
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
            let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
            assert_eq!(
                map.original_position_at(&code, line, column, 100_000, 1_000_000),
                Ok(None),
                "generated guard trivia at {offset}: {code}"
            );
        }
    }
}

#[test]
fn namespace_function_exports_keep_the_native_statement_line_break() {
    let source = "export namespace Outer { export interface Shape {value:number;} export namespace Inner { export const value:number=42; export function identity<T>(value:T):T{return value;} } }\nexport const result:number = Outer.Inner.identity(42);\n";
    for newline in [NewLine::Lf, NewLine::CrLf] {
        let (code, map) = artifact(source, EcmaTarget::Es2022, newline);
        assert!(
            map.original_position_at(&code, 0, 0, 100_000, 1_000_000)
                .is_ok(),
            "namespace export map must validate in full: {code}"
        );
        let expected = match newline {
            NewLine::Lf => "return value; }\n        Inner.identity = identity;",
            NewLine::CrLf => "return value; }\r\n        Inner.identity = identity;",
        };
        assert!(
            code.contains(expected),
            "Native namespace declaration/export boundary: {code}"
        );
    }
}

#[test]
fn all_eleven_original_namespace_products_validate_twice() {
    let temporary = cli::Temporary::new("namespace-map-boundaries");
    let mut observations = Vec::new();
    for index in [
        7620, 7621, 7622, 7623, 7628, 7629, 7630, 7631, 8299, 8300, 8301,
    ] {
        let specification = &inputs::input()["cases"][index];
        let reference = &inputs::reference()["cases"][index];
        assert!(reference["id"]
            .as_str()
            .unwrap()
            .contains("/namespace-nested-"));
        let row = inputs::resolve(specification);
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        let failures = declarations::compare(specification, &row, reference, &actual);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        observations.push(serde_json::json!({"index":index,"id":reference["id"],
            "repeatBuilds":2,"outputs":actual.outputs}));
    }
    if let Some(path) = std::env::var_os("BLUEICE_NAMESPACE_MAP_OBSERVATIONS") {
        std::fs::write(path, serde_json::to_vec(&observations).unwrap()).unwrap();
    }
}
