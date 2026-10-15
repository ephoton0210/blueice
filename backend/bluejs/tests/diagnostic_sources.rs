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
fn cleanup_preserves_the_origin_of_the_throw_that_actually_escapes() {
    let mut failures = Vec::new();
    for (name, source, expected_line) in [
        (
            "caught error in finalizer",
            "try { throw 'original'; } finally { try { throw 'caught'; } catch {} }",
            2,
        ),
        (
            "nested finalizer replacement",
            "try { throw 'original'; } finally { try { throw 'replacement'; } finally { try { throw 'caught'; } catch {} } }",
            3,
        ),
        (
            "abrupt finalizer replacement",
            "try { throw 'original'; } finally { throw 'replacement'; }",
            3,
        ),
        (
            "report during finalizer",
            "try { throw 'original'; } finally { try { throw 'caught'; } catch {} reportError('report'); }",
            2,
        ),
        (
            "iterator close failure during throw",
            "var iterator={ [Symbol.iterator](){return this;}, next(){return {value:1,done:false};}, return(){throw 'close';} }; for (var value of iterator) { throw 'original'; }",
            2,
        ),
        (
            "first iterator close failure during return",
            "var outer={ [Symbol.iterator](){return this;}, next(){return {value:1,done:false};}, return(){throw 'outer';} }; var inner={ [Symbol.iterator](){return this;}, next(){return {value:1,done:false};}, return(){throw 'inner';} }; function fail(){for(var a of outer){for(var b of inner){return 9;}}} fail();",
            3,
        ),
        (
            "native collection adder failure",
            "var input={ [Symbol.iterator](){return this;}, next(){return {value:[1,2],done:false};}, return(){throw 'close';} }; Map.prototype.set=function(){throw 'original';}; new Map(input);",
            3,
        ),
        (
            "direct iterator helper callback failure",
            "var input={next(){return {value:1,done:false};},return(){throw 'close';}}; Iterator.from(input).map(function(){throw 'original';}).next();",
            3,
        ),
        (
            "unmapped original through mapped finalizer",
            "try{eval(\"throw 'original';\");}finally{try{throw 'caught';}catch{}}",
            0,
        ),
    ] {
        let mut vm = page();
        let code = mapped_throw_code(source, "memory:///position.js");
        assert!(vm.execute_script(&code).is_err(), "{name}");
        if name == "caught error in finalizer" {
            let site = vm.debugger_uncaught_throw_site().unwrap();
            assert_eq!(site.code_unit_ordinal, 0);
            assert_eq!(code.diagnostic_source_location(site.bytecode_offset), Some(&location(2)));
        }
        let actual = observed(&mut vm);
        let expected = Value::String(if expected_line == 0 {
            ":0:0".into()
        } else {
            format!("memory:///position.js:{expected_line}:1").into()
        });
        if actual != expected {
            failures.push((name, actual, expected));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

fn mapped_throw_code(source: &str, filename: &str) -> blueice_bluejs::Bytecode {
    fn collect(
        code: &blueice_bluejs::Bytecode,
        filename: &str,
        unit: &mut u32,
        ranges: &mut Vec<DiagnosticSourceRange>,
    ) {
        let ordinal = *unit;
        *unit += 1;
        for instruction in code.instructions().filter(|i| i.opcode == Opcode::Throw) {
            let mut row = range(
                ordinal,
                instruction.offset as u32,
                instruction.offset as u32 + 1,
            );
            row.location = DiagnosticSourceLocation {
                filename: filename.into(),
                line: ranges.len() as u32 + 2,
                column: 1,
            };
            ranges.push(row);
        }
        for child in code.child_code_units() {
            collect(child, filename, unit, ranges);
        }
    }
    let mut registry = BlueJsProgramRegistry::default();
    let program = BlueJsProgramV1::Script(parse(source).unwrap());
    let handle = registry.install(identity("original"), &program).unwrap();
    let mut ranges = Vec::new();
    collect(
        registry.get(handle).unwrap().bytecode(),
        filename,
        &mut 0,
        &mut ranges,
    );
    registry
        .bind_diagnostic_sources(
            handle,
            DiagnosticSourceMap::new(identity("original"), ranges).unwrap(),
        )
        .unwrap();
    registry.get(handle).unwrap().bytecode().clone()
}

#[test]
fn copied_console_errors_keep_the_escaping_origin_after_error_handlers_run() {
    let mut vm = page();
    vm.execute_script(
        &compile(&parse("onerror=function(){console.log('handler');return false;};").unwrap())
            .unwrap(),
    )
    .unwrap();
    let code = mapped_throw_code(
        "try{throw 'original';}finally{try{throw 'caught';}catch{}}",
        "memory:///position.js",
    );
    assert!(vm.execute_script(&code).is_err());
    let batch = serde_json::to_value(vm.drain_host_console().unwrap()).unwrap();
    assert_eq!(batch["records"][0]["message"], "handler");
    assert_eq!(batch["records"][1]["message"], "Uncaught original");
    assert_eq!(
        batch["records"][1]["source"],
        serde_json::json!({
            "filename":"memory:///position.js", "line":2, "column":1
        })
    );
    assert!(batch["records"][0].get("source").is_none());
}

#[test]
fn copied_console_calls_keep_active_locations_and_leave_unknown_code_unlocated() {
    for source in ["console.log('probe');", "reportError('probe');"] {
        let mut vm = page();
        vm.execute_script(&compile(&parse("onerror=null;").unwrap()).unwrap())
            .unwrap();
        let mut registry = BlueJsProgramRegistry::default();
        let handle = registry
            .install(
                identity("original"),
                &BlueJsProgramV1::Script(parse(source).unwrap()),
            )
            .unwrap();
        let instruction = registry
            .get(handle)
            .unwrap()
            .bytecode()
            .instructions()
            .find(|i| i.opcode == Opcode::Call)
            .unwrap();
        let mut row = range(
            0,
            instruction.offset as u32,
            (instruction.offset + instruction.opcode.width()) as u32,
        );
        row.location = location(42);
        registry
            .bind_diagnostic_sources(
                handle,
                DiagnosticSourceMap::new(identity("original"), vec![row]).unwrap(),
            )
            .unwrap();
        vm.execute_script(registry.get(handle).unwrap().bytecode())
            .unwrap();
        let batch = serde_json::to_value(vm.drain_host_console().unwrap()).unwrap();
        assert_eq!(
            batch["records"][0]["source"],
            serde_json::json!({
                "filename":"memory:///position.js", "line":42, "column":1
            }),
            "{source}"
        );
        vm.execute_script(&compile(&parse("console.log('unknown');").unwrap()).unwrap())
            .unwrap();
        let unknown = serde_json::to_value(vm.drain_host_console().unwrap()).unwrap();
        assert!(unknown["records"][0].get("source").is_none());
    }
}

#[test]
fn copied_console_source_names_share_eviction_and_native_payload_accounting() {
    let mut vm = page();
    vm.install_host_rejection_tracking().unwrap();
    let filename = "f".repeat(8192);
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install(
            identity("original"),
            &BlueJsProgramV1::Script(
                parse("for(var i=0;i<100;i++){console.log('message');}").unwrap(),
            ),
        )
        .unwrap();
    let length = registry.get(handle).unwrap().bytecode().bytes().len() as u32;
    let mut row = range(0, 0, length);
    row.location.filename = filename.clone();
    registry
        .bind_diagnostic_sources(
            handle,
            DiagnosticSourceMap::new(identity("original"), vec![row]).unwrap(),
        )
        .unwrap();
    vm.execute_script(registry.get(handle).unwrap().bytecode())
        .unwrap();
    let buffered = vm.host_scheduling_usage().unwrap().payload_bytes;
    let batch = vm.drain_host_console().unwrap();
    let drained = vm.host_scheduling_usage().unwrap().payload_bytes;
    assert!(batch.records.len() < 100);
    assert_eq!(batch.dropped_records, 100 - batch.records.len() as u64);
    let text_bytes = batch
        .records
        .iter()
        .map(|record| {
            let source = record.source.as_ref().unwrap();
            assert_eq!(source.filename, filename);
            record.message.capacity() + source.filename.capacity()
        })
        .sum::<usize>();
    assert!(text_bytes <= blueice_bluejs::MAX_HOST_CONSOLE_BYTES);
    assert_eq!(buffered - drained, text_bytes);
}

#[test]
fn copied_console_unknown_tasks_do_not_inherit_the_previous_scripts_error_source() {
    let mut vm = page();
    vm.install_host_timer_functions().unwrap();
    vm.execute_script(
        &compile(
            &parse("onerror=null;setTimeout(Function(\"throw 'unknown-task';\"),0);").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(vm
        .execute_script(&mapped_throw_code(
            "throw 'original';",
            "memory:///position.js"
        ))
        .is_err());
    vm.drain_host_console().unwrap();
    assert_eq!(vm.run_due_host_timers(0).unwrap().uncaught_errors, 1);
    let batch = serde_json::to_value(vm.drain_host_console().unwrap()).unwrap();
    assert_eq!(batch["records"][0]["message"], "Uncaught unknown-task");
    assert!(batch["records"][0].get("source").is_none(), "{batch}");
}

#[test]
fn a_suspended_finalizer_restores_its_throw_after_another_script_and_collection() {
    let mut vm = page();
    let code = mapped_throw_code(
        "function* fail(){try{throw 'original';}finally{yield 1;try{throw 'caught';}catch{}}} var suspended=fail(); suspended.next();",
        "memory:///position.js",
    );
    vm.execute_script(&code).unwrap();
    let before = vm.heap().stats().minor_collections;
    // The caller's first caught throw deliberately has the same epoch as the
    // old generator throw. Resuming must publish a new propagation epoch.
    let resume = compile(&parse("try{throw 'ambient';}catch{}; for(var i=0;i<1000;i++){var garbage={i:i};} suspended.next();").unwrap()).unwrap();
    assert!(vm.execute_script(&resume).is_err());
    assert!(vm.heap().stats().minor_collections > before);
    let site = vm.debugger_uncaught_throw_site().unwrap();
    assert_eq!(site.code_unit_ordinal, 1);
    assert_eq!(
        code.child_code_units()
            .next()
            .unwrap()
            .diagnostic_source_location(site.bytecode_offset),
        Some(&location(2))
    );
    assert_eq!(
        observed(&mut vm),
        Value::String("memory:///position.js:2:1".into())
    );
}

#[test]
fn suspended_finalizer_source_copies_join_generator_and_await_byte_budgets() {
    fn retained_bytes(source: &str, filename: &str) -> (usize, usize) {
        let mut vm = page();
        vm.install_host_rejection_tracking().unwrap();
        vm.execute_script(&mapped_throw_code(source, filename))
            .unwrap();
        (
            vm.heap().stats().managed_bytes,
            vm.host_scheduling_usage().unwrap().payload_bytes,
        )
    }
    let short = "memory:///a.js";
    let long = format!("memory:///{}.js", "x".repeat(4096));
    let extra = long.len() - short.len();
    let generator = "function* fail(){try{throw 'original';}finally{yield 1;}} var suspended=fail(); suspended.next();";
    let (short_heap, _) = retained_bytes(generator, short);
    let (long_heap, _) = retained_bytes(generator, &long);
    assert_eq!(long_heap - short_heap, extra);
    let asynchronous = "var hold=Promise.withResolvers(); async function fail(){try{throw 'original';}finally{await hold.promise;}} var suspended=fail();";
    let (_, short_native) = retained_bytes(asynchronous, short);
    let (_, long_native) = retained_bytes(asynchronous, &long);
    assert_eq!(long_native - short_native, extra);
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
