// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn runtime() -> (bluejs::BlueJsPageRuntime, bluejs::BlueJsPageOrigin) {
    runtime_with_config(bluejs::BlueJsPageRuntimeConfig::default())
}

fn runtime_with_config(
    config: bluejs::BlueJsPageRuntimeConfig,
) -> (bluejs::BlueJsPageRuntime, bluejs::BlueJsPageOrigin) {
    let origin = bluejs::BlueJsPageOrigin::new("https://example.test").unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::new(config).unwrap();
    runtime.open_realm(7, origin.clone()).unwrap();
    runtime
        .configure_realm_bindings(7, |host| {
            host.install_web_file_api()?;
            host.install_window_aliases()?;
            host.install_host_console()?;
            host.install_host_error_reporting()
        })
        .unwrap();
    (runtime, origin)
}

#[test]
fn direct_binding_patterns_and_defaults_keep_their_original_sources() {
    let mut failures = Vec::new();
    for define in [true, false] {
        for (name, source) in [
            ("object root", "const absent: any=null;const\n {value}: any=absent;"),
            ("array root", "const absent: any=null;const\n [value]: any=absent;"),
            ("empty object", "const absent: any=null;const\n {}: any=absent;"),
            ("empty array", "const absent: any={};const\n []: any=absent;"),
            ("object default", "const absent: any=null;const {value=1+(\n absent.field)}: any={};"),
            ("renamed default", "const absent: any=null;const {value: renamed=1+(\n absent.field)}: any={};"),
            ("array default", "const absent: any=null;const [value=1+(\n absent.field)]: any=[];"),
            ("elided array default", "const absent: any=null;const [,value=1+(\n absent.field)]: any=[0];"),
            ("object rest root", "const absent: any=null;const\n {...rest}: any=absent;"),
            ("array rest root", "const absent: any=null;const\n [...rest]: any=absent;"),
            ("function root", "function fail(): void {const\n {value}: any=null;}fail();"),
            ("function default", "const absent: any=null;function fail(): void {const {value=1+(\n absent.field)}: any={};}fail();"),
            ("namespace root", "namespace N {const\n {value}: any=null;}"),
            ("namespace default", "const absent: any=null;namespace N {export function fail(): any {const {value=1+(\n absent.field)}: any={};return value;}}N.fail();"),
            ("method root", "class C {go(): void {const\n [value]: any=null;}}new C().go();"),
            ("constructor root", "class C {constructor(){const\n {value}: any=null;}}new C();"),
            ("getter default", "const absent: any=null;class C {get value(): any {const {value=1+(\n absent.field)}: any={};return value;}}new C().value;"),
            ("static block root", "class C {static {const\n {value}: any=null;}}"),
            ("proxy native property", "const Factory: any=(globalThis as any).Proxy;const target: any=Object.defineProperty({},'value',{value:1});class Handler {get(): any {return 0;}}const subject: any=new Factory(target,new Handler());const\n {value}: any=subject;"),
            ("native iterator", "const key: any=(Symbol as any).iterator;const subject: any={};subject[key]=0;const\n [value]: any=subject;"),
            ("iterator next", "const key: any=(Symbol as any).iterator;class Iterable {[key](): any {return this;}next(): any {return 0;}}const\n [value]: any=new Iterable();"),
            ("iterator close", "const key: any=(Symbol as any).iterator;class Iterable {[key](): any {return this;}next(): any {return {value:1,done:false};}return(): any {return 0;}}const\n [value]: any=new Iterable();"),
        ] {
            let artifact = compile_direct_script(ENTRY, &MapLoader::from([ModuleSource::new(ENTRY, source)]), CompilerOptions {use_define_for_class_fields: Some(define), ..CompilerOptions::default()}).unwrap_or_else(|error| panic!("{name}: {error:?}"));
            let (mut runtime, origin) = runtime();
            let attachment = artifact.attach_in_page_realm(&mut runtime, 7, &origin).unwrap();
            assert!(runtime.execute_program(7, attachment.handle).is_err(), "{name}");
            let messages = runtime.drain_console(7).unwrap();
            assert_eq!(messages.records.len(), 1, "{name}: {messages:?}");
            let actual = messages.records[0].source.as_ref().map(|location| (location.filename.as_str(), location.line, location.column));
            if actual != Some((ENTRY, 2, 2)) {failures.push((name, define, format!("{actual:?}")));}
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn default_export_and_dependency_binding_sources_survive_await() {
    for define in [true, false] {
        for awaited in [false, true] {
            let prefix = if awaited { "await 0;" } else { "" };
            for (dependency, entry) in [
                ("export const absent: any=null;const {value=1+(\n absent.field)}: any={};", "import './dep.ts';"),
                ("export default function(){const\n {value}: any=null;}", "import fail from './dep.ts';fail();"),
                ("export default class {constructor(){const\n [value]: any=null;}}", "import C from './dep.ts';new C();"),
                ("const absent: any=null;export default function(value: any=1+(\n absent.field)){}", "import fail from './dep.ts';fail();"),
                ("const absent: any=null;export default class {value: any=1+(\n absent.field);}", "import C from './dep.ts';new C();"),
                ("export namespace N {export function fail(): void {const\n {value}: any=null;}}N.fail();", "import './dep.ts';"),
            ] {
                let dependency = format!("{prefix}{dependency}");
                let graph = compile_direct_module_graph(GRAPH_ENTRY, &MapLoader::from([ModuleSource::new(GRAPH_ENTRY, entry), ModuleSource::new("graph/dep.ts", &dependency)]), CompilerOptions {use_define_for_class_fields: Some(define), ..CompilerOptions::default()}).unwrap();
                let (mut runtime, origin) = runtime();
                let attachment = graph.attach_in_page_realm(&mut runtime, 7, &origin).unwrap();
                assert!(attachment.execute_in_page_realm(&mut runtime, 7).is_err(), "{dependency}");
                runtime.run_microtask_checkpoint(7).unwrap();
                let messages = runtime.drain_console(7).unwrap();
                assert_eq!(messages.records.len(), 1, "{dependency}: {messages:?}");
                let location = messages.records[0].source.as_ref().unwrap();
                assert_eq!((location.filename.as_str(), location.line, location.column), ("graph/dep.ts", 2, 2), "{dependency}");
            }
        }
    }
}

#[test]
fn pattern_defaults_keep_callback_sources_and_explicit_dynamic_gaps() {
    for (source, expected) in [
        ("class Subject {get value(): any {\n throw 'getter';}}const\n {value}: any=new Subject();", Some((2, 2))),
        ("class Subject {get value(): any {const fail: any=Function(\"throw 'dynamic';\");return fail();}}const\n {value}: any=new Subject();", None),
        ("const fail: any=Function(\"throw 'dynamic';\");const {value=fail()}: any={};", None),
        ("const key: any=(Symbol as any).iterator;class Iterable {[key](): any {return this;}next(): any {const fail: any=Function(\"throw 'dynamic';\");return fail();}}const\n [value]: any=new Iterable();", None),
        ("const absent: any=null;const {value=1+(\n absent.field)}: any={value:1};const\n {}: any=null;", Some((3, 2))),
    ] {
        let artifact = compile_direct_script(ENTRY, &MapLoader::from([ModuleSource::new(ENTRY, source)]), CompilerOptions::default()).unwrap();
        let (mut runtime, origin) = runtime();
        let attachment = artifact.attach_in_page_realm(&mut runtime, 7, &origin).unwrap();
        assert!(runtime.execute_program(7, attachment.handle).is_err(), "{source}");
        let messages = runtime.drain_console(7).unwrap();
        assert_eq!(messages.records.len(), 1, "{messages:?}");
        let actual = messages.records[0].source.as_ref().map(|location| (location.line, location.column));
        assert_eq!(actual, expected, "{source}: {messages:?}");
    }
}

#[test]
fn binding_positions_keep_original_utf16_columns_and_line_endings() {
    for newline in ["", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        for (pattern, initializer) in [("{value}", "null"), ("[value]", "null")] {
            for define in [true, false] {
                let source =
                    format!("const face='😀';const{newline} {pattern}: any={initializer};");
                let offset = source.find(pattern).unwrap();
                let expected = if newline.is_empty() {
                    (1, source[..offset].encode_utf16().count() as u32 + 1)
                } else {
                    (2, 2)
                };
                let artifact = compile_direct_script(
                    ENTRY,
                    &MapLoader::from([ModuleSource::new(ENTRY, &source)]),
                    CompilerOptions {
                        use_define_for_class_fields: Some(define),
                        ..CompilerOptions::default()
                    },
                )
                .unwrap();
                let (mut runtime, origin) = runtime();
                let attachment = artifact
                    .attach_in_page_realm(&mut runtime, 7, &origin)
                    .unwrap();
                assert!(runtime.execute_program(7, attachment.handle).is_err());
                let messages = runtime.drain_console(7).unwrap();
                assert_eq!(messages.records.len(), 1, "{messages:?}");
                let location = messages.records[0].source.as_ref().unwrap();
                assert_eq!((location.line, location.column), expected, "{source:?}");
            }
        }
    }
}

#[test]
fn queued_binding_sources_survive_major_gc_and_expire_on_navigation() {
    for navigate in [false, true] {
        let source = "const absent: any=null;function fail(): void {const {value=1+(\n absent.field)}: any={};}queueMicrotask(fail);";
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ModuleSource::new(
                    "memory:///queue.d.ts",
                    "declare function queueMicrotask(callback: () => void): void;",
                )],
                ..CompilerOptions::default()
            },
        )
        .unwrap();
        let mut config = bluejs::BlueJsPageRuntimeConfig::default();
        config.vm.heap.nursery_capacity = 1;
        config.vm.heap.major_threshold_bytes = 1;
        let (mut runtime, origin) = runtime_with_config(config);
        runtime
            .configure_realm_bindings(7, |host| host.install_host_timers())
            .unwrap();
        let attachment = artifact
            .attach_in_page_realm(&mut runtime, 7, &origin)
            .unwrap();
        runtime.execute_program(7, attachment.handle).unwrap();
        let before = runtime.realm_stats(7).unwrap().heap.major_collections;
        let later = runtime
            .install_program(
                7,
                &origin,
                bluejs::BlueJsSourceIdentity::new("memory:///later.js", "later").unwrap(),
                &bluejs::BlueJsProgramV1::Script(
                    bluejs::parse(
                        "try{throw 'noise';}catch{}for(var i=0;i<3000;i++){var garbage={i:i};}",
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        runtime.execute_program(7, later).unwrap();
        assert!(runtime.realm_stats(7).unwrap().heap.major_collections > before);
        if navigate {
            runtime.navigate(7, origin.clone()).unwrap();
        }
        runtime.run_task_turn(7, 0, 64).unwrap();
        let messages = runtime.drain_console(7).unwrap();
        if navigate {
            assert!(messages.records.is_empty());
        } else {
            assert_eq!(messages.records.len(), 1, "{messages:?}");
            let location = messages.records[0].source.as_ref().unwrap();
            assert_eq!(
                (location.filename.as_str(), location.line, location.column),
                (ENTRY, 2, 2)
            );
        }
    }
}

#[test]
fn binding_slots_use_written_names_and_leave_generated_export_names_unmapped() {
    use bluejs::diagnostics::{visit_diagnostic_source_nodes, DiagnosticSourceNode};
    let source = "function f(this: any, simple: number=1,...rest: any[]): void {const {key: alias=2,...tail}: any={};const [element,,...others]: any=[1];try{throw 0;}catch(caught: any){}}class C {constructor(public property: number){}}";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut expected = BTreeMap::new();
    visit_diagnostic_source_nodes(&artifact.program, &mut |node, ordinal| {
        if let DiagnosticSourceNode::Pattern(bluejs::Pattern::Identifier(name)) = node {
            if [
                "simple", "rest", "alias", "tail", "element", "others", "property", "caught",
            ]
            .contains(&name.as_str())
            {
                let offset = source.find(name).unwrap();
                expected.insert(
                    ordinal as u32,
                    bluejs::diagnostics::source_positions(source, &[offset])[0].unwrap(),
                );
            }
            assert_ne!(name, "this");
        }
    });
    assert_eq!(expected.len(), 8);
    let mut registry = bluejs::BlueJsProgramRegistry::default();
    let source_identity =
        bluejs::BlueJsSourceIdentity::new(ENTRY, &artifact.sources[0].content_hash).unwrap();
    let handle = registry
        .install_for_diagnostics(source_identity, &artifact.program)
        .unwrap();
    let before = registry.get(handle).unwrap().bytecode().clone();
    let map = artifact
        .diagnostic_origins
        .map(&registry, handle, &artifact.program)
        .unwrap();
    registry.bind_diagnostic_sources(handle, map).unwrap();
    fn verify(
        before: &bluejs::Bytecode,
        after: &bluejs::Bytecode,
        expected: &BTreeMap<u32, (u32, u32)>,
        seen: &mut BTreeSet<u32>,
    ) {
        for owner in before.diagnostic_instruction_owners() {
            if let Some((node, coordinates)) = owner
                .ast_node_ordinal
                .and_then(|node| expected.get(&node).map(|coordinates| (node, coordinates)))
            {
                let location = after
                    .diagnostic_source_location(owner.bytecode_offset)
                    .unwrap();
                assert_eq!(&(location.line, location.column), coordinates);
                seen.insert(node);
            }
        }
        for (before, after) in before.child_code_units().zip(after.child_code_units()) {
            verify(before, after, expected, seen);
        }
    }
    let mut seen = BTreeSet::new();
    verify(
        &before,
        registry.get(handle).unwrap().bytecode(),
        &expected,
        &mut seen,
    );
    assert_eq!(seen.len(), expected.len());

    let graph = compile_direct_module_graph(
        MODULE_ENTRY,
        &MapLoader::from([ModuleSource::new(MODULE_ENTRY, "export default class {}")]),
        CompilerOptions::default(),
    )
    .unwrap();
    let artifact = &graph.modules[MODULE_ENTRY];
    let mut generated = BTreeSet::new();
    visit_diagnostic_source_nodes(&artifact.program, &mut |node, ordinal| {
        if let DiagnosticSourceNode::Pattern(bluejs::Pattern::Identifier(name)) = node {
            if name.starts_with('\0') {
                generated.insert(ordinal as u32);
            }
        }
    });
    assert_eq!(generated.len(), 1);
    let source_identity =
        bluejs::BlueJsSourceIdentity::new(MODULE_ENTRY, &artifact.sources[0].content_hash).unwrap();
    let handle = registry
        .install_for_diagnostics(source_identity, &artifact.program)
        .unwrap();
    let before = registry.get(handle).unwrap().bytecode().clone();
    let map = artifact
        .diagnostic_origins
        .map(&registry, handle, &artifact.program)
        .unwrap();
    registry.bind_diagnostic_sources(handle, map).unwrap();
    let after = registry.get(handle).unwrap().bytecode();
    let mut checked = 0;
    for owner in before.diagnostic_instruction_owners() {
        if owner
            .ast_node_ordinal
            .is_some_and(|node| generated.contains(&node))
        {
            assert!(after
                .diagnostic_source_location(owner.bytecode_offset)
                .is_none());
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn pattern_source_capture_preserves_ordinary_ast_and_bytecode() {
    for source in [
        "const {value=1,...rest}: any={};",
        "const [first,,second=1,...rest]: any=[];",
        "function f(value: number=1,...rest: any[]): void {const {key: alias=1}: any={};}",
        "class C {constructor(public property: number) {const [value=1]: any=[];}}",
    ] {
        let module = blueice_bluets::parse_module(ENTRY, source).unwrap();
        let (body, _) = lower_script(&module, true).unwrap();
        let program = bluejs::BlueJsProgramV1::Script(bluejs::Program { body });
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        assert_eq!(program, artifact.program);
        let ordinary = program.compile().unwrap();
        assert_eq!(ordinary.bytes(), artifact.bytecode.bytes());
        assert_eq!(ordinary.constants(), artifact.bytecode.constants());
    }
}

#[test]
fn contextual_keyword_shorthand_bindings_keep_supported_source_evidence() {
    for name in ["async", "readonly", "type", "as"] {
        let source =
            format!("const absent: any=null;const {{{name}=1+(\n absent.field)}}: any={{}};");
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, &source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        let (mut runtime, origin) = runtime();
        let attachment = artifact
            .attach_in_page_realm(&mut runtime, 7, &origin)
            .unwrap();
        assert!(runtime.execute_program(7, attachment.handle).is_err());
        let messages = runtime.drain_console(7).unwrap();
        assert_eq!(messages.records.len(), 1, "{messages:?}");
        let location = messages.records[0].source.as_ref().unwrap();
        assert_eq!(
            (location.filename.as_str(), location.line, location.column),
            (ENTRY, 2, 2)
        );
    }
}
