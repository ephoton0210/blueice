// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::{MapLoader, ModuleSource};

#[test]
fn public_jsx_compilation_restores_the_previous_context() {
    let _scope = JsxScope::enter(JsxContext {
        mode: None,
        factory: "outerFactory".to_string(),
        fragment: "outerFragment".to_string(),
        automatic: false,
        experimental_decorators: false,
        define_class_fields: true,
    });
    let program = "/** @jsx h */\n\
        declare namespace JSX { interface Element {} interface IntrinsicElements { div: any } }\n\
        function h(_tag: any, props: any, ...children: any[]): any {\n\
            return props.n + children[0];\n\
        }\n\
        const result: any = <div n={(2 + 3)}>{[7, 8][1]}</div>;\nresult;\n";
    let artifact = compile_direct_script(
        "memory:///context.tsx",
        &MapLoader::from([ModuleSource::new("memory:///context.tsx", program)]),
        CompilerOptions {
            jsx: Some(JsxMode::React),
            ..CompilerOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
        bluejs::Value::Number(13.0)
    );
    let restored = current().expect("the caller's context is restored");
    assert!(restored.mode.is_none());
    assert_eq!(restored.factory, "outerFactory");
    assert_eq!(restored.fragment, "outerFragment");
}

#[test]
fn jsx_lowerer_refuses_missing_and_disabled_context_before_parsing() {
    let token = Token {
        kind: blueice_bluets::TokenKind::JsxElement,
        text: "<div />".to_string(),
        start: 0,
        end: 7,
    };
    let mut lowerer = ExpressionLowerer::new("memory:///missing.tsx", &[]);
    let missing = lowerer.lower_jsx_element(&token).unwrap_err();
    assert!(format!("{missing:?}").contains("JSX needs the `jsx` option"));

    let _scope = JsxScope::enter(JsxContext {
        mode: None,
        factory: "React.createElement".to_string(),
        fragment: "React.Fragment".to_string(),
        automatic: false,
        experimental_decorators: false,
        define_class_fields: true,
    });
    let disabled = lowerer.lower_jsx_element(&token).unwrap_err();
    assert!(format!("{disabled:?}").contains("cannot use JSX unless"));
}

#[test]
fn jsx_lowerer_reports_missing_and_truncated_synthetic_groups_at_the_element() {
    let token = Token {
        kind: blueice_bluets::TokenKind::JsxElement,
        text: "<div />".to_string(),
        start: 0,
        end: 7,
    };
    let _scope = JsxScope::enter(JsxContext {
        mode: Some(JsxMode::React),
        factory: "h".to_string(),
        fragment: "Fr".to_string(),
        automatic: false,
        experimental_decorators: false,
        define_class_fields: true,
    });
    for (tokens, expected) in [
        (vec![], "a JSX element is missing its expression group"),
        (
            vec![Token {
                kind: blueice_bluets::TokenKind::Punct,
                text: "(".to_string(),
                start: 7,
                end: 7,
            }],
            "a JSX element's expression group is unterminated",
        ),
    ] {
        let mut lowerer = ExpressionLowerer::new("memory:///truncated.tsx", &tokens);
        let error = lowerer.lower_jsx_element(&token).unwrap_err();
        let BridgeError::UnsupportedRuntimeTarget { span, message } = error else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(message, expected);
        assert_eq!(span, SourceSpan::new("memory:///truncated.tsx", 0, 7));
    }
}
