// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::JsxMode;

const TSX: &str = "memory:///origins.tsx";
const PREFIX: &str = "declare namespace JSX {interface Element {} interface IntrinsicElements {div: any;span: any;}}function h(tag: any,props: any,...children: any[]): any {return 0;}const Fr: any=0;const absent: any=null;const tools: any=null;";

fn options() -> CompilerOptions {
    CompilerOptions {
        jsx: Some(JsxMode::React),
        jsx_factory: Some("h".into()),
        jsx_fragment_factory: Some("Fr".into()),
        ..CompilerOptions::default()
    }
}

fn console(source: &str, options: CompilerOptions) -> bluejs::HostConsoleBatch {
    let artifact = compile_direct_script(
        TSX,
        &MapLoader::from([ModuleSource::new(TSX, source)]),
        options,
    )
    .unwrap();
    let (mut runtime, origin) = runtime();
    let attachment = artifact
        .attach_in_page_realm(&mut runtime, 7, &origin)
        .unwrap();
    assert!(
        runtime.execute_program(7, attachment.handle).is_err(),
        "{source}"
    );
    runtime.drain_console(7).unwrap()
}

fn runtime() -> (bluejs::BlueJsPageRuntime, bluejs::BlueJsPageOrigin) {
    let origin = bluejs::BlueJsPageOrigin::new("https://example.test").unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin.clone()).unwrap();
    runtime
        .configure_realm_bindings(7, |host| {
            host.install_web_file_api()?;
            host.install_window_aliases()?;
            host.install_host_console()?;
            host.install_host_error_reporting()?;
            host.install_host_timers()
        })
        .unwrap();
    (runtime, origin)
}

