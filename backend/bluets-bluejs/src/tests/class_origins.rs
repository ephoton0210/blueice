// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn runtime() -> (bluejs::BlueJsPageRuntime, bluejs::BlueJsPageOrigin) {
    let origin = bluejs::BlueJsPageOrigin::new("https://example.test").unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
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
fn direct_class_keys_and_moved_initializers_keep_their_original_sources() {
    let mut failures = Vec::new();
    for define in [true, false] {
        for expression in [false, true] {
            for (name, body, suffix) in [
                (
                    "instance field",
                    "value: any=0 + (\n absent.field);",
                    "new C();",
                ),
                (
                    "static field",
                    "static value: any=0 + (\n absent.field);",
                    "",
                ),
                (
                    "private field",
                    "#value: any=0 + (\n absent.field);",
                    "new C();",
                ),
                ("method", "go(): void {\n throw 'method';}", "new C().go();"),
                (
                    "getter",
                    "get value(): number {\n throw 'getter';}",
                    "new C().value;",
                ),
                ("static block", "static {\n throw 'static';}", ""),
                (
                    "constructor prologue",
                    "value: number=0;constructor(public p: number) {\n throw 'constructor';}",
                    "new C(0);",
                ),
                ("field key", "[\n absent.field]: any=0;", ""),
                ("erased field key", "[\n absent.field]: any;", ""),
                ("method key", "[\n absent.field](): void {}", ""),
                (
                    "pending key prefix",
                    "[0]: any=0;[\n absent.field](): void {}",
                    "",
                ),
                (
                    "member after erased field",
                    "value: any;go(): void {\n throw 'method-after-field';}",
                    "new C().go();",
                ),
                (
                    "anonymous class field",
                    "nested: any=class {go(): void {\n throw 'nested';}};",
                    "const Nested=new C().nested;new Nested().go();",
                ),
            ] {
                let declaration = if expression {
                    format!("const C=class {{{body}}};")
                } else {
                    format!("class C {{{body}}}")
                };
                let source = format!("const absent: any=null;{declaration}{suffix}");
                let artifact = compile_direct_script(
                    ENTRY,
                    &MapLoader::from([ModuleSource::new(ENTRY, &source)]),
                    CompilerOptions {
                        use_define_for_class_fields: Some(define),
                        ..CompilerOptions::default()
                    },
                )
                .unwrap_or_else(|error| {
                    panic!("{name}, define={define}, expression={expression}: {error:?}")
                });
                let (mut runtime, origin) = runtime();
                let attachment = artifact
                    .attach_in_page_realm(&mut runtime, 7, &origin)
                    .unwrap();
                assert!(
                    runtime.execute_program(7, attachment.handle).is_err(),
                    "{name}"
                );
                let messages = runtime.drain_console(7).unwrap();
                assert_eq!(messages.records.len(), 1, "{name}: {messages:?}");
                let location = messages.records[0]
                    .source
                    .as_ref()
                    .map(|source| (source.filename.as_str(), source.line, source.column));
                if location != Some((ENTRY, 2, 2)) {
                    failures.push((
                        name,
                        define,
                        expression,
                        location.map(|(file, line, column)| (file.to_owned(), line, column)),
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn direct_computed_key_conversions_keep_original_sources_after_lowering() {
    let mut failures = Vec::new();
    for define in [true, false] {
        for (name, body) in [
            ("object value", "const value: any={[\n key]:1};"),
            ("class method", "class C {[\n key](): void {}}"),
            ("class getter", "class C {get [\n key](): any {return 0;}}"),
            ("class setter", "class C {set [\n key](value: any){}}"),
            ("static method", "class C {static [\n key](): void {}}"),
            ("class expression", "const C=class {[\n key](): void {}};"),
            (
                "pending erased key",
                "class C {[0]: any;[\n key](): void {}}",
            ),
            (
                "pending initialized key",
                "class C {[0]: any=1;[\n key](): void {}}",
            ),
        ] {
            let source = format!("class Key {{toString(): any {{try{{throw 'caught';}}catch(error){{}}return {{}};}}valueOf(): any {{return {{}};}}}}const key: any=new Key();{body}");
            let artifact = compile_direct_script(
                ENTRY,
                &MapLoader::from([ModuleSource::new(ENTRY, &source)]),
                CompilerOptions {
                    use_define_for_class_fields: Some(define),
                    ..CompilerOptions::default()
                },
            )
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            let (mut runtime, origin) = runtime();
            let attachment = artifact
                .attach_in_page_realm(&mut runtime, 7, &origin)
                .unwrap();
            assert!(
                runtime.execute_program(7, attachment.handle).is_err(),
                "{name}"
            );
            let messages = runtime.drain_console(7).unwrap();
            assert_eq!(messages.records.len(), 1, "{name}: {messages:?}");
            let actual = messages.records[0]
                .source
                .as_ref()
                .map(|location| (location.filename.as_str(), location.line, location.column));
            if actual != Some((ENTRY, 2, 2)) {
                failures.push((name, define, format!("{actual:?}")));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn direct_computed_key_conversions_preserve_callback_sources_and_dynamic_gaps() {
    for (source, expected) in [
        ("class Key {toString(): any {\n throw 'callback';}}const key: any=new Key();const value: any={[\n key]:1};", Some((2, 2))),
        ("class Key {toString(): any {const fail: any=Function(\"throw 'dynamic';\");return fail();}}const key: any=new Key();const value: any={[\n key]:1};", None),
        ("class Key {get toString(): any {try{throw 'caught';}catch(error){}return 0;}valueOf(): any {return {};}}const key: any=new Key();class C {[\n key](): void {}}", Some((2, 2))),
    ] {
        let artifact = compile_direct_script(ENTRY, &MapLoader::from([ModuleSource::new(ENTRY, source)]), CompilerOptions::default()).unwrap();
        let (mut runtime, origin) = runtime();
        let attachment = artifact.attach_in_page_realm(&mut runtime, 7, &origin).unwrap();
        assert!(runtime.execute_program(7, attachment.handle).is_err(), "{source}");
        let messages = runtime.drain_console(7).unwrap();
        assert_eq!(messages.records.len(), 1, "{source}: {messages:?}");
        let actual = messages.records[0].source.as_ref().map(|location| (location.line, location.column));
        assert_eq!(actual, expected, "{source}: {messages:?}");
    }
}

#[test]
fn direct_dependency_key_conversions_keep_sources_before_and_after_await() {
    for define in [true, false] {
        for awaited in [false, true] {
            for (body, entry) in [
                (
                    "export const value: any={[\n key]:1};",
                    "import './dep.ts';",
                ),
                ("export class C {[\n key](): void {}}", "import './dep.ts';"),
                (
                    "export default class {[\n key](): void {}}",
                    "import './dep.ts';",
                ),
                (
                    "export function fail(): void {const value: any={[\n key]:1};}fail();",
                    "import './dep.ts';",
                ),
                (
                    "export class C {[0]: any;[\n key](): void {}}",
                    "import './dep.ts';",
                ),
            ] {
                let prefix = if awaited { "await 0;" } else { "" };
                let dependency = format!("{prefix}class Key {{toString(): any {{return {{}};}}valueOf(): any {{return {{}};}}}}const key: any=new Key();{body}");
                let graph = compile_direct_module_graph(
                    GRAPH_ENTRY,
                    &MapLoader::from([
                        ModuleSource::new(GRAPH_ENTRY, entry),
                        ModuleSource::new("graph/dep.ts", &dependency),
                    ]),
                    CompilerOptions {
                        use_define_for_class_fields: Some(define),
                        ..CompilerOptions::default()
                    },
                )
                .unwrap();
                let (mut runtime, origin) = runtime();
                let attachment = graph
                    .attach_in_page_realm(&mut runtime, 7, &origin)
                    .unwrap();
                assert!(
                    attachment.execute_in_page_realm(&mut runtime, 7).is_err(),
                    "{dependency}"
                );
                runtime.run_microtask_checkpoint(7).unwrap();
                let messages = runtime.drain_console(7).unwrap();
                assert_eq!(messages.records.len(), 1, "{dependency}: {messages:?}");
                let location = messages.records[0]
                    .source
                    .as_ref()
                    .unwrap_or_else(|| panic!("{dependency}: {messages:?}"));
                assert_eq!(
                    (location.filename.as_str(), location.line, location.column),
                    ("graph/dep.ts", 2, 2),
                    "{dependency}"
                );
            }
        }
    }
}

#[test]
fn class_lowering_path_capture_preserves_ast_and_ordinary_bytecode() {
    for define in [true, false] {
        for source in [
            "class C {value: any=1;}",
            "class B {}class C extends B {value: any=1;}",
            "class C {[0]: any=1;[1](): void {}}",
            "class C {[0]: any;static [1]: any=2;}",
            "class C {value: any=class {};constructor(public p: number) {}}",
            "class C {#value: any=0;static value: any=1;get read(): number {return 0;}}",
        ] {
            let module = blueice_bluets::parse_module(ENTRY, source).unwrap();
            let _context = JsxScope::enter(JsxContext::of(
                &CompilerOptions {
                    use_define_for_class_fields: Some(define),
                    ..CompilerOptions::default()
                },
                &module,
            ));
            for original in module.declarations.iter().filter_map(|declaration| {
                if let Declaration::Class(class) = declaration {
                    Some(class)
                } else {
                    None
                }
            }) {
                let ordinary = lowering::classes::lower_class(&module, original, define).unwrap();
                let (captured, origins) =
                    lowering::classes::lower_class_with_origins(&module, original, define).unwrap();
                assert_eq!(captured, ordinary, "{source}, define={define}");
                let bluejs::Stmt::ClassDecl(class) = &captured else {
                    unreachable!()
                };
                assert_eq!(class.elements.len(), origins.elements.len());
                let program = |statement| {
                    bluejs::BlueJsProgramV1::Script(bluejs::Program {
                        body: vec![statement],
                    })
                };
                let ordinary = program(ordinary).compile().unwrap();
                let captured = program(captured).compile().unwrap();
                assert_eq!(ordinary.bytes(), captured.bytes());
                assert_eq!(ordinary.constants(), captured.constants());
            }
        }
    }
}

#[test]
fn class_path_capture_bounds_members_without_executable_expression_slots() {
    let module = blueice_bluets::parse_module(ENTRY, "class C {value: any;}").unwrap();
    let Declaration::Class(mut class) = module.declarations[0].clone() else {
        unreachable!()
    };
    class.members = vec![class.members[0].clone(); 50_000];
    assert!(lowering::classes::lower_class(&module, &class, true).is_ok());
    assert!(
        matches!(lowering::classes::lower_class_with_origins(&module,&class,true),Err(BridgeError::ProvenanceAttachment(message)) if message.contains("byte budget"))
    );
}

#[test]
fn direct_class_decorators_keep_their_original_expressions() {
    for define in [true, false] {
        for source in [
            "const absent: any=null;@(\n absent.field) class C {}",
            "const absent: any=null;class C {@(\n absent.field) go(): void {}}",
            "const bad: any=0;\n@bad class C {}",
            "const Native: any=WeakMap;\n@Native class C {}",
            "const Factory: any=(globalThis as any).Proxy;const dec: any=new Factory(WeakMap,{apply:0});\n@dec class C {}",
            "class Handler {get apply(): any {try{throw 'caught';}catch(error){}return 0;}}const Factory: any=(globalThis as any).Proxy;const dec: any=new Factory(WeakMap,new Handler());\n@dec class C {}",
        ] {
            let artifact = compile_direct_script(
                ENTRY,
                &MapLoader::from([ModuleSource::new(ENTRY, source)]),
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
            assert_eq!(
                (location.filename.as_str(), location.line, location.column),
                (ENTRY, 2, 2),
                "{source}, define={define}: {messages:?}"
            );
        }
    }
}

#[test]
fn generated_field_writes_keep_unavailable_origins() {
    let source="class C {value: any=1;}const ctor: any=C;Object.defineProperty(ctor.prototype,'value',{value:0,writable:false});new C();";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            use_define_for_class_fields: Some(false),
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
    assert!(messages.records[0].source.is_none(), "{messages:?}");
}

#[test]
fn constructor_source_paths_account_for_hoisted_computed_key_bindings() {
    let source="class B {}class C extends B {value: number=0;constructor(public p: number) {const Key=class {[0]: any=0;};super();\n throw 'after-hoist';}}new C(0);";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions {
            use_define_for_class_fields: Some(false),
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
    assert_eq!(
        (location.filename.as_str(), location.line, location.column),
        (ENTRY, 2, 2)
    );
}

#[test]
fn lowered_class_keys_and_initializers_keep_utf16_and_line_terminators() {
    for define in [true, false] {
        for newline in ["", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
            for body in [
                format!("value: any=0 + ({newline} absent.field);"),
                format!("[{newline} absent.field]: any=0;"),
            ] {
                let source = format!(
                    "const face: string='😀';const absent: any=null;class C {{{body}}}new C();"
                );
                let expected = if newline.is_empty() {
                    (
                        1,
                        source[..source.rfind("absent.field").unwrap()]
                            .encode_utf16()
                            .count() as u32
                            + 1,
                    )
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
                assert_eq!(
                    (location.filename.as_str(), location.line, location.column),
                    (ENTRY, expected.0, expected.1)
                );
            }
        }
    }
}

#[test]
fn lowered_class_module_dependencies_retain_key_field_and_decorator_origins_after_await() {
    for define in [true, false] {
        for awaited in [false, true] {
            let prefix = if awaited { "await 0;" } else { "" };
            for source in [
                format!("export const absent: any=null;{prefix}export class C {{value: any=0 + (\n absent.field);}}new C();"),
                format!("export const absent: any=null;{prefix}export class C {{[\n absent.field]: any=0;}}new C();"),
                format!("export const marker=0;{prefix}const Native: any=WeakMap;\n@Native class C {{}}"),
                format!("export const marker=0;{prefix}const Factory: any=(globalThis as any).Proxy;const dec: any=new Factory(WeakMap,{{apply:0}});\n@dec class C {{}}"),
            ] {
                let graph = compile_direct_module_graph(
                    GRAPH_ENTRY,
                    &MapLoader::from([
                        ModuleSource::new(GRAPH_ENTRY, "import './dep.ts';"),
                        ModuleSource::new("graph/dep.ts", &source),
                    ]),
                    CompilerOptions {
                        use_define_for_class_fields: Some(define),
                        ..CompilerOptions::default()
                    },
                )
                .unwrap();
                let (mut runtime, origin) = runtime();
                let attachment = graph
                    .attach_in_page_realm(&mut runtime, 7, &origin)
                    .unwrap();
                assert!(attachment.execute_in_page_realm(&mut runtime, 7).is_err());
                runtime.run_microtask_checkpoint(7).unwrap();
                let messages = runtime.drain_console(7).unwrap();
                assert_eq!(messages.records.len(), 1, "{messages:?}");
                let location = messages.records[0].source.as_ref().unwrap();
                assert_eq!(
                    (location.filename.as_str(), location.line, location.column),
                    ("graph/dep.ts", 2, 2)
                );
            }
        }
    }
}

#[test]
fn queued_assignment_field_origins_survive_later_code_and_expire_on_navigation() {
    for navigate in [false, true] {
        let source="const absent: any=null;function fail(): void {const C=class {value: any=0 + (\n absent.field);};new C();}queueMicrotask(fail);";
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions {
                use_define_for_class_fields: Some(false),
                ambient_declaration_modules: vec![ModuleSource::new(
                    "memory:///queue.d.ts",
                    "declare function queueMicrotask(callback: () => void): void;",
                )],
                ..CompilerOptions::default()
            },
        )
        .unwrap();
        let (mut runtime, origin) = runtime();
        runtime
            .configure_realm_bindings(7, |host| host.install_host_timers())
            .unwrap();
        let attachment = artifact
            .attach_in_page_realm(&mut runtime, 7, &origin)
            .unwrap();
        runtime.execute_program(7, attachment.handle).unwrap();
        let later = runtime
            .install_program(
                7,
                &origin,
                bluejs::BlueJsSourceIdentity::new("memory:///later.js", "sha256:later").unwrap(),
                &bluejs::BlueJsProgramV1::Script(bluejs::parse("0;").unwrap()),
            )
            .unwrap();
        runtime.execute_program(7, later).unwrap();
        if navigate {
            runtime.navigate(7, origin.clone()).unwrap();
        }
        runtime.run_task_turn(7, 0, 64).unwrap();
        let messages = runtime.drain_console(7).unwrap();
        if navigate {
            assert!(messages.records.is_empty(), "{messages:?}");
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
