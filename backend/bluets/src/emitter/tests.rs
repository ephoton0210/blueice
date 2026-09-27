// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{
    compile, CompilerLimits, CompilerOptions, MapLoader, ModuleSource, RuntimePolicy, SourceSpan,
    StrictRuntimeBoundary,
};

use super::source_line_column;

#[test]
fn erases_types_and_rewrites_typescript_module_specifiers() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "memory:///main.ts",
            "import { add } from './math.ts'; export const total: number = add(1, 2);",
        ),
        ModuleSource::new(
            "memory:///math.ts",
            "export function add(left: number, right: number): number { return left; }",
        ),
    ]);
    let output = compile("memory:///main.ts", &loader, CompilerOptions::default())
        .output
        .unwrap();
    let javascript = &output.artifacts["memory:///main.ts"].javascript;
    assert!(javascript.contains("'./math.js'"));
    assert!(javascript.contains("const total="));
    assert!(!javascript.contains(": number"));
}

#[test]
fn preserves_array_holes_while_erasing_the_annotation() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///holes.ts",
        "const values: number[] = [1,,3];",
    )]);
    let output = compile("memory:///holes.ts", &loader, CompilerOptions::default())
        .output
        .unwrap();
    let javascript = &output.artifacts["memory:///holes.ts"].javascript;
    assert!(javascript.contains("[1,,3]"), "{javascript}");
    assert!(!javascript.contains(": number[]"), "{javascript}");
}

