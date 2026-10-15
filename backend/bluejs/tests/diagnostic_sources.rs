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
    page_with_vm(Vm::default())
}

fn page_with_vm(mut vm: Vm) -> Vm {
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

fn mapped_promise_code(source: &str) -> blueice_bluejs::Bytecode {
    mapped_promise_code_with_filename(source, "memory:///position.js")
}

fn mapped_promise_code_with_filename(source: &str, filename: &str) -> blueice_bluejs::Bytecode {
    mapped_program(BlueJsProgramV1::Script(parse(source).unwrap()), filename)
}

fn mapped_module(source: &str, filename: &str) -> blueice_bluejs::Bytecode {
    mapped_program(
        BlueJsProgramV1::Module(blueice_bluejs::parse_module(source).unwrap()),
        filename,
    )
}

fn mapped_program(program: BlueJsProgramV1, filename: &str) -> blueice_bluejs::Bytecode {
    fn collect(
        code: &blueice_bluejs::Bytecode,
        ordinal: &mut u32,
        rows: &mut Vec<DiagnosticSourceRange>,
    ) {
        let unit = *ordinal;
        *ordinal += 1;
        let mut row = range(unit, 0, code.bytes().len() as u32);
        row.location = location(unit + 2);
        rows.push(row);
        for child in code.child_code_units() {
            collect(child, ordinal, rows);
        }
    }
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry.install(identity("original"), &program).unwrap();
    let mut rows = Vec::new();
    collect(registry.get(handle).unwrap().bytecode(), &mut 0, &mut rows);
    for row in &mut rows {
        row.location.filename = filename.into();
    }
    registry
        .bind_diagnostic_sources(
            handle,
            DiagnosticSourceMap::new(identity("original"), rows).unwrap(),
        )
        .unwrap();
    registry.get(handle).unwrap().bytecode().clone()
}

#[test]
fn module_rejection_origins_survive_async_fanout_and_cached_imports() {
    use std::collections::HashMap;
    let mut failures = Vec::new();
    for (name, dependency, parent, expected_file, expected_line) in [
        (
            "synchronous",
            "throw 'probe';",
            "import './dep.js';",
            "dep",
            2,
        ),
        (
            "asynchronous",
            "await 0;throw 'probe';",
            "import './dep.js';",
            "dep",
            2,
        ),
        (
            "awaited finalizer",
            "try{throw 'probe';}finally{await 0;try{throw 'caught';}catch{}}",
            "import './dep.js';",
            "dep",
            2,
        ),
        (
            "rejected await",
            "await globalThis.input;",
            "import './dep.js';",
            "driver",
            2,
        ),
        (
            "rejected await through disposal",
            "await using resource=null;await globalThis.input;",
            "import './dep.js';",
            "driver",
            2,
        ),
        (
            "unknown module",
            "Function(\"throw 'probe';\")();",
            "import './dep.js';",
            "",
            0,
        ),
    ] {
        let mut vm = page();
        vm.install_host_rejection_tracking().unwrap();
        vm.execute_script(&mapped_promise_code_with_filename(
            "var input=Promise.reject('probe');input.catch(function(){});",
            "memory:///driver.js",
        ))
        .unwrap();
        vm.set_module_loader_context(
            "root.js",
            HashMap::from([
                (
                    "dep.js".into(),
                    mapped_module(dependency, "memory:///dep.js"),
                ),
                ("one.js".into(), mapped_module(parent, "memory:///one.js")),
                ("two.js".into(), mapped_module(parent, "memory:///two.js")),
            ]),
        );
        for turn in 0..2 {
            vm.execute_script(&mapped_promise_code_with_filename(
                "import('./one.js');import('./two.js');",
                "memory:///importer.js",
            ))
            .unwrap();
            vm.run_host_microtask_checkpoint().unwrap();
            vm.run_browser_task_turn(1).unwrap();
            let batch = vm.drain_host_console().unwrap();
            assert_eq!(batch.records.len(), 2, "{name}/{turn}: {batch:?}");
            for record in batch.records {
                assert_eq!(
                    record.message, "Uncaught (in promise) probe",
                    "{name}/{turn}"
                );
                let expected = (expected_line != 0).then(|| DiagnosticSourceLocation {
                    filename: format!("memory:///{expected_file}.js"),
                    line: expected_line,
                    column: 1,
                });
                if record.source != expected {
                    failures.push((name, turn, record.source, expected));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn asynchronous_module_parents_share_one_charged_origin() {
    use std::collections::HashMap;
    fn usage(filename: &str) -> usize {
        let mut vm = page();
        vm.execute_script(&compile(&parse("var hold=Promise.withResolvers();").unwrap()).unwrap())
            .unwrap();
        let modules = HashMap::from([
            (
                "main.js".into(),
                mapped_module("import './one.js';import './two.js';", "memory:///main.js"),
            ),
            (
                "one.js".into(),
                mapped_module("import './dep.js';", "memory:///one.js"),
            ),
            (
                "two.js".into(),
                mapped_module("import './dep.js';", "memory:///two.js"),
            ),
            (
                "dep.js".into(),
                mapped_module("await globalThis.hold.promise;throw 'probe';", filename),
            ),
        ]);
        vm.execute_module_graph("main.js", &modules).unwrap();
        vm.execute_script(&compile(&parse("hold.resolve();").unwrap()).unwrap())
            .unwrap();
        vm.run_host_microtask_checkpoint().unwrap();
        vm.execute_script(&compile(&parse("0;").unwrap()).unwrap())
            .unwrap();
        vm.host_scheduling_usage().unwrap().payload_bytes
    }
    let short = "memory:///a.js";
    let long = format!("memory:///{}.js", "x".repeat(4096));
    assert_eq!(usage(&long) - usage(short), long.len() - short.len());
}

#[test]
fn page_module_errors_keep_the_dependency_origin_after_top_level_await() {
    use std::collections::HashMap;
    for source in [
        "throw 'probe';",
        "await 0;throw 'probe';",
        "try{throw 'probe';}finally{await 0;}",
    ] {
        let mut vm = page();
        let modules = HashMap::from([
            (
                "main.js".into(),
                mapped_module("import './dep.js';", "memory:///main.js"),
            ),
            ("dep.js".into(), mapped_module(source, "memory:///dep.js")),
        ]);
        assert!(
            vm.execute_module_graph("main.js", &modules).is_err(),
            "{source}"
        );
        vm.run_host_microtask_checkpoint().unwrap();
        assert_eq!(
            observed(&mut vm),
            Value::String("memory:///dep.js:2:1".into()),
            "{source}"
        );
        assert!(vm.drain_host_console().unwrap().records.is_empty());
    }
}

#[test]
fn dynamic_import_argument_failures_keep_their_own_origin() {
    for (source, line) in [
        ("import({toString(){throw 'probe';}});", Some(3)),
        ("import('./dep.js',{get with(){throw 'probe';}});", Some(3)),
        ("import({toString:Function(\"throw 'probe';\")});", None),
    ] {
        let mut vm = page();
        vm.install_host_rejection_tracking().unwrap();
        vm.execute_script(&mapped_promise_code(source)).unwrap();
        vm.run_host_microtask_checkpoint().unwrap();
        vm.run_browser_task_turn(1).unwrap();
        let records = vm.drain_host_console().unwrap().records;
        assert_eq!(records.len(), 1, "{source}");
        assert_eq!(
            records[0].message, "Uncaught (in promise) probe",
            "{source}"
        );
        let expected = line.map(|line| DiagnosticSourceLocation {
            filename: "memory:///position.js".into(),
            line,
            column: 1,
        });
        assert_eq!(records[0].source, expected, "{source}");
    }
}

#[test]
fn resource_disposal_keeps_the_escaping_failure_origin() {
    let mut failures = Vec::new();
    for (name, source, expected_line) in [
        ("await using preserves rejected input", "var input=Promise.reject('probe');(async function(){await using x=null;await input;})();", 2),
        ("await using awaits rejected disposer", "var input=Promise.reject('probe');(async function(){await using x={[Symbol.asyncDispose](){return input;}};})();", 2),
        ("await using unknown failure", "(async function(){await using x=null;Function(\"throw 'probe';\")();})();", 0),
        ("async stack rejected disposer", "var input=Promise.reject('probe');var stack=new AsyncDisposableStack();stack.defer(function(){return input;});stack.disposeAsync();", 2),
        ("async stack throwing disposer", "var stack=new AsyncDisposableStack();stack.defer(function(){throw 'probe';});stack.disposeAsync();", 3),
        ("async stack later caught noise", "var stack=new AsyncDisposableStack();stack.defer(function(){try{throw 'caught';}catch{}});stack.defer(function(){throw 'probe';});stack.disposeAsync();", 4),
        ("async stack suppression", "var input=Promise.reject('probe');var stack=new AsyncDisposableStack();stack.defer(function(){return input;});stack.defer(function(){throw 'ignored';});stack.disposeAsync();", 2),
        ("async stack unknown replacement", "var stack=new AsyncDisposableStack();stack.defer(Function(\"throw 'probe';\"));stack.defer(function(){throw 'ignored';});stack.disposeAsync();", 0),
        ("sync stack later caught noise", "var stack=new DisposableStack();stack.defer(function(){try{throw 'caught';}catch{}});stack.defer(function(){throw 'probe';});Promise.resolve().then(function(){stack.dispose();});", 4),
        ("using later caught noise", "(async function(){using first={[Symbol.dispose](){try{throw 'caught';}catch{}}};using second={[Symbol.dispose](){throw 'probe';}};})();", 5),
    ] {
        for aggressive_gc in [false, true] {
            let mut config = blueice_bluejs::VmConfig::default();
            if aggressive_gc {
                config.heap.nursery_capacity = 1;
            }
            let mut vm = page_with_vm(Vm::new(config).unwrap());
            vm.install_host_rejection_tracking().unwrap();
            vm.execute_script(&mapped_promise_code(source)).unwrap();
            vm.run_host_microtask_checkpoint().unwrap();
            vm.run_browser_task_turn(1).unwrap();
            let batch = vm.drain_host_console().unwrap();
            assert_eq!(batch.records.len(), 1, "{name}/{aggressive_gc}: {batch:?}");
            let expected = (expected_line != 0).then(|| location(expected_line));
            if batch.records[0].source != expected {
                failures.push((name, aggressive_gc, batch.records[0].source.clone(), expected));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn disposal_origins_survive_suspension_and_are_charged_without_retaining_owners() {
    fn suspended(source: &str, filename: &str) -> Vm {
        let mut config = blueice_bluejs::VmConfig::default();
        config.heap.nursery_capacity = 1;
        config.heap.major_threshold_bytes = 1;
        let mut vm = page_with_vm(Vm::new(config).unwrap());
        vm.install_host_rejection_tracking().unwrap();
        vm.execute_script(&mapped_promise_code_with_filename(source, filename))
            .unwrap();
        vm.run_host_microtask_checkpoint().unwrap();
        assert!(vm.drain_host_console().unwrap().records.is_empty());
        vm
    }
    for (source, line) in [
        ("var hold=Promise.withResolvers();var stack=new AsyncDisposableStack();stack.defer(function(){return hold.promise;});stack.defer(function(){throw 'probe';});var result=stack.disposeAsync();var weak=new WeakRef(result);", 4),
        ("var stack;var hold=Promise.withResolvers();var result=(async function(){await using first={[Symbol.asyncDispose](){return hold.promise;}};await using second={[Symbol.asyncDispose](){throw 'probe';}};})();var weak=new WeakRef(result);", 5),
    ] {
        let short = "memory:///a.js";
        let long = format!("memory:///{}.js", "x".repeat(4096));
        let short_bytes = suspended(source, short).host_scheduling_usage().unwrap().payload_bytes;
        let mut vm = suspended(source, &long);
        assert_eq!(vm.host_scheduling_usage().unwrap().payload_bytes - short_bytes, long.len() - short.len());
        let before = vm.heap().stats().major_collections;
        vm.execute_script(&mapped_promise_code_with_filename(
            "try{throw 'ambient';}catch{};for(var i=0;i<1000;i++){var garbage={i:i};}hold.resolve();",
            "memory:///later.js",
        )).unwrap();
        assert!(vm.heap().stats().major_collections > before);
        vm.run_host_microtask_checkpoint().unwrap();
        vm.run_browser_task_turn(1).unwrap();
        let batch = vm.drain_host_console().unwrap();
        assert_eq!(batch.records.len(), 1, "{source}: {batch:?}");
        assert_eq!(batch.records[0].source, Some(DiagnosticSourceLocation {
            filename: long,
            line,
            column: 1,
        }));
        vm.execute_script(&compile(&parse("result=null;stack=null;hold=null;0;").unwrap()).unwrap()).unwrap();
        vm.run_host_microtask_checkpoint().unwrap();
        let before = vm.heap().stats().major_collections;
        let collected = vm.execute_script(&compile(&parse(
            "for(var i=0;i<1000;i++){var garbage={i:i};}weak.deref()===undefined"
        ).unwrap()).unwrap()).unwrap();
        assert!(vm.heap().stats().major_collections > before);
        assert_eq!(collected, Value::Bool(true), "{source}");
    }
}

#[test]
fn promise_rejection_sources_survive_jobs_awaits_and_native_forwarding() {
    let mut failures = Vec::new();
    for (name, source, expected_line) in [
        ("static reject", "Promise.reject('probe');", 2),
        ("executor reject", "new Promise(function(resolve,reject){reject('probe');});", 3),
        ("executor throw", "new Promise(function(){throw 'probe';});", 3),
        ("missing handler", "Promise.reject('probe').then();", 2),
        ("throwing reaction", "Promise.resolve().then(function(){throw 'probe';});", 3),
        ("new rejection in handler", "Promise.reject('ignored').catch(function(){Promise.reject('probe');});", 3),
        ("thenable reject", "Promise.resolve({then(resolve,reject){reject('probe');}});", 3),
        ("thenable throw", "Promise.resolve({then(){throw 'probe';}});", 3),
        ("adopted rejection", "new Promise(function(resolve){resolve(Promise.reject('probe'));});", 3),
        ("rejected await", "var input=Promise.reject('probe'); (async function(){await input;})();", 2),
        ("throw after await", "(async function(){await 0;throw 'probe';})();", 3),
        ("successful finally", "Promise.reject('probe').finally(function(){return Promise.resolve();});", 2),
        ("throwing finally", "Promise.reject('ignored').finally(function(){throw 'probe';});", 3),
        ("species forwarding", "class P extends Promise {} P.reject('probe').then();", 2),
        ("unknown reaction", "Promise.reject('ignored').then(null,Function(\"throw 'probe';\"));", 0),
        ("awaited finalizer preserves throw", "(async function(){try{throw 'probe';}finally{await 0;try{throw 'caught';}catch{}}})();", 3),
        ("rejected finalizer replaces throw", "var input=Promise.reject('probe'); (async function(){try{throw 'ignored';}finally{await input;}})();", 2),
        ("async generator yield", "var input=Promise.reject('probe'); (async function*(){yield input;})().next();", 2),
        ("async generator throw", "(async function*(){await 0;throw 'probe';})().next();", 3),
        ("combinator iterator throws", "Promise.all({[Symbol.iterator](){throw 'probe';}});", 3),
        ("all forwards rejection", "Promise.all([Promise.reject('probe')]);", 2),
        ("race forwards rejection", "Promise.race([Promise.reject('probe')]);", 2),
        ("rethrow replaces origin", "Promise.reject('ignored').catch(function(){throw 'probe';});", 3),
        ("unknown rejected await", r#"var input=Function("return Promise.reject('probe');")(); (async function(){await input;})();"#, 0),
        ("async body throws before await", "(async function(){throw 'probe';})();", 3),
        ("async body adopts rejection", "var input=Promise.reject('probe'); (async function(){return input;})();", 2),
        ("async generator throws before await", "(async function*(){throw 'probe';})().next();", 3),
        ("throwing then getter", "Promise.resolve({get then(){throw 'probe';}});", 3),
        ("finally adopts rejected cleanup", "var input=Promise.reject('probe'); Promise.reject('ignored').finally(function(){return input;});", 2),
        ("Promise.try throws", "Promise.try(function(){throw 'probe';});", 3),
        ("unknown executor throw", r#"new Promise(Function("throw 'probe';"));"#, 0),
        ("unknown finally throw", r#"Promise.reject('ignored').finally(Function("throw 'probe';"));"#, 0),

    ] {
        let mut vm = page();
        vm.install_host_rejection_tracking().unwrap();
        vm.execute_script(&mapped_promise_code(source)).unwrap();
        vm.run_host_microtask_checkpoint().unwrap();
        vm.run_browser_task_turn(1).unwrap();
        let batch = vm.drain_host_console().unwrap();
        assert_eq!(batch.records.len(), 1, "{name}: {batch:?}");
        assert_eq!(batch.records[0].message, "Uncaught (in promise) probe", "{name}");
        let expected = (expected_line != 0).then(|| location(expected_line));
        if batch.records[0].source != expected {
            failures.push((name, batch.records[0].source.clone(), expected));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn queued_async_generator_requests_keep_the_request_or_replacement_origin() {
    let mut failures = Vec::new();
    for aggressive_gc in [false, true] {
        for (name, setup, request, expected_file, expected_line) in [
        ("suspended start", "var hold=Promise.withResolvers();var iterator=(async function*(){})();", "iterator.throw('probe');", "request", 2),
        ("queued after await", "var hold=Promise.withResolvers();var iterator=(async function*(){await hold.promise;})();iterator.next();", "iterator.throw('probe');", "request", 2),
        ("queued after yield", "var hold=Promise.withResolvers();var iterator=(async function*(){yield 0;})();iterator.next();", "iterator.throw('probe');", "request", 2),
        ("awaited finalizer", "var hold=Promise.withResolvers();var iterator=(async function*(){try{yield 0;}finally{await hold.promise;try{throw 'caught';}catch{}}})();iterator.next();", "iterator.throw('probe');", "request", 2),
        ("awaited disposal", "var hold=Promise.withResolvers();var iterator=(async function*(){await using resource={[Symbol.asyncDispose](){return hold.promise;}};yield 0;})();iterator.next();", "iterator.throw('probe');", "request", 2),
        ("replacement finalizer", "var hold=Promise.withResolvers();var iterator=(async function*(){try{yield 0;}finally{await hold.promise;throw 'probe';}})();iterator.next();", "iterator.throw('ignored');", "generator", 3),
        ("rejected return", "var hold=Promise.withResolvers();var iterator=(async function*(){await hold.promise;yield 0;})();iterator.next();", "iterator.return(Promise.reject('probe'));", "request", 2),
        ("return constructor throws", "var hold=Promise.withResolvers();var iterator=(async function*(){yield 0;})();iterator.next();", "var input=Promise.resolve();Object.defineProperty(input,'constructor',{get:function(){throw 'probe';}});iterator.return(input);", "request", 3),
        ("return thenable throws", "var hold=Promise.withResolvers();var iterator=(async function*(){yield 0;})();iterator.next();", "iterator.return({then:function(){throw 'probe';}});", "request", 3),
        ("unknown request", "var hold=Promise.withResolvers();var iterator=(async function*(){yield 0;})();iterator.next();", "Function(\"iterator.throw('probe');\")();", "", 0),
        ("delegated rejection through awaited finalizer", "var hold=Promise.withResolvers();var delegate={[Symbol.asyncIterator](){return this;},next(){return Promise.resolve({done:false,value:0});}};var iterator=(async function*(){try{yield* delegate;}finally{await hold.promise;}})();iterator.next();", "delegate.throw=function(){return Promise.reject('probe');};iterator.throw('ignored');", "request", 3),
        ("delegated rejection replaced by finalizer", "var hold=Promise.withResolvers();var delegate={[Symbol.asyncIterator](){return this;},next(){return Promise.resolve({done:false,value:0});}};var iterator=(async function*(){try{yield* delegate;}finally{throw 'probe';}})();iterator.next();", "delegate.throw=function(){return Promise.reject('ignored');};iterator.throw('ignored');", "generator", 5),
        ("completed return constructor throws", "var hold=Promise.withResolvers();var iterator=(async function*(){})();", "var input=Promise.resolve();Object.defineProperty(input,'constructor',{get:function(){throw 'probe';}});iterator.return(input);", "request", 3),

    ] {
        let mut config = blueice_bluejs::VmConfig::default();
        if aggressive_gc { config.heap.nursery_capacity = 1; config.heap.major_threshold_bytes = 1; }
        let mut vm = page_with_vm(Vm::new(config).unwrap());
        vm.install_host_rejection_tracking().unwrap();
        vm.execute_script(&mapped_promise_code_with_filename(setup, "memory:///generator.js")).unwrap();
        vm.execute_script(&mapped_promise_code_with_filename(request, "memory:///request.js")).unwrap();
        vm.execute_script(&mapped_promise_code_with_filename("hold.resolve();", "memory:///later.js")).unwrap();
        vm.run_host_microtask_checkpoint().unwrap();
        vm.run_browser_task_turn(1).unwrap();
        let batch = vm.drain_host_console().unwrap();
        assert_eq!(batch.records.len(), 1, "{name}: {batch:?}");
        assert_eq!(batch.records[0].message, "Uncaught (in promise) probe", "{name}");
        let expected = (expected_line != 0).then(|| DiagnosticSourceLocation {
            filename: format!("memory:///{expected_file}.js"),
            line: expected_line,
            column: 1,
        });
        if batch.records[0].source != expected {
            failures.push((name, batch.records[0].source.clone(), expected));
        }
    }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn neighboring_async_generator_requests_keep_distinct_and_unknown_sources() {
    let mut config = blueice_bluejs::VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.heap.major_threshold_bytes = 1;
    let mut vm = page_with_vm(Vm::new(config).unwrap());
    vm.install_host_rejection_tracking().unwrap();
    vm.execute_script(&mapped_promise_code_with_filename(
        "var hold=Promise.withResolvers();var iterator=(async function*(){await hold.promise;})();iterator.next();var weak=new WeakRef(iterator);",
        "memory:///generator.js",
    )).unwrap();
    vm.execute_script(&mapped_promise_code_with_filename(
        "iterator.throw('same');",
        "memory:///first.js",
    ))
    .unwrap();
    vm.execute_script(&compile(&parse("iterator.throw('same');").unwrap()).unwrap())
        .unwrap();
    vm.execute_script(&mapped_promise_code_with_filename(
        "iterator.throw('same');",
        "memory:///third.js",
    ))
    .unwrap();
    vm.execute_script(
        &compile(&parse("for(var i=0;i<3000;i++){var garbage={i:i};}hold.resolve();").unwrap())
            .unwrap(),
    )
    .unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    vm.run_browser_task_turn(1).unwrap();
    let batch = vm.drain_host_console().unwrap();
    assert_eq!(batch.records.len(), 3, "{batch:?}");
    for record in &batch.records {
        assert_eq!(record.message, "Uncaught (in promise) same");
    }
    let sources = batch
        .records
        .iter()
        .map(|record| {
            record
                .source
                .as_ref()
                .map(|source| (source.filename.as_str(), source.line, source.column))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        [
            Some(("memory:///first.js", 2, 1)),
            None,
            Some(("memory:///third.js", 2, 1))
        ]
    );
    let before = vm.heap().stats().major_collections;
    let collected = vm.execute_script(&compile(&parse("iterator=null;hold=null;for(var i=0;i<3000;i++){var garbage={i:i};}weak.deref()===undefined").unwrap()).unwrap()).unwrap();
    assert!(vm.heap().stats().major_collections > before);
    assert_eq!(collected, Value::Bool(true));
}

#[test]
fn queued_async_generator_source_names_join_heap_accounting() {
    fn usage(filename: &str) -> (usize, usize) {
        let mut vm = page();
        vm.install_host_rejection_tracking().unwrap();
        vm.execute_script(&mapped_promise_code_with_filename(
            "var hold=Promise.withResolvers();var iterator=(async function*(){await hold.promise;})();iterator.next();",
            "memory:///generator.js",
        )).unwrap();
        vm.execute_script(&mapped_promise_code_with_filename(
            "iterator.throw('probe');",
            filename,
        ))
        .unwrap();
        (
            vm.heap().stats().managed_bytes,
            vm.host_scheduling_usage().unwrap().payload_bytes,
        )
    }
    let short = "memory:///a.js";
    let long = format!("memory:///{}.js", "x".repeat(4096));
    let (short_heap, short_native) = usage(short);
    let (long_heap, long_native) = usage(&long);
    assert_eq!(long_heap - short_heap, long.len() - short.len());
    assert_eq!(long_native, short_native);
}

#[test]
fn promise_rejection_records_and_queued_jobs_charge_source_copies() {
    fn usage(filename: &str) -> (usize, usize) {
        let mut vm = page();
        vm.install_host_rejection_tracking().unwrap();
        vm.execute_script(&mapped_promise_code_with_filename(
            "var retained=Promise.reject('probe');retained.catch(function(){});",
            filename,
        ))
        .unwrap();
        let queued = vm.host_scheduling_usage().unwrap().payload_bytes;
        vm.run_host_microtask_checkpoint().unwrap();
        (queued, vm.host_scheduling_usage().unwrap().payload_bytes)
    }
    let short = "memory:///a.js";
    let long = format!("memory:///{}.js", "x".repeat(4096));
    let extra = long.len() - short.len();
    let (short_queued, short_retained) = usage(short);
    let (long_queued, long_retained) = usage(&long);
    assert_eq!(long_queued - short_queued, 2 * extra);
    assert_eq!(long_retained - short_retained, extra);
}

#[test]
fn retained_rejection_sources_obey_the_document_native_byte_limit() {
    fn execute(filename: &str) -> Result<Value, BlueJsPageRuntimeError> {
        let mut runtime = BlueJsPageRuntime::default();
        let origin = BlueJsPageOrigin::new("https://positions.test").unwrap();
        runtime.open_realm(1, origin.clone()).unwrap();
        runtime
            .configure_realm_bindings(1, |host| host.install_host_timers())
            .unwrap();
        let program = BlueJsProgramV1::Script(parse(
            "var retained=[];for(var i=0;i<512;i++){var p=Promise.reject('probe');retained.push(p);p.catch(function(){});} 'complete';"
        ).unwrap());
        let handle = runtime
            .install_program_with_diagnostics(
                1,
                &origin,
                identity("original"),
                &program,
                |registry, handle| {
                    let code = registry.get(handle).unwrap().bytecode();
                    let mut row = range(0, 0, code.bytes().len() as u32);
                    row.location.filename = filename.into();
                    DiagnosticSourceMap::new(identity("original"), vec![row])
                        .map_err(BlueJsPageRuntimeError::DiagnosticSourceMap)
                },
            )
            .unwrap();
        runtime.execute_program(1, handle)
    }
    assert_eq!(
        execute("memory:///a.js").unwrap(),
        Value::String("complete".into())
    );
    assert!(matches!(
        execute(&"f".repeat(8192)),
        Err(BlueJsPageRuntimeError::Runtime(
            blueice_bluejs::RuntimeError::SchedulingLimit
        ))
    ));
}

#[test]
fn finally_thunk_keeps_original_source_across_scripts_and_collection() {
    let mut config = blueice_bluejs::VmConfig::default();
    config.heap.nursery_capacity = 1;
    config.heap.major_threshold_bytes = 1;
    let mut vm = page_with_vm(Vm::new(config).unwrap());
    vm.install_host_rejection_tracking().unwrap();
    vm.execute_script(&mapped_promise_code(
        "var hold=Promise.withResolvers();var retained=Promise.reject('probe').finally(function(){return hold.promise;});var weak=new WeakRef(retained);",
    )).unwrap();
    vm.run_host_microtask_checkpoint().unwrap();
    let before = vm.heap().stats().minor_collections;
    vm.execute_script(&compile(&parse("try{throw 'ambient';}catch{};for(var i=0;i<3000;i++){var garbage={i:i};}hold.resolve();").unwrap()).unwrap()).unwrap();
    assert!(vm.heap().stats().minor_collections > before);
    vm.run_host_microtask_checkpoint().unwrap();
    vm.run_browser_task_turn(1).unwrap();
    let batch = vm.drain_host_console().unwrap();
    assert_eq!(batch.records.len(), 1, "{batch:?}");
    assert_eq!(batch.records[0].source, Some(location(2)));
    assert_eq!(batch.records[0].message, "Uncaught (in promise) probe");
    vm.execute_script(&compile(&parse("retained=null;hold=null;for(var i=0;i<3000;i++){var garbage={i:i};}weak.deref()===undefined").unwrap()).unwrap()).map(|value| assert_eq!(value, Value::Bool(true))).unwrap();
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

fn mapped_expression_code(source: &str) -> blueice_bluejs::Bytecode {
    let (program, spans) = blueice_bluejs::parse_with_diagnostic_node_spans(source).unwrap();
    assert_eq!(
        program,
        parse(source).unwrap(),
        "diagnostic capture preserves syntax"
    );
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_for_diagnostics(identity("original"), &BlueJsProgramV1::Script(program))
        .unwrap();
    fn collect(
        code: &blueice_bluejs::Bytecode,
        unit: &mut u32,
        source: &str,
        spans: &[Option<blueice_bluejs::ParsedStatementSpan>],
        rows: &mut Vec<DiagnosticSourceRange>,
    ) {
        let ordinal = *unit;
        *unit += 1;
        let owners = code.diagnostic_instruction_owners();
        for (index, owner) in owners.iter().enumerate() {
            let Some(span) = owner
                .ast_node_ordinal
                .and_then(|id| spans.get(id as usize))
                .copied()
                .flatten()
            else {
                continue;
            };
            let (line, column) = source_positions(source, &[span.start_byte])[0].unwrap();
            rows.push(DiagnosticSourceRange {
                code_unit_ordinal: ordinal,
                start: owner.bytecode_offset,
                end: owners
                    .get(index + 1)
                    .map_or(code.bytes().len() as u32, |next| next.bytecode_offset),
                location: DiagnosticSourceLocation {
                    filename: "memory:///position.js".into(),
                    line,
                    column,
                },
            });
        }
        for child in code.child_code_units() {
            collect(child, unit, source, spans, rows);
        }
    }
    fn same_code(ordinary: &blueice_bluejs::Bytecode, diagnostic: &blueice_bluejs::Bytecode) {
        assert_eq!(ordinary.bytes(), diagnostic.bytes());
        assert_eq!(
            ordinary.child_code_units().count(),
            diagnostic.child_code_units().count()
        );
        for (ordinary, diagnostic) in ordinary
            .child_code_units()
            .zip(diagnostic.child_code_units())
        {
            same_code(ordinary, diagnostic);
        }
    }
    let diagnostic = registry.get(handle).unwrap().bytecode();
    assert_eq!(diagnostic.diagnostic_source_node_count(), spans.len());
    same_code(&compile(&parse(source).unwrap()).unwrap(), diagnostic);
    let mut rows = Vec::new();
    collect(
        registry.get(handle).unwrap().bytecode(),
        &mut 0,
        source,
        &spans,
        &mut rows,
    );
    registry
        .bind_diagnostic_sources(
            handle,
            DiagnosticSourceMap::new(identity("original"), rows).unwrap(),
        )
        .unwrap();
    registry.get(handle).unwrap().bytecode().clone()
}

#[test]
fn expression_and_parameter_failures_use_their_own_original_source() {
    let mut failures = Vec::new();
    for (name, source, expected) in [
        (
            "initializer operand",
            "var result = (\n  1 +\n  missing\n);",
            "3:3",
        ),
        ("member access", "var value =\n  null.field;", "2:3"),
        ("call argument", "Math.max(\n  missing\n);", "2:3"),
        (
            "parameter default",
            "function fail(value =\n  missing) {}\nfail();",
            "2:3",
        ),
        (
            "parameter pattern",
            "function fail(\n  {value}\n) {}\nfail(null);",
            "2:3",
        ),
        (
            "repeated arrow body",
            "var pair=[()=>missing,()=>\n  absent];\npair[1]();",
            "2:3",
        ),
        ("sequence operand", "(0,\n  missing);", "2:3"),
        (
            "computed property",
            "var value={};value[\n  missing\n];",
            "2:3",
        ),
        (
            "parenthesized arrow",
            "var fail=()=> (\n  missing\n);fail();",
            "2:3",
        ),
        ("binary child", "var v=1 +\n  2n / 0n;", "2:3"),
        ("binary parent", "var v = 1 +\n  1n;", "1:9"),
        ("member intermediate", "var v=\n  null.field.next;", "2:3"),
        (
            "constructor intermediate",
            "var v=\n  new new (0)();",
            "2:7",
        ),
        (
            "arrow parameter default",
            "var fail=(\n  value = missing\n)=>value;fail();",
            "2:11",
        ),
        (
            "generator parameter default",
            "function* fail(value=\n  missing){}var it=fail();it.next();",
            "2:3",
        ),
        (
            "template placeholder",
            "var v=`value ${\n  missing}`;",
            "2:3",
        ),
        (
            "tagged placeholder",
            "function tag(){}tag`value ${\n  missing}`;",
            "2:3",
        ),
        (
            "nested template",
            "var v=`${`inner ${\n  missing}`}`;",
            "2:3",
        ),
        (
            "template callback",
            "var v=`${(()=>{\n  return missing;\n})()}`;",
            "2:10",
        ),
        (
            "template comment brace",
            "var v=`${ /* } */\n  missing }`;",
            "2:3",
        ),
        (
            "CRLF and astral",
            "var face='😀';\r\nvar v =\r\n  missing;",
            "3:3",
        ),
        ("UTF16 column", "var face='😀';missing;", "1:15"),
        (
            "repeated member objects",
            "var object={};\nvar v=[object,object[\n  missing]];",
            "3:3",
        ),
    ] {
        let mut vm = page();
        assert!(
            vm.execute_script(&mapped_expression_code(source)).is_err(),
            "{name}"
        );
        let found = observed(&mut vm);
        let expected = Value::String(format!("memory:///position.js:{expected}").into());
        if found != expected {
            failures.push((name, found, expected));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn class_lowering_and_decorators_keep_their_original_source() {
    let mut failures = Vec::new();
    for (name, source, expected) in [
        ("constructor body", "class C {\n constructor() {\n  missing;\n }\n}\nnew C();", "3:3"),
        ("constructor default", "class C {\n constructor(value =\n  missing) {}\n}\nnew C();", "3:3"),
        ("constructor pattern", "class C {\n constructor(\n  {value}\n ) {}\n}\nnew C(null);", "3:3"),
        ("class getter", "class C {\n get value() {\n  return missing;\n }\n}\nnew C().value;", "3:10"),
        ("class setter pattern", "class C {\n set value(\n  {item}\n ) {}\n}\nnew C().value=null;", "3:3"),
        ("instance field", "class C {\n value =\n  missing;\n}\nnew C();", "3:3"),
        ("static field", "class C {\n static value =\n  missing;\n}", "3:3"),
        ("private field", "class C {\n #value =\n  missing;\n}\nnew C();", "3:3"),
        ("auto accessor field", "class C {\n accessor value =\n  missing;\n}\nnew C();", "3:3"),
        ("static block", "class C {\n static {\n  missing;\n }\n}", "3:3"),
        ("field arrow", "class C {value=()=>missing;other=()=>\n  absent;}\nnew C().other();", "2:3"),
        ("field nested class", "class C {value=class {\n get value(){return missing;}\n};}\nnew (new C().value)().value;", "2:21"),
        ("ordinary method", "class C {\n method(){return missing;}\n}\nnew C().method();", "2:18"),
        ("object getter", "var object={\n get value(){return missing;}\n};object.value;", "2:21"),
        ("class decorator reference", "@missing\nclass C {}", "1:2"),
        ("class decorator member", "var decorators=null;\n@decorators.decorate\nclass C {}", "2:2"),
        ("element decorator member", "var decorators=null;\nclass C {\n @decorators.decorate method(){}\n}", "3:3"),
        ("decorator returned initializer", "function dec(){return function(){\n return missing;\n};}\nclass C {@dec value=1;}new C();", "2:9"),
        ("static getter", "class C {\n static get value() {\n  return missing;\n }\n}\nC.value;", "3:10"),
        ("private getter", "class C {\n get #value() {\n  return missing;\n }\n read(){return this.#value;}\n}\nnew C().read();", "3:10"),
        ("static private field", "class C {\n static #value=\n  missing;\n}", "3:3"),
        ("static auto accessor", "class C {\n static accessor value=\n  missing;\n}", "3:3"),
        ("decorated field", "function dec(){}\nclass C {@dec value=\n  missing;}new C();", "3:3"),
        ("decorated static field", "function dec(){}\nclass C {@dec static value=\n  missing;}", "3:3"),
        ("computed field", "class C {[1]=\n  missing;}new C();", "2:3"),
        ("field function default", "class C {value=function(arg=\n  missing){};}new C().value();", "2:3"),
        ("static block arrow", "class C {\n static {this.run=()=>\n  missing;}\n}\nC.run();", "3:3"),
        ("decorator intermediate", "var decorators={nested:null};\n@decorators.nested.decorate\nclass C {}", "2:2"),
        ("decorator callee", "var decorators=null;\n@decorators.decorate()\nclass C {}", "2:2"),
        ("decorator argument", "function dec(){}\n@dec(\n  missing)\nclass C {}", "3:3"),
        ("parenthesized decorator member", "var decorators=null;\n@(decorators.decorate)\nclass C {}", "2:3"),
        ("CRLF constructor", "var face='😀';\r\nclass C {constructor(){\r\n  missing;}}\r\nnew C();", "3:3"),
    ] {
        let mut vm = page();
        assert!(vm.execute_script(&mapped_expression_code(source)).is_err(), "{name}");
        let found = observed(&mut vm);
        let expected = Value::String(format!("memory:///position.js:{expected}").into());
        if found != expected { failures.push((name, found, expected)); }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn generated_class_operations_and_dynamic_code_do_not_borrow_source_locations() {
    for source in [
        "class C {accessor value=1;}Object.getOwnPropertyDescriptor(C.prototype,'value').get.call({});",
        "class Base {constructor(){return Object.preventExtensions({});}}class C extends Base {value;}new C();",
        "class C {constructor(){eval(\"throw 'dynamic';\");}}new C();",
    ] {
        let mut vm = page();
        assert!(vm.execute_script(&mapped_expression_code(source)).is_err(), "{source}");
        assert_eq!(observed(&mut vm), Value::String(":0:0".into()), "{source}");
    }
}

#[test]
fn lowered_class_closures_keep_sources_across_scripts_and_major_collection() {
    for (setup, invoke, position) in [
        (
            "var saved=class {constructor(){\n  missing;}};",
            "new saved();",
            "2:3",
        ),
        (
            "var saved=new class {get value(){\n  return missing;}};",
            "saved.value;",
            "2:10",
        ),
        (
            "var saved=new class {value=()=>\n  missing;}().value;",
            "saved();",
            "2:3",
        ),
        (
            "var saved;class C {static {saved=()=>\n  missing;}}",
            "saved();",
            "2:3",
        ),
    ] {
        let mut config = blueice_bluejs::VmConfig::default();
        config.heap.nursery_capacity = 1;
        config.heap.major_threshold_bytes = 1;
        let mut vm = page_with_vm(Vm::new(config).unwrap());
        vm.execute_script(&mapped_expression_code(setup)).unwrap();
        let before = vm.heap().stats().major_collections;
        let collect = "try{throw 'ambient';}catch{}for(var i=0;i<3000;i++){var garbage={i:i};}";
        vm.execute_script(&compile(&parse(collect).unwrap()).unwrap())
            .unwrap();
        assert!(vm.heap().stats().major_collections > before);
        assert!(vm
            .execute_script(&compile(&parse(invoke).unwrap()).unwrap())
            .is_err());
        assert_eq!(
            observed(&mut vm),
            Value::String(format!("memory:///position.js:{position}").into()),
            "{setup}"
        );
    }
}

#[test]
fn reference_dispatch_and_assignment_patterns_use_their_own_source() {
    let mut failures = Vec::new();
    for (name, source, expected) in [
        ("parenthesized call", "(\n null.field\n)();", "2:2"),
        (
            "parenthesized optional call",
            "(\n null.field\n)?.();",
            "2:2",
        ),
        ("parenthesized tag", "(\n null.field\n)`value`;", "2:2"),
        ("prefix reference", "++\n missing;", "2:2"),
        ("prefix with reference", "with({}){++\n missing;}", "2:2"),
        ("delete member", "delete (\n null.field\n);", "2:2"),
        (
            "optional reference",
            "var value=(\n missing?.field);",
            "2:2",
        ),
        ("optional key", "({})?.[\n missing];", "2:2"),
        ("nested array", "var value;[\n [value]\n]=[null];", "2:2"),
        (
            "nested object",
            "var value;({key:\n {value}\n}={key:null});",
            "2:2",
        ),
        (
            "array identifier",
            "'use strict';[\n missing\n]=[1];",
            "2:2",
        ),
        (
            "object shorthand",
            "'use strict';({\n missing\n}={missing:1});",
            "2:2",
        ),
        (
            "object identifier",
            "'use strict';({key:\n missing\n}={key:1});",
            "2:2",
        ),
        (
            "for-of reference",
            "'use strict';for(\n missing\n of [1]){}",
            "2:2",
        ),
        (
            "for-of nested pattern",
            "var value;for([\n [value]\n] of [[null]]){}",
            "2:2",
        ),
        (
            "prepared array member",
            "var object=null;[\n object.field\n]=[1];",
            "2:2",
        ),
        (
            "prepared array rest member",
            "var object=null;[...\n object.field\n]=[];",
            "2:2",
        ),
        (
            "prepared object member",
            "var object=null;({key:\n object.field\n}={key:1});",
            "2:2",
        ),
        ("array default", "var value;[\n value=missing\n]=[];", "2:8"),
        ("nested default", "var value;[\n [value]=null\n]=[];", "2:2"),
        (
            "shorthand default",
            "'use strict';({\n missing=1\n}={});",
            "2:2",
        ),
        (
            "constructor pattern",
            "class C{constructor(){var v;[\n [v]\n]=[null];}}new C();",
            "2:2",
        ),
        (
            "arrow pattern",
            "var fail=()=>{var v;[\n [v]\n]=[null];};fail();",
            "2:2",
        ),
        (
            "field arrow pattern",
            "class C{value=()=>{var v;[\n [v]\n]=[null];};}new C().value();",
            "2:2",
        ),
        (
            "template pattern",
            "var value;`${(()=>{[\n [value]\n]=[null];})()}`;",
            "2:2",
        ),
        (
            "nested object identifier",
            "'use strict';({key:[\n missing\n]}={key:[1]});",
            "2:2",
        ),
        (
            "parenthesized target",
            "'use strict';[\n (missing)\n]=[1];",
            "2:3",
        ),
        ("compound reference", "(\n missing\n)+=1;", "2:2"),
        (
            "strict assignment reference",
            "'use strict';(\n missing\n)=1;",
            "2:2",
        ),
        ("logical assignment reference", "(\n missing\n)||=1;", "2:2"),
        (
            "logical const store",
            "const value=0;(\n value\n)||=1;",
            "2:2",
        ),
        ("const store", "const value=0;(\n value\n)=1;", "2:2"),
        (
            "const array rest",
            "const value=0;[...\n value\n]=[1];",
            "2:2",
        ),
        (
            "object rest identifier",
            "'use strict';({...\n missing\n}={});",
            "2:2",
        ),
        (
            "escaped shorthand",
            "'use strict';({\n \\u006Dissing\n}={missing:1});",
            "2:2",
        ),
        (
            "for-in reference",
            "'use strict';for(\n missing\n in {value:1}){}",
            "2:2",
        ),
        (
            "for-head repeated target",
            "var value;for(value of []){}\n[\n [value]\n]=[null];",
            "3:2",
        ),
    ] {
        let mut vm = page();
        assert!(
            vm.execute_script(&mapped_expression_code(source)).is_err(),
            "{name}"
        );
        let found = observed(&mut vm);
        let expected = Value::String(format!("memory:///position.js:{expected}").into());
        if found != expected {
            failures.push((name, found, expected));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn assignment_pattern_source_slots_follow_binding_slots_without_debugger_authority() {
    let source = "let bound;[\n [bound]\n]=[null];";
    let (program, spans) = blueice_bluejs::parse_with_diagnostic_node_spans(source).unwrap();
    let (legacy, statements) = blueice_bluejs::parse_with_diagnostic_spans(source).unwrap();
    assert_eq!(program, legacy);
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_for_diagnostics(identity("original"), &BlueJsProgramV1::Script(program))
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    assert_eq!(statements.len(), compiled.ast_nodes().len());
    assert_eq!(spans.len(), statements.len() + 4);
    assert_eq!(
        spans.len(),
        compiled.bytecode().diagnostic_source_node_count()
    );
    let starts = spans[statements.len()..]
        .iter()
        .map(|span| span.unwrap().start_byte)
        .collect::<Vec<_>>();
    assert_eq!(
        starts,
        vec![
            source.find("bound;").unwrap(),
            source.find("[\n").unwrap(),
            source.find("[bound]").unwrap(),
            source.find("bound]").unwrap()
        ]
    );
    for (index, span) in statements.iter().enumerate() {
        if span.is_some() {
            assert_eq!(span, &spans[index]);
        }
    }
}

#[test]
fn assignment_pattern_capture_shares_the_bounded_snapshot_budget() {
    let wide = format!("let value;[{}]=[];", vec!["value"; 3_000].join(","));
    assert!(blueice_bluejs::parse_with_diagnostic_node_spans(&wide).is_ok());
    let excessive = format!("let value;[{}]=[];", vec!["value"; 40_000].join(","));
    assert!(parse(&excessive).is_ok());
    let error = blueice_bluejs::parse_with_diagnostic_node_spans(&excessive).unwrap_err();
    assert_eq!(error.message, "diagnostic source capture limit exceeded");
    assert!(!error.known_syntax);
}

#[test]
fn reference_dispatch_preserves_explicitly_unknown_getter_and_proxy_failures() {
    for source in [
        "var object=Object.defineProperty({},'field',{get:Function(\"throw 'unknown';\")});(\n object.field\n)?.();",
        "var target=new Proxy({},{set:Function(\"throw 'unknown';\")});[\n target.value\n]=[1];",
    ] {
        let mut vm=page();
        assert!(vm.execute_script(&mapped_expression_code(source)).is_err());
        assert_eq!(observed(&mut vm),Value::String(":0:0".into()),"{source}");
    }
}

#[test]
fn module_diagnostic_inventory_preserves_exports_and_nested_original_spans() {
    for source in [
        "export default function(){\n throw 'default';\n}",
        "import {value} from './dep.js';export const fail=()=>{\n throw value;\n};",
        "export function first(){throw 1;}export function second(){\n throw 1;\n}",
        "var value;export function fail(){[\n [value]\n]=[null];}",
        "export default class {method(){\n throw 'class';\n}}",
        "function dec(value){return value;}@dec export class C {method(){\n throw 'decorated';\n}}",
        "export default (()=>{\n throw 'arrow';\n})();",
        "export const face='😀';\r\nawait 0;function fail(){\r\n throw 'await';\r\n}fail();",
    ] {
        let (module, spans) =
            blueice_bluejs::parse_module_with_diagnostic_node_spans(source).unwrap();
        assert_eq!(module, blueice_bluejs::parse_module(source).unwrap());
        let mut registry = BlueJsProgramRegistry::default();
        let program = BlueJsProgramV1::Module(module);
        let ordinary = registry.install(identity("ordinary"), &program).unwrap();
        let handle = registry
            .install_for_diagnostics(identity("original"), &program)
            .unwrap();
        let code = registry.get(handle).unwrap().bytecode();
        assert_eq!(code.diagnostic_source_node_count(), spans.len());
        let plain = registry.get(ordinary).unwrap();
        let shape = |compiled: &blueice_bluejs::BlueJsCompiledProgram| {
            compiled
                .ast_nodes()
                .iter()
                .map(|node| {
                    (
                        node.id().ordinal(),
                        node.kind(),
                        node.is_top_level_statement(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(shape(plain), shape(registry.get(handle).unwrap()));
        fn same_code(left: &blueice_bluejs::Bytecode, right: &blueice_bluejs::Bytecode) {
            assert_eq!(left.bytes(), right.bytes());
            let left = left.child_code_units().collect::<Vec<_>>();
            let right = right.child_code_units().collect::<Vec<_>>();
            assert_eq!(left.len(), right.len());
            for (left, right) in left.into_iter().zip(right) {
                same_code(left, right);
            }
        }
        same_code(plain.bytecode(), code);
        fn throws(
            code: &blueice_bluejs::Bytecode,
            spans: &[Option<blueice_bluejs::ParsedDiagnosticSpan>],
            found: &mut Vec<usize>,
        ) {
            for instruction in code.instructions().filter(|i| i.opcode == Opcode::Throw) {
                let owner = code
                    .diagnostic_instruction_owners()
                    .iter()
                    .rev()
                    .find(|owner| owner.bytecode_offset <= instruction.offset as u32)
                    .unwrap();
                found.push(
                    spans[owner.ast_node_ordinal.unwrap() as usize]
                        .unwrap()
                        .start_byte,
                );
            }
            for child in code.child_code_units() {
                throws(child, spans, found);
            }
        }
        let mut found = Vec::new();
        throws(code, &spans, &mut found);
        let expected = source
            .match_indices("throw")
            .map(|(offset, _)| offset)
            .collect::<Vec<_>>();
        assert_eq!(found, expected, "{source}");
    }
}

#[test]
fn diagnostic_patterns_extend_only_the_source_inventory_and_preserve_ordinary_code() {
    let source = "function fail(\n  {value}=missing\n){return value;}";
    let (program, spans) = blueice_bluejs::parse_with_diagnostic_node_spans(source).unwrap();
    let (ordinary, statements) = blueice_bluejs::parse_with_diagnostic_spans(source).unwrap();
    assert_eq!(program, ordinary);
    let mut registry = BlueJsProgramRegistry::default();
    let handle = registry
        .install_for_diagnostics(
            identity("original"),
            &BlueJsProgramV1::Script(program.clone()),
        )
        .unwrap();
    let compiled = registry.get(handle).unwrap();
    assert_eq!(statements.len(), compiled.ast_nodes().len());
    assert_eq!(
        spans.len(),
        compiled.bytecode().diagnostic_source_node_count()
    );
    assert!(spans.len() > statements.len());
    assert_eq!(
        spans[statements.len()].unwrap().start_byte,
        source.find("{value}").unwrap()
    );
    for (index, span) in statements.iter().enumerate() {
        if span.is_some() {
            assert_eq!(span, &spans[index]);
        }
    }
    let plain = compile(&program).unwrap();
    fn same_code(left: &blueice_bluejs::Bytecode, right: &blueice_bluejs::Bytecode) {
        assert_eq!(left.bytes(), right.bytes());
        let left = left.child_code_units().collect::<Vec<_>>();
        let right = right.child_code_units().collect::<Vec<_>>();
        assert_eq!(left.len(), right.len());
        for (left, right) in left.into_iter().zip(right) {
            same_code(left, right);
        }
    }
    same_code(&plain, compiled.bytecode());
}

#[test]
fn diagnostic_capture_bounds_clone_work_and_accepts_wide_expression_lists() {
    let source = format!(
        "var value='{}';",
        "x".repeat(MAX_DIAGNOSTIC_SOURCE_MAP_BYTES / 2)
    );
    assert!(parse(&source).is_ok());
    let error = blueice_bluejs::parse_with_diagnostic_node_spans(&source).unwrap_err();
    assert_eq!(error.message, "diagnostic source capture limit exceeded");
    assert!(!error.known_syntax);
    let source = format!("var value=[{}];", vec!["1"; 10_000].join(","));
    assert!(blueice_bluejs::parse_with_diagnostic_node_spans(&source).is_ok());
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
    let config = BlueJsPageRuntimeConfig {
        max_bytecode_bytes_per_realm: code_bytes,
        ..Default::default()
    };
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
