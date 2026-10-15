// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile,
    diagnostics::{
        source_positions, DiagnosticSourceLocation, DiagnosticSourceMap,
        DiagnosticSourceMapError as Error, DiagnosticSourceRange, MAX_DIAGNOSTIC_SOURCE_MAP_BYTES,
        MAX_DIAGNOSTIC_SOURCE_NAME_BYTES,
    },
    parse, BlueJsPageOrigin, BlueJsPageRuntime, BlueJsPageRuntimeConfig, BlueJsPageRuntimeError,
    BlueJsProgramRegistry, BlueJsProgramV1, BlueJsSourceIdentity, Opcode, Value, Vm,
};

fn identity(hash: &str) -> BlueJsSourceIdentity {
    BlueJsSourceIdentity::new("memory:///position.js", hash).unwrap()
}

fn location(line: u32) -> DiagnosticSourceLocation {
    DiagnosticSourceLocation {
        filename: "memory:///position.js".into(),
        line,
        column: 1,
    }
}

fn range(unit: u32, start: u32, end: u32) -> DiagnosticSourceRange {
    DiagnosticSourceRange {
        code_unit_ordinal: unit,
        start,
        end,
        location: location(2),
    }
}

#[test]
fn source_maps_follow_owned_code_without_filling_unmapped_gaps() {
    let mut registry = BlueJsProgramRegistry::default();
    let program = BlueJsProgramV1::Script(parse("\nthrow 'probe';").unwrap());
    let handle = registry.install(identity("original"), &program).unwrap();
    let before = registry.get(handle).unwrap().bytecode().clone();
    let instruction = before
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Throw)
        .unwrap();
    let start = instruction.offset as u32;
    let end = start + 1;
    let map = DiagnosticSourceMap::new(identity("original"), vec![range(0, start, end)]).unwrap();
    let bytes = map.payload_bytes();
    assert_eq!(
        registry.bind_diagnostic_sources(handle, map).unwrap(),
        bytes
    );
    let owned = registry.get(handle).unwrap().bytecode().clone();
    assert_eq!(owned.diagnostic_source_location(start), Some(&location(2)));
    assert_eq!(owned.diagnostic_source_location(end), None);
    assert_eq!(owned.diagnostic_source_location(0), None);
    assert_eq!(before.diagnostic_source_location(start), None);
    assert_eq!(owned.diagnostic_source_map_bytes(), bytes);
    registry.invalidate(handle);
    assert_eq!(owned.diagnostic_source_location(start), Some(&location(2)));
    let map = DiagnosticSourceMap::new(identity("original"), vec![range(0, start, end)]).unwrap();
    assert_eq!(
        registry.bind_diagnostic_sources(handle, map),
        Err(Error::UnknownProgram)
    );
}

#[test]
fn binding_rejects_wrong_sources_units_and_operand_offsets_atomically() {
    let mut registry = BlueJsProgramRegistry::default();
    let program = BlueJsProgramV1::Script(parse("throw 7;").unwrap());
    let handle = registry.install(identity("original"), &program).unwrap();
    let code = registry.get(handle).unwrap().bytecode();
    let instruction = code
        .instructions()
        .find(|instruction| instruction.opcode.width() > 1)
        .unwrap();
    let operand = instruction.offset as u32 + 1;
    let end = (instruction.offset + instruction.opcode.width()) as u32;
    for (hash, row, expected) in [
        ("different", range(0, 0, end), Error::SourceMismatch),
        ("original", range(999, 0, end), Error::UnknownCodeUnit),
        (
            "original",
            range(0, operand, end),
            Error::InvalidInstructionBoundary,
        ),
        (
            "original",
            range(0, 0, operand),
            Error::InvalidInstructionBoundary,
        ),
    ] {
        let map = DiagnosticSourceMap::new(identity(hash), vec![row]).unwrap();
        assert_eq!(registry.bind_diagnostic_sources(handle, map), Err(expected));
        assert_eq!(
            registry
                .get(handle)
                .unwrap()
                .bytecode()
                .diagnostic_source_map_bytes(),
            0
        );
    }
    let map = DiagnosticSourceMap::new(identity("original"), vec![range(0, 0, end)]).unwrap();
    registry.bind_diagnostic_sources(handle, map).unwrap();
    let map = DiagnosticSourceMap::new(identity("original"), vec![range(0, 0, end)]).unwrap();
    assert_eq!(
        registry.bind_diagnostic_sources(handle, map),
        Err(Error::AlreadyBound)
    );
}

