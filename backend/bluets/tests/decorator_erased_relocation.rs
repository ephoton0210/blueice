// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Atomic asserted decorator names move only their retained runtime operand.
use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};
use std::process::Command;

#[test]
fn frozen_erased_decorator_remains_executable_and_keeps_its_identifier_owner() {
    let source = include_str!("fixtures/k11_source_maps/decorated_erased_assertion.ts");
    for source_map in [false, true] {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                source_map,
                inline_sources: source_map,
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let array = code
            .split("_method_decorators = [")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap();
        assert_eq!(
            array.trim(),
            "decoSite",
            "only the parsed runtime operand may be relocated"
        );
        if let Some(map) = &artifact.source_map {
            assert_eq!(map.sources_content, vec![source.to_string()]);
            let position = |text: &str, offset: usize| {
                let before = &text[..offset];
                (
                    before.bytes().filter(|byte| *byte == b'\n').count(),
                    before.rsplit('\n').next().unwrap().encode_utf16().count(),
                )
            };
            let owner = |offset| {
                let (line, column) = position(code, offset);
                map.original_position_at(code, line, column, 200_000, 16 * 1024 * 1024)
                    .unwrap()
                    .map(|point| (point.source_index, point.line, point.column_utf16))
            };
            let marker = "_method_decorators = [";
            let generated = code.find(marker).unwrap() + marker.len();
            let original = source.find("@(decoSite").unwrap() + 2;
            let expected = position(source, original);
            for offset in generated..generated + "decoSite".len() {
                assert_eq!(
                    owner(offset),
                    Some((0, expected.0, expected.1)),
                    "complete relocated identifier source role"
                );
            }
            let end = source.find("@(decoSite as any)").unwrap() + "@(decoSite as any)".len();
            let expected = position(source, end);
            assert_eq!(
                owner(generated + "decoSite".len()),
                Some((0, expected.0, expected.1)),
                "array boundary retains the original decorator end"
            );
        }
        let output = Command::new("node")
            .arg("--input-type=module")
            .arg("-e")
            .arg(code)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"42\n");
    }
}
