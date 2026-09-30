// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespaces (J.3.5): parsing, typing, merging and emitted text. Verdicts on
//! real programs are pinned against TypeScript by the `namespace-*` matrix
//! fixtures and the runtime oracle; these tests pin the structure, codes and
//! messages.

use blueice_bluets::{parse_module, Declaration, NamespaceDeclaration};

fn namespaces(source: &str) -> Vec<NamespaceDeclaration> {
    parse_module("memory:///main.ts", source)
        .expect("the source parses")
        .declarations
        .into_iter()
        .filter_map(|declaration| match declaration {
            Declaration::Namespace(namespace) => Some(namespace),
            _ => None,
        })
        .collect()
}

#[test]
fn a_namespace_keeps_its_body_declarations_in_source_order() {
    let found = namespaces(
        "namespace N { export const a = 1; function hidden() {} export interface I { x: number } }",
    );
    assert_eq!(found.len(), 1);
    let namespace = &found[0];
    assert_eq!(namespace.name, "N");
    assert!(!namespace.exported && !namespace.declared && !namespace.implicit);
    let kinds = namespace
        .body
        .iter()
        .map(|declaration| match declaration {
            Declaration::Variable(variable) => ("variable", variable.exported),
            Declaration::Function(function) => ("function", function.exported),
            Declaration::Interface(interface) => ("interface", interface.exported),
            other => panic!("unexpected {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [("variable", true), ("function", false), ("interface", true)]
    );
}

#[test]
fn a_dotted_name_is_one_namespace_per_segment_each_exported_from_its_parent() {
    let found = namespaces("export namespace A.B.C { export const x = 1; }");
    let outer = &found[0];
    assert_eq!(
        (outer.name.as_str(), outer.exported, outer.implicit),
        ("A", true, false)
    );
    let Declaration::Namespace(middle) = &outer.body[0] else {
        panic!("expected B");
    };
    assert_eq!(
        (middle.name.as_str(), middle.exported, middle.implicit),
        ("B", true, true)
    );
    let Declaration::Namespace(inner) = &middle.body[0] else {
        panic!("expected C");
    };
    assert_eq!((inner.name.as_str(), inner.implicit), ("C", true));
    assert!(matches!(&inner.body[0], Declaration::Variable(variable) if variable.name == "x"));
}

#[test]
fn the_module_keyword_declares_a_namespace_and_an_ambient_body_is_ambient() {
    let found = namespaces("declare module M { function f(): void; const v: number; }");
    let namespace = &found[0];
    assert!(namespace.declared);
    assert!(namespace.body.iter().all(|declaration| match declaration {
        Declaration::Function(function) => function.declared,
        Declaration::Variable(variable) => variable.declared,
        _ => false,
    }));
    assert!(namespace.exports_every_member());
    let explicit =
        namespaces("declare namespace M { export function f(): void; function g(): void; }");
    assert!(!explicit[0].exports_every_member());
}

#[test]
fn nested_namespaces_spans_and_header_cover_the_written_text() {
    let source = "export namespace Outer { namespace Inner { export let v = 2; } }";
    let found = namespaces(source);
    let outer = &found[0];
    assert_eq!(
        &source[outer.header_span.start..outer.header_span.end],
        "export namespace Outer {"
    );
    assert_eq!(
        &source[outer.closing_span.start..outer.closing_span.end],
        "}"
    );
    assert_eq!(outer.span.end, source.len());
    let Declaration::Namespace(inner) = &outer.body[0] else {
        panic!("expected Inner");
    };
    assert_eq!(&source[inner.name_span.start..inner.name_span.end], "Inner");
}

#[test]
fn a_keyword_not_followed_by_a_name_on_the_same_line_is_not_a_namespace() {
    // `namespace` then a newline is an expression statement in TypeScript, so it is
    // not read as a declaration header (BlueTS still refuses the identifier itself).
    let error = parse_module("memory:///main.ts", "namespace\nN { }").expect_err("not a namespace");
    assert!(error
        .iter()
        .all(|diagnostic| !diagnostic.message.contains("namespace body")));
}

#[test]
fn what_a_namespace_body_cannot_hold_is_refused() {
    for source in [
        "namespace N { export default 1; }",
        "namespace N { import a from './a'; }",
        "namespace N { export { a }; }",
    ] {
        let error = parse_module("memory:///main.ts", source).expect_err(source);
        assert!(
            error
                .iter()
                .any(|diagnostic| diagnostic.message.contains("inside a namespace body")),
            "{source}: {error:?}"
        );
    }
    let error = parse_module(
        "memory:///main.ts",
        "declare namespace N { declare const x: number; }",
    )
    .expect_err("a redundant declare");
    assert!(error[0].message.contains("already ambient"));
    let error =
        parse_module("memory:///main.ts", "namespace N { const x = 1;").expect_err("unterminated");
    assert!(error[0].message.contains("unterminated namespace body"));
}