#[test]
fn source_maps_reject_unavailable_coordinates_overlaps_and_excessive_names() {
    let mut zero = range(0, 0, 1);
    zero.location.line = 0;
    assert_eq!(
        DiagnosticSourceMap::new(identity("a"), vec![zero]).unwrap_err(),
        Error::InvalidLocation
    );
    let mut large = range(0, 0, 1);
    large.location.filename = "x".repeat(MAX_DIAGNOSTIC_SOURCE_NAME_BYTES + 1);
    assert_eq!(
        DiagnosticSourceMap::new(identity("a"), vec![large]).unwrap_err(),
        Error::InvalidLocation
    );
    assert_eq!(
        DiagnosticSourceMap::new(identity("a"), vec![range(0, 2, 2)]).unwrap_err(),
        Error::InvalidRange
    );
    assert_eq!(
        DiagnosticSourceMap::new(identity("a"), vec![range(0, 0, 3), range(0, 2, 4)]).unwrap_err(),
        Error::OverlappingRanges
    );
    // Half-open adjacency and distinct code units do not overlap.
    DiagnosticSourceMap::new(
        identity("a"),
        vec![range(1, 0, 3), range(0, 2, 4), range(0, 0, 2)],
    )
    .unwrap();
}

#[test]
fn source_identity_spare_capacity_participates_in_the_map_budget() {
    let mut name = String::with_capacity(MAX_DIAGNOSTIC_SOURCE_MAP_BYTES);
    name.push_str("memory:///short.js");
    let source = BlueJsSourceIdentity::new(name, "original").unwrap();
    assert_eq!(
        DiagnosticSourceMap::new(source, Vec::new()).unwrap_err(),
        Error::LimitExceeded
    );
}

#[test]
fn nested_interpreters_restore_the_callers_mapped_reporting_context() {
    let mut vm = page();
    vm.execute_script(&compile(&parse("var seen=[]; onerror=function(message,filename,line,column,error){seen.push(filename+':'+line+':'+column);return true;};").unwrap()).unwrap()).unwrap();
    let mut registry = BlueJsProgramRegistry::default();
    let program = BlueJsProgramV1::Script(
        parse("function peer(){reportError('inner');} peer(); reportError('outer');").unwrap(),
    );
    let handle = registry.install(identity("original"), &program).unwrap();
    let code = registry.get(handle).unwrap().bytecode();
    let child = code
        .child_code_units()
        .next()
        .unwrap()
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Call)
        .unwrap();
    let root = code
        .instructions()
        .filter(|instruction| instruction.opcode == Opcode::Call)
        .last()
        .unwrap();
    let mut inner = range(
        1,
        child.offset as u32,
        (child.offset + child.opcode.width()) as u32,
    );
    inner.location = location(7);
    let mut outer = range(
        0,
        root.offset as u32,
        (root.offset + root.opcode.width()) as u32,
    );
    outer.location = location(42);
    registry
        .bind_diagnostic_sources(
            handle,
            DiagnosticSourceMap::new(identity("original"), vec![outer, inner]).unwrap(),
        )
        .unwrap();
    vm.execute_script(registry.get(handle).unwrap().bytecode())
        .unwrap();
    assert_eq!(
        vm.execute_script(&compile(&parse("seen.join('|')").unwrap()).unwrap())
            .unwrap(),
        Value::String("memory:///position.js:7:1|memory:///position.js:42:1".into())
    );
}

fn page() -> Vm {
    let mut vm = Vm::default();
    vm.install_web_file_api().unwrap();
    vm.install_host_console().unwrap();
    vm.install_window_aliases().unwrap();
    vm.install_host_error_reporting().unwrap();
    vm.execute_script(&compile(&parse("var observed='waiting'; onerror=function(message,filename,line,column,error){observed=filename+':'+line+':'+column;return true;};").unwrap()).unwrap()).unwrap();
    vm
}

fn observed(vm: &mut Vm) -> Value {
    vm.execute_script(&compile(&parse("observed").unwrap()).unwrap())
        .unwrap()
}