#[test]
fn emits_source_map_and_public_declaration() {
    let loader = MapLoader::from([ModuleSource::new(
            "memory:///api.ts",
            "export interface User { id: string }\nexport function greeting(user: User): string { return 'hi'; }",
        )]);
    let output = compile(
        "memory:///api.ts",
        &loader,
        CompilerOptions {
            source_map: true,
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let artifact = &output.artifacts["memory:///api.ts"];
    assert!(artifact
        .source_map
        .as_ref()
        .unwrap()
        .to_json()
        .contains("\"version\":3"));
    assert!(artifact
        .declaration
        .as_ref()
        .unwrap()
        .contains("export interface User"));
}

#[test]
fn source_map_tracks_columns_across_erased_annotations() {
    let source = "export const label: string = 'value';";
    let loader = MapLoader::from([ModuleSource::new("memory:///columns.ts", source)]);
    let output = compile(
        "memory:///columns.ts",
        &loader,
        CompilerOptions {
            source_map: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let artifact = &output.artifacts["memory:///columns.ts"];
    let generated_equals = artifact.javascript.find('=').unwrap();
    let source_equals = source.find('=').unwrap();
    let expected_generated_column = artifact.javascript[..generated_equals]
        .encode_utf16()
        .count();
    let expected_source_column = source[..source_equals].encode_utf16().count();
    let mappings = decode_mappings(&artifact.source_map.as_ref().unwrap().mappings);
    assert!(mappings
        .iter()
        .any(|mapping| { *mapping == (0, expected_generated_column, 0, expected_source_column) }));
}

fn strict_boundary(
    module: &str,
    source: &str,
    function: &str,
    contract_id: &str,
    budget: usize,
) -> StrictRuntimeBoundary {
    let start = source.find("export function").unwrap();
    let end = start + source[start..].find('}').unwrap() + 1;
    StrictRuntimeBoundary {
        contract_id: contract_id.to_string(),
        function: function.to_string(),
        span: SourceSpan::new(module, start, end),
        max_string_bytes: budget,
        helper_version: "bluets-runtime-helper-v1".to_string(),
    }
}

#[test]
fn strict_calls_keep_ingress_and_egress_source_map_positions() {
    let module = "src/main.ts";
    let source =
        "export function join(left: string, right: string): string { return left + right; }";
    let loader = MapLoader::from([ModuleSource::new(module, source)]);
    let boundary = strict_boundary(module, source, "join", "join-v1", 32);
    let options = CompilerOptions {
        runtime_policy: RuntimePolicy::StrictRuntime,
        strict_runtime_boundaries: vec![boundary.clone()],
        source_map: true,
        ..CompilerOptions::default()
    };
    let result = compile(module, &loader, options.clone());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts[module];
    let record = artifact.strict_runtime.as_ref().unwrap();
    assert_eq!(record.boundaries[0].source_span, boundary.span);
    assert_eq!(record.boundaries[0].ingress.len(), 2);
    let mappings = decode_mappings(&artifact.source_map.as_ref().unwrap().mappings);
    for site in &record.boundaries[0].ingress {
        let (generated_line, _) = source_line_column(&artifact.javascript, site.generated_start);
        let (source_line, source_column) =
            source_line_column(source, source.find('{').unwrap() + 1);
        assert!(
            mappings
                .iter()
                .any(|mapping| { *mapping == (generated_line, 0, source_line, source_column) }),
            "missing ingress map for {site:?}: {mappings:?}"
        );
    }
    let egress = &record.boundaries[0].egress;
    let (generated_line, generated_column) =
        source_line_column(&artifact.javascript, egress.generated_start);
    let (source_line, source_column) = source_line_column(source, egress.source_span.start);
    assert!(
        mappings.iter().any(|mapping| {
            *mapping == (generated_line, generated_column, source_line, source_column)
        }),
        "missing egress map: {mappings:?}"
    );

    let checked = compile(
        module,
        &loader,
        CompilerOptions {
            source_map: true,
            limits: CompilerLimits {
                max_source_map_segments: 10,
                ..CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let bounded = compile(
        module,
        &loader,
        CompilerOptions {
            limits: CompilerLimits {
                max_source_map_segments: 10,
                ..CompilerLimits::default()
            },
            ..options
        },
    );
    assert!(bounded.output.is_none());
    assert!(bounded
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.code == crate::DiagnosticCode::ResourceLimit }));
}

#[test]
fn strict_helper_calls_survive_in_every_type_imported_module() {
    let main_id = "src/main.ts";
    let other_id = "src/nested/other.ts";
    let main = "import type { Marker } from './nested/other.ts';\nexport function echo(value: string): string { return value; }";
    let other = "export interface Marker { value: string }\nexport function more(value: string): string { return value + '!'; }";
    let loader = MapLoader::from([
        ModuleSource::new(main_id, main),
        ModuleSource::new(other_id, other),
    ]);
    let result = compile(
        main_id,
        &loader,
        CompilerOptions {
            runtime_policy: RuntimePolicy::StrictRuntime,
            strict_runtime_boundaries: vec![
                strict_boundary(main_id, main, "echo", "echo-v1", 16),
                strict_boundary(other_id, other, "more", "more-v1", 16),
            ],
            source_map: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let output = result.output.unwrap();
    assert_eq!(output.artifacts.len(), 2);
    for (module, expected_path) in [
        (main_id, "../bluets.runtime-helper.v1.mjs"),
        (other_id, "../../bluets.runtime-helper.v1.mjs"),
    ] {
        let artifact = &output.artifacts[module];
        let record = artifact.strict_runtime.as_ref().unwrap();
        assert!(record.helper_import.expected_text.contains(expected_path));
        assert_eq!(record.boundaries.len(), 1);
        assert_eq!(record.boundaries[0].ingress.len(), 1);
        for site in record.boundaries[0]
            .ingress
            .iter()
            .chain(std::iter::once(&record.boundaries[0].egress))
        {
            assert!(artifact.javascript[site.generated_start..].starts_with(&site.expected_text));
        }
        assert!(artifact.source_map.is_some());
    }
    assert!(!output.artifacts[main_id].javascript.contains("import type"));
}

#[test]
fn unsupported_typed_strict_crossing_never_reaches_emission() {
    let module = "src/main.ts";
    let source = "export function wrong(value: number): string { return 'ok'; }";
    let loader = MapLoader::from([ModuleSource::new(module, source)]);
    let result = compile(
        module,
        &loader,
        CompilerOptions {
            runtime_policy: RuntimePolicy::StrictRuntime,
            strict_runtime_boundaries: vec![strict_boundary(
                module, source, "wrong", "wrong-v1", 16,
            )],
            ..CompilerOptions::default()
        },
    );
    assert!(result.output.is_none());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == crate::DiagnosticCode::InvalidContract
            && diagnostic.span == SourceSpan::new(module, 0, source.len())
    }));
}

#[test]
fn provenance_uses_utf16_columns_and_normalizes_crlf_to_one_line_break() {
    let source = "const 值 = 1;\r\nconst next = 2;";
    let first_value = source.find('1').unwrap();
    let second_line = source.rfind("next").unwrap();
    assert_eq!(
        source_line_column(source, first_value),
        (0, "const 值 = ".encode_utf16().count())
    );
    assert_eq!(source_line_column(source, second_line), (1, 6));
}

#[test]
fn emits_an_erasable_generic_function_without_a_javascript_type_parameter() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///generic.ts",
        "export function identity<T>(value: T): T { return value; }",
    )]);
    let output = compile("memory:///generic.ts", &loader, CompilerOptions::default())
        .output
        .unwrap();
    let javascript = &output.artifacts["memory:///generic.ts"].javascript;
    assert!(javascript.contains("function identity(value)"));
    assert!(!javascript.contains("<T>"));
}

