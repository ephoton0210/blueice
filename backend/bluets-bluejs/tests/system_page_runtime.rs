// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    parse, parse_with_debug_spans, BlueJsPageDebuggerExecutionState,
    BlueJsPageDebuggerNestedExecutionState, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsProgramV1,
    BlueJsSourceIdentity, Value,
};
use blueice_bluets::{CompilerOptions, MapLoader, ModuleKind, ModuleSource};

#[test]
fn system_class_declarations_keep_their_statement_boundary() {
    let id = "memory:///system-class.ts";
    for source in [
        "export const marker: number=0;class Handler {fail(): void {throw 'class';}}new Handler().fail();",
        "export class Handler {fail(): void {throw 'class';}}const result: number=1;",
        "export const marker: number=0;class Handler {}class Other {}new Other();",
        "export const Handler=class Named {static value(): number {return 1;}};const result: number=Handler.value();",
    ] {
        let compilation = blueice_bluets::compile(id,&MapLoader::from([ModuleSource::new(id,source)]),CompilerOptions {
            module_kind: ModuleKind::System,source_map: true,diagnostic_source_map: true,inline_sources: true,
            ..CompilerOptions::default()
        });
        assert!(!compilation.has_errors(),"{:?}",compilation.diagnostics);
        let artifact = compilation.output.unwrap().artifacts.remove(id).unwrap();
        parse(&artifact.javascript).unwrap_or_else(|error|panic!("{error:?}: {}",artifact.javascript));
        let positions=artifact.source_map.as_ref().unwrap().original_positions_at_exact(&artifact.javascript,&[(0,0)],100_000,16_000_000);
        let map=artifact.source_map.as_ref().unwrap();
        assert!(positions.is_ok(),"{source}: {positions:?}\n{}",serde_json::json!({"javascript":artifact.javascript,"mappings":map.mappings,"sources_content":map.sources_content}));
    }
}

#[test]
fn frozen_system_output_pauses_through_a_separately_installed_loader_once() {
    let module = "memory:///main.ts";
    let frozen = include_str!("../../bluets/tests/fixtures/k11_source_maps/system_wrapper.ts");
    let source = format!("{frozen}\ndeclare const console: {{ log(value: number): void }};\n");
    let compilation = blueice_bluets::compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source.clone())]),
        CompilerOptions {
            module_kind: ModuleKind::System,
            source_map: true,
            inline_sources: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let generated = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    let token_byte = generated.rfind("console.log(").unwrap();
    let prefix = &generated[..token_byte];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    assert!(map
        .original_position_at(generated, line, column, 200_000, 16 * 1024 * 1024)
        .unwrap()
        .is_some());

    let origin = BlueJsPageOrigin::new("https://example.test").unwrap();
    let mut runtime = BlueJsPageRuntime::default();
    runtime.open_realm(7, origin.clone()).unwrap();
    let loader_source = "var loaderCalls = 0; var consoleCalls = 0; var console = { log: function(value) { consoleCalls++; if (value !== 42) throw 7; } }; var System = { register: function(dependencies, declare) { loaderCalls++; var exports = {}; var body = declare(function(name, value) { exports[name] = value; return value; }, { id: 'main' }); body.execute(); if (exports.answer !== 42) throw 8; } };";
    let loader = runtime
        .install_program(
            7,
            &origin,
            BlueJsSourceIdentity::new("page:///system-loader.js", "sha256:system-loader").unwrap(),
            &BlueJsProgramV1::Script(parse(loader_source).unwrap()),
        )
        .unwrap();
    let emitted = runtime
        .install_program(
            7,
            &origin,
            BlueJsSourceIdentity::new("page:///emitted-system.js", "sha256:emitted-system")
                .unwrap(),
            &BlueJsProgramV1::Script(parse(generated).unwrap()),
        )
        .unwrap();
    let (_, statements, identifiers) = parse_with_debug_spans(generated).unwrap();
    assert!(identifiers
        .iter()
        .any(|span| span.name == "console" && span.start_byte == token_byte));
    let statement_index = statements
        .iter()
        .position(|span| span.start_byte <= token_byte && token_byte < span.end_byte)
        .unwrap();
    let point = runtime
        .program_registry()
        .get(emitted)
        .unwrap()
        .unique_identifier_safe_point(statement_index, "console")
        .expect("mapped console call has an exact nested instruction");
    assert_ne!(point.code_unit.ordinal(), 0);
    runtime.execute_program(7, loader).unwrap();
    let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
        .execute_program_until_foreign_intermediate_debugger_pause(7, emitted, loader, point)
        .unwrap()
    else {
        panic!("frozen emitted System execute body must pause");
    };
    let stack = runtime
        .debugger_foreign_intermediate_stack_snapshot(7, frame, loader, 3, 256)
        .unwrap();
    assert_eq!(
        stack
            .frames
            .iter()
            .map(|frame| frame.program_generation)
            .collect::<Vec<_>>(),
        vec![
            emitted.generation().as_u64(),
            loader.generation().as_u64(),
            emitted.generation().as_u64(),
        ]
    );
    assert!(runtime
        .debugger_stack_snapshot(7, emitted, Some(frame), 3, 256)
        .is_err());
    assert!(matches!(
        runtime.step_debugger_nested_instruction(frame),
        Ok(BlueJsPageDebuggerNestedExecutionState::Paused { .. })
    ));
    assert!(matches!(
        runtime.resume_debugger_nested_execution(frame),
        Ok(BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. })
    ));
    assert_eq!(
        runtime.resume_debugger_execution(7),
        Ok(BlueJsPageDebuggerExecutionState::Completed)
    );
    let probe = runtime
        .install_program(
            7,
            &origin,
            BlueJsSourceIdentity::new("page:///system-probe.js", "sha256:system-probe").unwrap(),
            &BlueJsProgramV1::Script(parse("loaderCalls * 10 + consoleCalls;").unwrap()),
        )
        .unwrap();
    assert_eq!(runtime.execute_program(7, probe), Ok(Value::Number(11.0)));
}
