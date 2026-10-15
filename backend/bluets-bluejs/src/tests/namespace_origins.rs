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
fn namespace_and_enum_descendants_keep_original_expression_sources() {
    let mut failures = Vec::new();
    for define in [true, false] {
        for (name, source) in [
            ("namespace export", "const absent: any=null;namespace N {export const value: any=1+(\n absent.field);}"),
            ("namespace local", "const absent: any=null;namespace N {const value: any=1+(\n absent.field);}"),
            ("namespace expression", "const absent: any=null;namespace N {1+(\n absent.field);}"),
            ("namespace function", "namespace N {export function fail(): void {\n throw 'namespace';}}N.fail();"),
            ("namespace default", "const absent: any=null;namespace N {export function fail(value: any=1+(\n absent.field)): void {}}N.fail();"),
            ("namespace getter", "namespace N {export class C {get value(): number {\n throw 'getter';}}}new N.C().value;"),
            ("namespace field", "const absent: any=null;namespace N {export class C {value: any=1+(\n absent.field);}}new N.C();"),
            ("namespace computed field", "const absent: any=null;namespace N {export class C {[\n absent.field]: any=0;}}"),
            ("namespace computed method", "const absent: any=null;namespace N {export class C {[\n absent.field](): void {}}}"),
            ("namespace rewritten field", "namespace N {export const absent: any=null;export class C {value: any=1+(\n absent.field);}}new N.C();"),
            ("nested namespace", "namespace N {export namespace Deep {export function fail(): void {\n throw 'nested';}}}N.Deep.fail();"),
            ("dotted namespace", "namespace N.Deep {export function fail(): void {\n throw 'dotted';}}N.Deep.fail();"),
            ("merged namespace", "namespace N {export const absent: any=null;}namespace N {export function fail(): any {return 1+(\n absent.field);}}N.fail();"),
            ("outer namespace reference", "namespace N {export const absent: any=null;export namespace Deep {export function fail(): any {return 1+(\n absent.field);}}}N.Deep.fail();"),
            ("rewritten argument", "namespace N {export const absent: any=null;function read(value: any): void {}export function fail(): void {read(\n absent.field);}}N.fail();"),
            ("enum initializer", "const absent: any=null;enum E {A=0,B=(\n absent.field)}"),
            ("namespace enum", "const absent: any=null;namespace N {export enum E {A=0,B=(\n absent.field)}}"),
            ("rewritten enum", "namespace N {export const absent: any=null;export enum E {A=0,B=(\n absent.field)}}"),
            ("merged enum", "const absent: any=null;enum E {A=0}enum E {B=(\n absent.field)}"),
            ("function namespace merge", "const absent: any=null;function N(): void {}namespace N {export const value: any=1+(\n absent.field);}"),
            ("enum namespace merge", "const absent: any=null;enum N {A=0}namespace N {export const value: any=1+(\n absent.field);}"),
        ] {
            let artifact = compile_direct_script(ENTRY, &MapLoader::from([ModuleSource::new(ENTRY, source)]), CompilerOptions {
                use_define_for_class_fields: Some(define), ..CompilerOptions::default()
            }).unwrap_or_else(|error| panic!("{name}: {error:?}"));
            let (mut runtime, origin) = runtime();
            let attachment = artifact.attach_in_page_realm(&mut runtime, 7, &origin).unwrap();
            assert!(runtime.execute_program(7, attachment.handle).is_err(), "{name}");
            let messages = runtime.drain_console(7).unwrap();
            assert_eq!(messages.records.len(), 1, "{name}: {messages:?}");
            let actual = messages.records[0].source.as_ref().map(|location| (location.filename.as_str(), location.line, location.column));
            if actual != Some((ENTRY, 2, 2)) { failures.push((name, define, format!("{actual:?}"))); }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn namespace_paths_preserve_normal_ast_and_bytecode() {
    use lowering::namespaces::{NamespaceLowering, NamespaceScope};
    for define in [true, false] {
        for source in [
            "namespace N {export const x: number=1;export function f(): number {return x;}}",
            "namespace N.Deep.More {export enum E {A=0,B=2}}",
            "namespace N {export namespace Deep {export class C {value: any=1;}}}",
            "function N(): void {}namespace N {export let x: number=1;}",
            "enum N {A=0}namespace N {export let x: number=1;}",
            "namespace N {export const x: number=1;}namespace N {export function f(): number {return x;}}",
        ] {
            let module = blueice_bluets::parse_module(ENTRY, source).unwrap();
            let _context = JsxScope::enter(JsxContext::of(&CompilerOptions {use_define_for_class_fields: Some(define), ..CompilerOptions::default()}, &module));
            let tokens = lex(ENTRY, source).unwrap();
            let lowerer = NamespaceLowering {module: &module, tokens: &tokens, exports: blueice_bluets::NamespaceExports::of(&module), define_class_fields: define};
            let mut declared = BTreeSet::new();
            for declaration in &module.declarations {
                match declaration {
                    Declaration::Namespace(namespace) => {
                        let mut captured_names = declared.clone();
                        let ordinary = lowerer.namespace(namespace, &NamespaceScope::Module, &mut declared, "", &BTreeMap::new()).unwrap();
                        let (captured, _) = lowerer.namespace_with_origins(namespace, &mut captured_names).unwrap();
                        assert_eq!(captured, ordinary, "{source}");
                        assert_eq!(captured_names, declared);
                        let program = |body| bluejs::BlueJsProgramV1::Script(bluejs::Program {body});
                        let ordinary = program(ordinary).compile().unwrap();
                        let captured = program(captured).compile().unwrap();
                        assert_eq!(ordinary.bytes(), captured.bytes());
                        assert_eq!(ordinary.constants(), captured.constants());
                    }
                    Declaration::Function(function) => {declared.insert(function.name.clone());}
                    Declaration::Enum(declaration) => {declared.insert(declaration.name.clone());}
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn generated_namespace_exports_enum_writes_and_dynamic_code_keep_unavailable_sources() {
    for source in [
        "'use strict';const ObjectApi: any=Object;const value: any=ObjectApi.freeze({});const global: any=globalThis;global.N=value;namespace N {export const x: any=1;}",
        "'use strict';const ObjectApi: any=Object;const value: any=ObjectApi.freeze({});const global: any=globalThis;global.E=value;enum E {A=1}",
        "namespace N {export const run: any=Function(\"throw 'dynamic';\");}N.run();",
    ] {
        let artifact = compile_direct_script(ENTRY, &MapLoader::from([ModuleSource::new(ENTRY, source)]), CompilerOptions::default()).unwrap();
        let (mut runtime, origin) = runtime();
        let attachment = artifact.attach_in_page_realm(&mut runtime, 7, &origin).unwrap();
        assert!(runtime.execute_program(7, attachment.handle).is_err(), "{source}");
        let messages = runtime.drain_console(7).unwrap();
        assert_eq!(messages.records.len(), 1, "{messages:?}");
        assert!(messages.records[0].source.is_none(), "{source}: {messages:?}");
    }
}

#[test]
fn constant_enum_nan_infinity_and_negative_zero_still_compile_and_bind() {
    for source in [
        "enum E {A=0/0,B=-0,C=1/0}E.B;",
        "namespace N {export enum E {A=0/0,B=-0,C=1/0}}N.E.B;",
    ] {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        let (mut runtime, origin) = runtime();
        let attachment = artifact
            .attach_in_page_realm(&mut runtime, 7, &origin)
            .unwrap();
        runtime.execute_program(7, attachment.handle).unwrap();
        assert!(runtime.drain_console(7).unwrap().records.is_empty());
    }
}

#[test]
fn namespace_capture_bounds_paths_independently_of_executable_slots() {
    use lowering::namespaces::{NamespaceLowering, NamespaceScope};
    let module = blueice_bluets::parse_module(ENTRY, "namespace N {let value: any;}").unwrap();
    let Declaration::Namespace(mut namespace) = module.declarations[0].clone() else {
        unreachable!()
    };
    namespace.body = vec![namespace.body[0].clone(); 25_000];
    let tokens = lex(ENTRY, &module.source).unwrap();
    let lowerer = NamespaceLowering {
        module: &module,
        tokens: &tokens,
        exports: blueice_bluets::NamespaceExports::of(&module),
        define_class_fields: true,
    };
    assert!(lowerer
        .namespace(
            &namespace,
            &NamespaceScope::Module,
            &mut BTreeSet::new(),
            "",
            &BTreeMap::new()
        )
        .is_ok());
    assert!(
        matches!(lowerer.namespace_with_origins(&namespace, &mut BTreeSet::new()), Err(BridgeError::ProvenanceAttachment(message)) if message.contains("byte budget"))
    );
}

#[test]
fn direct_enum_admission_compares_nan_payloads_and_zero_sign_bits() {
    fn change_value(program: &mut bluejs::BlueJsProgramV1) {
        let bluejs::BlueJsProgramV1::Script(program) = program else {
            unreachable!()
        };
        let bluejs::Stmt::VarDecl(_, declarations) = &mut program.body[0] else {
            unreachable!()
        };
        let Some(bluejs::Expr::Call { callee, .. }) = &mut declarations[0].init else {
            unreachable!()
        };
        let bluejs::Expr::Parenthesized(callee) = callee.as_mut() else {
            unreachable!()
        };
        let bluejs::Expr::Function(function) = callee.as_mut() else {
            unreachable!()
        };
        let bluejs::Stmt::Expr(bluejs::Expr::Assign { target, .. }) = &mut function.body[0] else {
            unreachable!()
        };
        let bluejs::Expr::Member { property, .. } = target.as_mut() else {
            unreachable!()
        };
        let bluejs::Expr::Assign { value, .. } = property.as_mut() else {
            unreachable!()
        };
        let bluejs::Expr::Number(number) = value.as_mut() else {
            unreachable!()
        };
        *number = if number.is_nan() {
            f64::from_bits(number.to_bits() ^ 1)
        } else {
            0.0
        };
    }
    for source in ["enum E {A=0/0}", "enum E {A=-0}"] {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            CompilerOptions::default(),
        )
        .unwrap();
        let mut mutated = artifact.program.clone();
        change_value(&mut mutated);
        let mut changed_code = artifact.clone();
        changed_code.bytecode = mutated.compile().unwrap();
        let (mut runtime, origin) = runtime();
        assert!(
            matches!(changed_code.attach_in_page_realm(&mut runtime, 7, &origin), Err(BridgeError::ProvenanceAttachment(message)) if message.contains("bytecode"))
        );
        assert!(runtime.program_handles(7).unwrap().is_empty());
        let mut changed_program = artifact;
        changed_program.bytecode = mutated.compile().unwrap();
        changed_program.program = mutated;
        assert!(changed_program
            .attach_in_page_realm(&mut runtime, 7, &origin)
            .is_err());
        assert!(runtime.program_handles(7).unwrap().is_empty());
    }
}

#[test]
fn namespace_and_enum_dependency_sources_survive_top_level_await() {
    for define in [true, false] {
        for awaited in [false, true] {
            let prefix = if awaited { "await 0;" } else { "" };
            for body in [
                "export namespace N {export const absent: any=null;export const value: any=1+(\n absent.field);}",
                "export const absent: any=null;export enum E {A=(\n absent.field)}",
                "export namespace N.Deep {export function fail(): void {\n throw 'nested';}}N.Deep.fail();",
                "export namespace N {export const absent: any=null;}export namespace N {export function fail(): any {return 1+(\n absent.field);}}N.fail();",
            ] {
                let source = format!("{prefix}{body}");
                let graph = compile_direct_module_graph(GRAPH_ENTRY, &MapLoader::from([
                    ModuleSource::new(GRAPH_ENTRY, "import './dep.ts';"), ModuleSource::new("graph/dep.ts", &source)
                ]), CompilerOptions {use_define_for_class_fields: Some(define), ..CompilerOptions::default()}).unwrap();
                let (mut runtime, origin) = runtime();
                let attachment = graph.attach_in_page_realm(&mut runtime, 7, &origin).unwrap();
                assert!(attachment.execute_in_page_realm(&mut runtime, 7).is_err());
                runtime.run_microtask_checkpoint(7).unwrap();
                let messages = runtime.drain_console(7).unwrap();
                assert_eq!(messages.records.len(), 1, "{source}: {messages:?}");
                let location = messages.records[0].source.as_ref().unwrap();
                assert_eq!((location.filename.as_str(), location.line, location.column), ("graph/dep.ts", 2, 2));
            }
        }
    }
}

#[test]
fn namespace_and_enum_sources_keep_utf16_columns_and_original_newlines() {
    for newline in ["", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        for declaration in ["namespace N {export const value: any=1+(", "enum E {A=("] {
            let source = format!(
                "const face='😀';const absent: any=null;{declaration}{newline} absent.field)}}"
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
            assert_eq!((location.line, location.column), expected, "{source:?}");
        }
    }
}

#[test]
fn queued_namespace_sources_survive_major_gc_and_expire_on_navigation() {
    for navigate in [false, true] {
        let source = "namespace N {export function fail(): void {\n throw 'queued';}}queueMicrotask(N.fail);";
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
            assert_eq!(runtime.realm_stats(7).unwrap().program_count, 0);
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
fn rewritten_namespace_receivers_keep_gaps_while_original_reads_are_mapped() {
    use bluejs::diagnostics::{visit_diagnostic_source_nodes, DiagnosticSourceNode};
    let source =
        "namespace N {export const x: any=null;export function fail(): any {return x.field;}}";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        CompilerOptions::default(),
    )
    .unwrap();
    let mut receivers = BTreeSet::new();
    let mut reads = BTreeSet::new();
    visit_diagnostic_source_nodes(&artifact.program, &mut |node, ordinal| match node {
        DiagnosticSourceNode::Expression(bluejs::Expr::Identifier(name)) if name == "N" => {
            receivers.insert(ordinal as u32);
        }
        DiagnosticSourceNode::Expression(bluejs::Expr::Member {
            object, property, ..
        }) if matches!(object.as_ref(), bluejs::Expr::Identifier(name) if name == "N")
            && matches!(property.as_ref(), bluejs::Expr::Identifier(name) if name == "x") =>
        {
            reads.insert(ordinal as u32);
        }
        _ => {}
    });
    let mut registry = bluejs::BlueJsProgramRegistry::default();
    let handle = registry
        .install_for_diagnostics(
            bluejs::BlueJsSourceIdentity::new(ENTRY, &artifact.sources[0].content_hash).unwrap(),
            &artifact.program,
        )
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
        receivers: &BTreeSet<u32>,
        reads: &BTreeSet<u32>,
        column: u32,
        counts: &mut (usize, usize),
    ) {
        for owner in before.diagnostic_instruction_owners() {
            let Some(node) = owner.ast_node_ordinal else {
                continue;
            };
            let location = after.diagnostic_source_location(owner.bytecode_offset);
            if receivers.contains(&node) {
                assert!(location.is_none(), "{location:?}");
                counts.0 += 1;
            }
            if reads.contains(&node) {
                if let Some(location) = location {
                    assert_eq!((location.line, location.column), (1, column));
                    counts.1 += 1;
                }
            }
        }
        for (before, after) in before.child_code_units().zip(after.child_code_units()) {
            verify(before, after, receivers, reads, column, counts);
        }
    }
    let mut counts = (0, 0);
    verify(
        &before,
        registry.get(handle).unwrap().bytecode(),
        &receivers,
        &reads,
        source.rfind("x.field").unwrap() as u32 + 1,
        &mut counts,
    );
    assert!(counts.0 > 0 && counts.1 > 0, "{counts:?}");
}

#[test]
fn namespace_capture_bounds_reference_maps_even_without_initializer_slots() {
    use lowering::namespaces::{NamespaceLowering, NamespaceScope};
    let name = format!("N{}", "x".repeat(2500));
    let body = (0..1000)
        .map(|index| format!("export let value{index}: any;"))
        .collect::<String>();
    let source = format!("namespace {name} {{{body}}}");
    let module = blueice_bluets::parse_module(ENTRY, &source).unwrap();
    let Declaration::Namespace(namespace) = &module.declarations[0] else {
        unreachable!()
    };
    let tokens = lex(ENTRY, &source).unwrap();
    let lowerer = NamespaceLowering {
        module: &module,
        tokens: &tokens,
        exports: blueice_bluets::NamespaceExports::of(&module),
        define_class_fields: true,
    };
    let ordinary = lowerer
        .namespace(
            namespace,
            &NamespaceScope::Module,
            &mut BTreeSet::new(),
            "",
            &BTreeMap::new(),
        )
        .unwrap();
    let program = bluejs::BlueJsProgramV1::Script(bluejs::Program { body: ordinary });
    assert!(bluejs::diagnostics::visit_diagnostic_source_nodes(&program, &mut |_, _| {}) < 32);
    assert!(
        matches!(lowerer.namespace_with_origins(namespace, &mut BTreeSet::new()), Err(BridgeError::ProvenanceAttachment(message)) if message.contains("byte budget"))
    );
}