#[test]
fn erases_explicit_generic_arguments_from_direct_calls() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///generic-call.ts",
        "function identity<T extends string>(value: T): T { return value; }\n\
             console.log(identity<string>('Ada'));",
    )]);
    let output = compile(
        "memory:///generic-call.ts",
        &loader,
        CompilerOptions::default(),
    )
    .output
    .unwrap();
    let javascript = &output.artifacts["memory:///generic-call.ts"].javascript;
    assert!(javascript.contains("identity('Ada')"), "{javascript}");
    assert!(!javascript.contains("identity<string>"), "{javascript}");
}

#[test]
fn retains_generic_constraints_and_defaults_in_declaration_output_only() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///generic.ts",
        "export interface Box<T extends string = string> { value: T }\n\
             export function echo<T extends string = string>(value?: T): string { return ''; }",
    )]);
    let result = compile(
        "memory:///generic.ts",
        &loader,
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts["memory:///generic.ts"];
    assert!(
        artifact.javascript.contains("function echo(value)"),
        "{}",
        artifact.javascript
    );
    assert!(!artifact.javascript.contains("extends string"));
    assert_eq!(
        artifact.declaration.as_deref(),
        Some(
            "export interface Box<T extends string = string> {\n  value: T;\n}\n\
                 export declare function echo<T extends string = string>(value?: T): string;\n"
        )
    );
}

#[test]
fn retains_readonly_fields_in_declaration_output() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///event.ts",
        "export interface Event { readonly type: 'click'; mutable: string; }\n\
             export type Detail = { readonly currentTarget: string; mutable: string };",
    )]);
    let result = compile(
        "memory:///event.ts",
        &loader,
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let artifact = &result.output.unwrap().artifacts["memory:///event.ts"];
    let declaration = artifact.declaration.as_deref().unwrap();
    assert!(
        declaration.contains("readonly type: 'click';"),
        "{declaration}"
    );
    assert!(
        declaration.contains("{ readonly currentTarget: string; mutable: string }"),
        "{declaration}"
    );
    assert!(!artifact.javascript.contains("readonly"));
}

#[test]
fn retains_interface_heritage_in_declaration_output_only() {
    let loader = MapLoader::from([ModuleSource::new(
            "memory:///inheritance.ts",
            "export interface Envelope<T> { payload: T }\n\
             export interface Tagged { tag: string }\n\
             export interface Labeled<T extends string = string> extends Envelope<T>, Tagged { label: T }",
        )]);
    let output = compile(
        "memory:///inheritance.ts",
        &loader,
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let artifact = &output.artifacts["memory:///inheritance.ts"];
    assert!(!artifact.javascript.contains("interface"));
    assert_eq!(
            artifact.declaration.as_deref(),
            Some(
                "export interface Envelope<T> {\n  payload: T;\n}\n\
                 export interface Tagged {\n  tag: string;\n}\n\
                 export interface Labeled<T extends string = string> extends Envelope<T>, Tagged {\n  label: T;\n}\n"
            )
        );
}

#[test]
fn erases_interface_methods_but_retains_their_exact_public_signature() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///document.ts",
        "export interface Document { getElementById(id: string): Element | null; }\n\
             export interface Element { textContent: string; }",
    )]);
    let output = compile(
        "memory:///document.ts",
        &loader,
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let artifact = &output.artifacts["memory:///document.ts"];
    assert!(!artifact.javascript.contains("getElementById"));
    assert_eq!(
        artifact.declaration.as_deref(),
        Some(
            "export interface Document {\n  getElementById(id: string): Element | null;\n}\n\
                 export interface Element {\n  textContent: string;\n}\n"
        )
    );
}