#[test]
fn mapped_throw_positions_survive_error_handler_execution() {
    let mut vm = page();
    let mut registry = BlueJsProgramRegistry::default();
    let program = BlueJsProgramV1::Script(parse("\nthrow 'probe';").unwrap());
    let handle = registry.install(identity("original"), &program).unwrap();
    let point = registry
        .get(handle)
        .unwrap()
        .bytecode()
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Throw)
        .unwrap()
        .offset as u32;
    registry
        .bind_diagnostic_sources(
            handle,
            DiagnosticSourceMap::new(identity("original"), vec![range(0, point, point + 1)])
                .unwrap(),
        )
        .unwrap();
    assert!(vm
        .execute_script(registry.get(handle).unwrap().bytecode())
        .is_err());
    assert_eq!(
        observed(&mut vm),
        Value::String("memory:///position.js:2:1".into())
    );
    assert!(vm.drain_host_console().unwrap().records.is_empty());
}

#[test]
fn explicit_reports_use_the_active_call_position_and_leave_unknown_code_unmapped() {
    let mut vm = page();
    let mut registry = BlueJsProgramRegistry::default();
    let program = BlueJsProgramV1::Script(parse("reportError('probe');").unwrap());
    let handle = registry.install(identity("original"), &program).unwrap();
    let instruction = registry
        .get(handle)
        .unwrap()
        .bytecode()
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Call)
        .unwrap();
    let point = instruction.offset as u32;
    let end = point + instruction.opcode.width() as u32;
    registry
        .bind_diagnostic_sources(
            handle,
            DiagnosticSourceMap::new(identity("original"), vec![range(0, point, end)]).unwrap(),
        )
        .unwrap();
    vm.execute_script(registry.get(handle).unwrap().bytecode())
        .unwrap();
    assert_eq!(
        observed(&mut vm),
        Value::String("memory:///position.js:2:1".into())
    );
    vm.execute_script(&compile(&parse("reportError('unknown');").unwrap()).unwrap())
        .unwrap();
    assert_eq!(observed(&mut vm), Value::String(":0:0".into()));
}

#[test]
fn a_throw_from_unmapped_dynamic_code_does_not_inherit_the_callers_position() {
    for source in [
        "eval(\"throw 'unknown';\");",
        "Function(\"throw 'unknown';\")();",
    ] {
        let mut vm = page();
        let mut registry = BlueJsProgramRegistry::default();
        let program = BlueJsProgramV1::Script(parse(source).unwrap());
        let handle = registry.install(identity("original"), &program).unwrap();
        let (start, end) = registry
            .get(handle)
            .unwrap()
            .bytecode()
            .root_statement_ranges()[0]
            .unwrap();
        registry
            .bind_diagnostic_sources(
                handle,
                DiagnosticSourceMap::new(identity("original"), vec![range(0, start, end)]).unwrap(),
            )
            .unwrap();
        assert!(vm
            .execute_script(registry.get(handle).unwrap().bytecode())
            .is_err());
        assert_eq!(observed(&mut vm), Value::String(":0:0".into()), "{source}");
    }
}

#[test]
fn original_coordinates_count_utf16_and_all_javascript_line_terminators() {
    let source = "α\r\n/* 🚀 */ throw 1;\u{2028}throw 2;\u{2029}throw 3;";
    let first = source.find("throw 1").unwrap();
    let second = source.find("throw 2").unwrap();
    let third = source.find("throw 3").unwrap();
    let invalid = source.find('🚀').unwrap() + 1;
    assert_eq!(
        source_positions(
            source,
            &[
                third,
                first,
                second,
                invalid,
                source.len() + 1,
                0,
                source.len(),
                first
            ]
        ),
        vec![
            Some((4, 1)),
            Some((2, 10)),
            Some((3, 1)),
            None,
            None,
            Some((1, 1)),
            Some((4, 9)),
            Some((2, 10))
        ]
    );
}

fn root_map(
    registry: &BlueJsProgramRegistry,
    handle: blueice_bluejs::BlueJsProgramHandle,
) -> Result<DiagnosticSourceMap, BlueJsPageRuntimeError> {
    let compiled = registry.get(handle).unwrap();
    let code = compiled.bytecode();
    let instruction = code
        .instructions()
        .find(|instruction| instruction.opcode == Opcode::Throw)
        .unwrap();
    DiagnosticSourceMap::new(
        compiled.source().clone(),
        vec![range(
            0,
            instruction.offset as u32,
            instruction.offset as u32 + 1,
        )],
    )
    .map_err(BlueJsPageRuntimeError::DiagnosticSourceMap)
}