#[test]
fn direct_jsx_user_expressions_keep_their_own_original_sources() {
    let mut failures = Vec::new();
    for (name, body) in [
        (
            "attribute",
            "const value: any=<div field={\n absent.field} />;",
        ),
        ("child", "const value: any=<div>{\n absent.field}</div>;"),
        ("fragment child", "const value: any=<>{\n absent.field}</>;"),
        ("member tag", "const value: any=<\n tools.Item />;"),
        ("commented tag", "const value: any=<//🧊\n tools.Item />;"),
        (
            "block comment tag",
            "const value: any=</*🧊*/\n tools.Item />;",
        ),
        (
            "nested element",
            "const value: any=<div><span field={\n absent.field} /></div>;",
        ),
        (
            "element attribute",
            "const value: any=<div child=<span field={\n absent.field} /> />;",
        ),
        (
            "embedded element",
            "const value: any=<div>{<span field={\n absent.field} />}</div>;",
        ),
        (
            "attribute spread operand",
            "const value: any=<div {...(\n absent.field)} />;",
        ),
        (
            "child spread operand",
            "const value: any=<div>{...(\n absent.field)}</div>;",
        ),
        (
            "template child",
            "const value: any=<div>{`value:${\n absent.field}`}</div>;",
        ),
        (
            "later attribute",
            "const value: any=<div first={0} second={\n absent.field} />;",
        ),
        (
            "later child",
            "const value: any=<div>{0}{\n absent.field}</div>;",
        ),
    ] {
        let source = format!("{PREFIX}{body}");
        let messages = console(&source, options());
        assert_eq!(messages.records.len(), 1, "{name}: {messages:?}");
        let location = messages.records[0]
            .source
            .as_ref()
            .map(|source| (source.filename.as_str(), source.line, source.column));
        if location != Some((TSX, 2, 2)) {
            failures.push((
                name,
                location.map(|(file, line, column)| (file.to_owned(), line, column)),
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn generated_jsx_factory_calls_cannot_borrow_the_element_location() {
    let source="declare namespace JSX {interface Element {} interface IntrinsicElements {div: any;}}const h: any=0;const value: any=<div />;";
    let messages = console(source, options());
    assert_eq!(messages.records.len(), 1, "{messages:?}");
    assert!(messages.records[0].source.is_none(), "{messages:?}");
}

#[test]
fn jsx_synthetic_factory_member_and_fragment_lookups_remain_unavailable() {
    for (factory, fragment, body) in [
        ("tools.h", "Fr", "<div />"),
        ("h", "tools.Fragment", "<></>"),
    ] {
        let source = format!("{PREFIX}const value: any={body};");
        let messages = console(
            &source,
            CompilerOptions {
                jsx_factory: Some(factory.into()),
                jsx_fragment_factory: Some(fragment.into()),
                ..options()
            },
        );
        assert_eq!(messages.records.len(), 1, "{messages:?}");
        assert!(messages.records[0].source.is_none(), "{messages:?}");
    }
}

#[test]
fn jsx_embedded_dynamic_function_failures_keep_unavailable_sources() {
    let source=format!("{PREFIX}const dynamic: any=Function(\"throw 'dynamic'\");const value: any=<div>{{dynamic()}}</div>;");
    let messages = console(
        &source,
        CompilerOptions {
            ambient_declaration_modules: vec![ModuleSource::new(
                "memory:///dynamic.d.ts",
                "declare function Function(source: string): any;",
            )],
            ..options()
        },
    );
    assert_eq!(messages.records.len(), 1, "{messages:?}");
    assert!(messages.records[0].source.is_none(), "{messages:?}");
}

#[test]
fn jsx_user_callbacks_keep_their_sources_below_generated_calls() {
    for source in [
        "declare namespace JSX {interface Element {} interface IntrinsicElements {div: any;}}function h(tag: any,props: any): any {\n throw 'factory-body';}const value: any=<div />;".to_owned(),
        format!("{PREFIX}function fail(): number {{\n throw 'getter-body';}}const fields: any={{}};Object.defineProperty(fields,'value',{{'get':fail,enumerable:true}});const value: any=<div {{...fields}} />;"),
    ] {
        let messages=console(&source,options());
        assert_eq!(messages.records.len(),1,"{messages:?}");
        let location=messages.records[0].source.as_ref().unwrap();
        assert_eq!((location.filename.as_str(),location.line,location.column),(TSX,2,2),"{messages:?}");
    }
}

#[test]
fn wide_jsx_children_share_the_origin_capture_budget() {
    let module = blueice_bluets::parse_module(TSX, "").unwrap();
    let _context = JsxScope::enter(JsxContext::of(&options(), &module));
    // Exercise the source producer independently of full compiler admission.
    let source = format!("<div>{}</div>", "{0}".repeat(50_000));
    let mut tokens = lex(TSX, &source).unwrap();
    tokens.pop();
    assert!(ExpressionLowerer::new(TSX, &tokens).parse().is_ok());
    assert!(
        matches!(ExpressionLowerer::new(TSX,&tokens).parse_with_origins(),Err(BridgeError::ProvenanceAttachment(message)) if message.contains("byte budget"))
    );
}

#[test]
fn direct_jsx_module_dependencies_preserve_origins_before_and_after_await() {
    for awaited in [false, true] {
        let source = format!(
            "{PREFIX}{}export const value: any=<div>{{\n absent.field}}</div>;",
            if awaited { "await 0;" } else { "" }
        );
        let graph = compile_direct_module_graph(
            "graph/main.tsx",
            &MapLoader::from([
                ModuleSource::new("graph/main.tsx", "import './dep.tsx';"),
                ModuleSource::new("graph/dep.tsx", &source),
            ]),
            options(),
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
            ("graph/dep.tsx", 2, 2)
        );
    }
}

#[test]
fn jsx_queued_callbacks_keep_origins_after_later_code_and_discard_them_on_navigation() {
    for navigate in [false, true] {
        let source=format!("{PREFIX}function fail(): void {{const value: any=<div>{{\n absent.field}}</div>;}}queueMicrotask(fail);");
        let artifact = compile_direct_script(
            TSX,
            &MapLoader::from([ModuleSource::new(TSX, &source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ModuleSource::new(
                    "memory:///queue.d.ts",
                    "declare function queueMicrotask(callback: () => void): void;",
                )],
                ..options()
            },
        )
        .unwrap();
        let (mut runtime, origin) = runtime();
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
                (TSX, 2, 2)
            );
        }
    }
}

#[test]
fn jsx_sources_preserve_utf16_and_original_line_terminators() {
    for newline in ["", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        for body in [
            format!("const value: any=<div title='😀'>{{{newline} absent.field}}</div>;"),
            format!("const value: any=<div><{newline} tools.Item /></div>;"),
        ] {
            let source = format!("{PREFIX}{body}");
            let marker = if body.contains("absent.field") {
                "absent.field"
            } else {
                "tools.Item"
            };
            let expected = if newline.is_empty() {
                (
                    1,
                    source[..source.rfind(marker).unwrap()]
                        .encode_utf16()
                        .count() as u32
                        + 1,
                )
            } else {
                (2, 2)
            };
            let messages = console(&source, options());
            assert_eq!(messages.records.len(), 1, "{messages:?}");
            let location = messages.records[0].source.as_ref().unwrap();
            assert_eq!(
                (location.filename.as_str(), location.line, location.column),
                (TSX, expected.0, expected.1),
                "{source}"
            );
        }
    }
}

#[test]
fn jsx_origin_trees_preserve_duplicate_order_ast_and_instruction_bytes() {
    let module = blueice_bluets::parse_module(TSX, "").unwrap();
    let _context = JsxScope::enter(JsxContext::of(&options(), &module));
    for source in [
        "<div a={x} b={x}>{x}{x}</div>",
        "<Tools.Item a={x}><span>{x}</span></Tools.Item>",
        "<><div>{x}</div>{x}</>",
        "<div a=<span>{x}</span> title='label' flag />",
        "[<div>{x}</div>,x]",
        "<div>{<span>{x}</span>}</div>",
    ] {
        let mut tokens = lex(TSX, source).unwrap();
        tokens.pop();
        let ordinary = ExpressionLowerer::new(TSX, &tokens).parse().unwrap();
        let (expression, origins) = ExpressionLowerer::new(TSX, &tokens)
            .parse_with_origins()
            .unwrap();
        assert_eq!(expression, ordinary, "{source}");
        let mut positions = Vec::new();
        origins
            .bind(&expression, &mut |node, offset| {
                if let bluejs::Expr::Identifier(name) = node {
                    if name == "x" {
                        positions.push(offset);
                    }
                }
            })
            .unwrap();
        assert_eq!(
            positions,
            source
                .match_indices('x')
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>(),
            "{source}"
        );
        let program = |expr| {
            bluejs::BlueJsProgramV1::Script(bluejs::Program {
                body: vec![bluejs::Stmt::Expr(expr)],
            })
        };
        let ordinary = program(ordinary).compile().unwrap();
        let collected = program(expression).compile().unwrap();
        assert_eq!(ordinary.bytes(), collected.bytes(), "{source}");
        assert_eq!(ordinary.constants(), collected.constants(), "{source}");
    }
}
