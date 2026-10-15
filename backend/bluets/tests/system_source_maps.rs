// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source locations survive moving declarations into a System factory.

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleKind, ModuleSource};

fn system_javascript(body: &str) -> String {
    let id = "memory:///system-await.ts";
    let source = format!("export const marker: number=0;{body}");
    let compilation = compile(
        id,
        &MapLoader::from([ModuleSource::new(id, &source)]),
        CompilerOptions {
            module_kind: ModuleKind::System,
            source_map: true,
            diagnostic_source_map: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        !compilation.has_errors(),
        "{source}: {:?}",
        compilation.diagnostics
    );
    compilation
        .output
        .unwrap()
        .artifacts
        .remove(id)
        .unwrap()
        .javascript
}

#[test]
fn system_module_awaits_have_an_async_execute_boundary() {
    for source in [
        "await 0;",
        "{await 0;}",
        "const result: any=Number(await 0);",
        "const result: string=`value:${await 0}`;",
    ] {
        let javascript = system_javascript(source);
        assert!(
            javascript.contains("execute: async function()"),
            "{source}: module await must execute inside an async function"
        );
    }
}

#[test]
fn nested_awaits_and_await_text_keep_system_execute_synchronous() {
    for source in [
        "async function nested(): Promise<number> {return await 0;}",
        "const nested: any=async()=>await 0;",
        "const nested: any={async run(){await 0;}};",
        "class Nested {run(){const nested: any=async()=>await 0;}}",
        "const value: any={await:1};",
        "const value: any=/await/;",
        "const value: string=`await ${1}`;",
        "const value: string=`nested:${async()=>await 0}`;",
    ] {
        let javascript = system_javascript(source);
        assert!(
            javascript.contains("execute: function()"),
            "{source}: nested code or text must not change the execute result to a Promise"
        );
    }
}

#[test]
fn system_module_await_refuses_targets_below_es2017_at_the_original_token() {
    use blueice_bluets::EcmaTarget;
    let id = "memory:///system-target.ts";
    let source = "export const marker: number=0;\nawait 0;";
    for target in [EcmaTarget::Es5, EcmaTarget::Es2015] {
        let compilation = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                module_kind: ModuleKind::System,
                target,
                ..CompilerOptions::default()
            },
        );
        assert!(compilation.has_errors());
        assert!(compilation.output.is_none());
        assert!(
            compilation
                .diagnostics
                .iter()
                .any(|error| error.span.module == id
                    && &source[error.span.start..error.span.end] == "await"),
            "{:?}",
            compilation.diagnostics
        );
    }
}

fn source_lines(mappings: &str) -> Vec<Vec<i64>> {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut source_line = 0;
    mappings
        .split(';')
        .map(|line| {
            line.split(',')
                .filter(|segment| !segment.is_empty())
                .filter_map(|segment| {
                    let mut fields = Vec::new();
                    let (mut shift, mut accumulator) = (0, 0i64);
                    for character in segment.chars() {
                        let digit = ALPHABET.find(character).unwrap() as i64;
                        accumulator |= (digit & 31) << shift;
                        if digit & 32 == 0 {
                            let magnitude = accumulator >> 1;
                            fields.push(if accumulator & 1 == 1 {
                                -magnitude
                            } else {
                                magnitude
                            });
                            shift = 0;
                            accumulator = 0;
                        } else {
                            shift += 5;
                        }
                    }
                    if fields.len() < 4 {
                        return None;
                    }
                    source_line += fields[2];
                    Some(source_line)
                })
                .collect()
        })
        .collect()
}

#[test]
fn hoisted_functions_and_execute_body_keep_original_source_lines() {
    const ID: &str = "memory:///main.ts";
    for newline in ["\n", "\r\n"] {
        let source = [
            "export let answer: number = 20;",
            "export function bump(): void {",
            "    const marker: string = '😀';",
            "    answer += marker.length + 20;",
            "}",
            "export const result: number = answer;",
            "",
        ]
        .join(newline);
        let loader = MapLoader::from([ModuleSource::new(ID, source)]);
        let compilation = compile(
            ID,
            &loader,
            CompilerOptions {
                module_kind: ModuleKind::System,
                source_map: true,
                ..CompilerOptions::default()
            },
        );
        assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);
        let artifact = &compilation.output.as_ref().unwrap().artifacts[ID];
        assert!(artifact.javascript.contains("'use strict';"));
        assert!(
            !artifact.javascript.contains("\"use strict\";"),
            "System must not retain a second CommonJS strictness prelude"
        );
        assert!(
            !artifact
                .javascript
                .contains("_define(exports, \"__esModule\""),
            "System must not publish CommonJS-only module metadata"
        );
        let mapped = source_lines(&artifact.source_map.as_ref().unwrap().mappings);
        for (text, expected) in [
            ("function bump(", 1),
            ("const marker", 2),
            ("marker.length + 20", 3),
            ("exports.result", 5),
        ] {
            let line = artifact
                .javascript
                .lines()
                .position(|line| line.contains(text) && !line.contains("void 0"))
                .unwrap();
            assert!(mapped.get(line).is_some_and(|values| values.contains(&expected)),
                "{newline:?}: generated line {line} `{text}` has mappings {:?}, expected source line {expected}", mapped.get(line));
        }
    }
}