#[test]
fn erases_overload_signatures_and_retains_them_in_declarations() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///overload.ts",
        "export function describe(value: string): string;\n\
             export function describe(value: number): number;\n\
             export function describe(value: string | number): string | number { return value; }",
    )]);
    let output = compile(
        "memory:///overload.ts",
        &loader,
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let artifact = &output.artifacts["memory:///overload.ts"];
    assert_eq!(artifact.javascript.matches("function describe").count(), 1);
    assert_eq!(
        artifact.declaration.as_deref(),
        Some(
            "export declare function describe(value: string): string;\n\
                 export declare function describe(value: number): number;\n"
        )
    );
}

#[test]
fn erases_optional_parameter_markers_but_preserves_default_initializers() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///optional.ts",
        "export function count(value: number = 2, multiplier?: number): number { return value; }",
    )]);
    let output = compile("memory:///optional.ts", &loader, CompilerOptions::default())
        .output
        .unwrap();
    let javascript = &output.artifacts["memory:///optional.ts"].javascript;
    assert!(javascript.contains("function count(value= 2, multiplier)"));
    assert!(!javascript.contains("?:"));
    assert!(!javascript.contains("multiplier?"));
}

#[test]
fn rejects_source_maps_that_exceed_the_segment_budget() {
    let loader = MapLoader::from([ModuleSource::new(
        "memory:///limited.ts",
        "const label: string = 'BlueIce';\n",
    )]);
    let result = compile(
        "memory:///limited.ts",
        &loader,
        CompilerOptions {
            source_map: true,
            limits: crate::CompilerLimits {
                max_source_map_segments: 1,
                ..crate::CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == crate::DiagnosticCode::ResourceLimit
            && diagnostic.message.contains("segment limit")
    }));
    assert!(result.output.is_none());
}

#[test]
fn preserves_a_type_only_reexport_in_declarations_but_not_javascript() {
    let loader = MapLoader::from([
        ModuleSource::new(
            "memory:///api.ts",
            "export type { User } from './model.ts';",
        ),
        ModuleSource::new("memory:///model.ts", "export interface User { id: string }"),
    ]);
    let output = compile(
        "memory:///api.ts",
        &loader,
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    )
    .output
    .unwrap();
    let artifact = &output.artifacts["memory:///api.ts"];
    assert!(artifact.javascript.trim().is_empty());
    assert_eq!(
        artifact.declaration.as_deref(),
        Some("export type { User } from \"./model.ts\";\n")
    );
}

fn decode_mappings(value: &str) -> Vec<(usize, usize, usize, usize)> {
    let mut mappings = Vec::new();
    let mut source_index = 0i64;
    let mut source_line = 0i64;
    let mut source_column = 0i64;
    for (generated_line, line) in value.split(';').enumerate() {
        let mut generated_column = 0i64;
        for segment in line.split(',').filter(|segment| !segment.is_empty()) {
            let values = decode_vlq_fields(segment);
            assert_eq!(values.len(), 4);
            generated_column += values[0];
            source_index += values[1];
            source_line += values[2];
            source_column += values[3];
            assert_eq!(source_index, 0);
            mappings.push((
                generated_line,
                generated_column as usize,
                source_line as usize,
                source_column as usize,
            ));
        }
    }
    mappings
}

fn decode_vlq_fields(value: &str) -> Vec<i64> {
    let bytes = value.as_bytes();
    let mut fields = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let digit = base64_value(bytes[index]);
            index += 1;
            value |= u64::from(digit & 0b1_1111) << shift;
            shift += 5;
            if digit & 0b10_0000 == 0 {
                break;
            }
        }
        let negative = value & 1 == 1;
        let value = (value >> 1) as i64;
        fields.push(if negative { -value } else { value });
    }
    fields
}

fn base64_value(value: u8) -> u8 {
    match value {
        b'A'..=b'Z' => value - b'A',
        b'a'..=b'z' => value - b'a' + 26,
        b'0'..=b'9' => value - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("invalid base64 VLQ digit"),
    }
}