#[test]
fn parser_and_compiler_bind_each_nested_throw_to_its_own_statement() {
    for source in [
        "\nif (true) {\n  throw 'block';\n}",
        "\nfunction fail() {\n  throw 'function';\n}\nfail();",
        "var pair=[function(){throw 7;},function(){throw 7;}];",
        "if(true) function again(){throw 7;} else { function again(){throw 7;} } again();",
        "label: function again(){throw 7;}; again();",
        "'use\\u0020strict'; function again(){throw 7;} again();",
    ] {
        let (parsed, spans) = blueice_bluejs::parse_with_diagnostic_spans(source).unwrap();
        let mut registry = BlueJsProgramRegistry::default();
        let handle = registry
            .install_for_diagnostics(identity("original"), &BlueJsProgramV1::Script(parsed))
            .unwrap();
        let compiled = registry.get(handle).unwrap();
        assert_eq!(spans.len(), compiled.ast_nodes().len(), "{source}");
        fn collect(
            code: &blueice_bluejs::Bytecode,
            spans: &[Option<blueice_bluejs::ParsedStatementSpan>],
            found: &mut Vec<usize>,
        ) {
            for instruction in code
                .instructions()
                .filter(|instruction| instruction.opcode == Opcode::Throw)
            {
                let owner = code
                    .diagnostic_instruction_owners()
                    .iter()
                    .find(|owner| owner.bytecode_offset == instruction.offset as u32)
                    .unwrap();
                found.push(
                    spans[owner.ast_node_ordinal.unwrap() as usize]
                        .unwrap()
                        .start_byte,
                );
            }
            for child in code.child_code_units() {
                collect(child, spans, found);
            }
        }
        let mut starts = Vec::new();
        collect(compiled.bytecode(), &spans, &mut starts);
        starts.sort_unstable();
        let expected = source
            .match_indices("throw ")
            .map(|(start, _)| start)
            .collect::<Vec<_>>();
        assert_eq!(starts, expected, "{source}");
    }
}

#[test]
fn page_installation_charges_code_and_maps_and_releases_both_on_discard() {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://positions.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    let program = BlueJsProgramV1::Script(parse("throw 'probe';").unwrap());
    let handle = runtime
        .install_program_with_diagnostics(1, &origin, identity("original"), &program, root_map)
        .unwrap();
    let code = runtime.program_registry().get(handle).unwrap().bytecode();
    assert_eq!(
        runtime.realm_stats(1).unwrap().bytecode_bytes,
        code.bytes().len() + code.diagnostic_source_map_bytes()
    );
    runtime.discard_program(1, handle).unwrap();
    assert_eq!(runtime.realm_stats(1).unwrap().bytecode_bytes, 0);
    assert!(runtime.program_registry().get(handle).is_err());
}

#[test]
fn source_map_builder_and_budget_failures_leave_no_partially_installed_program() {
    let program = BlueJsProgramV1::Script(parse("throw 'probe';").unwrap());
    let code_bytes = program.compile().unwrap().bytes().len();
    let mut config = BlueJsPageRuntimeConfig::default();
    config.max_bytecode_bytes_per_realm = code_bytes;
    let mut runtime = BlueJsPageRuntime::new(config).unwrap();
    let origin = BlueJsPageOrigin::new("https://positions.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    let mut attempted = None;
    let result = runtime.install_program_with_diagnostics(
        1,
        &origin,
        identity("original"),
        &program,
        |registry, handle| {
            attempted = Some(handle);
            root_map(registry, handle)
        },
    );
    assert!(matches!(
        result,
        Err(BlueJsPageRuntimeError::BytecodeLimit { .. })
    ));
    assert!(runtime.program_registry().get(attempted.unwrap()).is_err());
    assert_eq!(runtime.realm_stats(1).unwrap().program_count, 0);
    assert_eq!(runtime.realm_stats(1).unwrap().bytecode_bytes, 0);
    let result: Result<_, BlueJsPageRuntimeError> = runtime.install_program_with_diagnostics(
        1,
        &origin,
        identity("original"),
        &program,
        |_, _| {
            Err(BlueJsPageRuntimeError::DiagnosticSourceMap(
                Error::SourceMismatch,
            ))
        },
    );
    assert!(matches!(
        result,
        Err(BlueJsPageRuntimeError::DiagnosticSourceMap(
            Error::SourceMismatch
        ))
    ));
    assert_eq!(runtime.realm_stats(1).unwrap().program_count, 0);
}
