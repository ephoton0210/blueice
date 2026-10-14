// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX parsing and checking through the public compiler (J.5.1): the cases the
//! pinned-`tsc` matrix cannot express, such as the option being off, lexing
//! corners and resource bounds.

use blueice_bluets::{compile, CompilerOptions, JsxMode, MapLoader, ModuleSource};

const PRELUDE: &str = "declare namespace JSX {\n  interface Element { tag: string }\n  interface IntrinsicElements { div: { id?: string; children?: any }; span: { title?: string; children?: any } }\n}\n";

fn check(files: &[(&str, &str)], jsx: Option<JsxMode>) -> Vec<String> {
    let sources: Vec<ModuleSource> = files
        .iter()
        .map(|(file, text)| ModuleSource::new(format!("memory:///{file}"), *text))
        .collect();
    compile(
        &format!("memory:///{}", files[0].0),
        &MapLoader::from(sources),
        CompilerOptions {
            jsx,
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .into_iter()
    .map(|diagnostic| diagnostic.message)
    .collect()
}

fn check_one(body: &str) -> Vec<String> {
    check(
        &[("main.tsx", &format!("{PRELUDE}{body}"))],
        Some(JsxMode::Preserve),
    )
}

#[test]
fn jsx_is_an_error_unless_the_jsx_option_is_set() {
    let source = format!("{PRELUDE}const a = <div />;\n");
    let messages = check(&[("main.tsx", &source)], None);
    assert!(
        messages.iter().any(|m| m.contains("`jsx` option")),
        "{messages:?}"
    );
    assert!(check(&[("main.tsx", &source)], Some(JsxMode::Preserve)).is_empty());
    // A file with no JSX needs no option.
    assert!(check(&[("main.tsx", "const a = 1;\n")], None).is_empty());
}

#[test]
fn a_tsx_module_without_jsx_parses_as_typescript() {
    assert!(check_one("const a: number = 1;\nfunction f<T>(x: T): T { return x; }\n").is_empty());
}

#[test]
fn generic_arrows_and_comparisons_are_not_elements() {
    let messages = check_one(
        "const id = <T,>(x: T): T => x;\nconst id2 = <T extends string>(x: T): T => x;\nconst a = 1;\nconst b = 2;\nconst c = a < b;\nconst d = a <= b;\nconst e = a << 1;\nconst f = <div>{a < b && b > a ? \"y\" : \"n\"}</div>;\n",
    );
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn strings_braces_templates_and_nested_elements_inside_expressions_are_lexed() {
    let messages = check_one(
        "const t = \"}\";\nconst a = <div id={`x${t}y`}>{\"{\"}{t}<span title={\"a>b\"} />{(() => <span />)()}{[1, 2].length}</div>;\n",
    );
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn apostrophes_and_quotes_in_jsx_text_are_text_not_strings() {
    let messages = check_one("const a = <div>don't \"quote\" `tick`</div>;\n");
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn malformed_elements_are_diagnosed_with_a_message() {
    for (body, expected) in [
        ("const a = <div></span>;\n", "does not match"),
        ("const a = <div>;\n", "unterminated"),
        ("const a = <div>a > b</div>;\n", "unexpected `>`"),
        ("const a = <div>}</div>;\n", "unexpected `}`"),
        ("const a = <div id=>;\n", "attribute value"),
        ("const a = <div id={} />;\n", "non-empty expression"),
        ("const a = <div {x} />;\n", "spread"),
    ] {
        let messages = check_one(body);
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{body}: {messages:?}"
        );
    }
}

#[test]
fn an_expression_inside_jsx_is_type_checked() {
    let messages = check_one("const n: number = \"x\";\nconst a = <div id={n}>{n + 1}</div>;\n");
    assert!(!messages.is_empty());
    let messages = check_one("const a = <div>{(x: number): string => x}</div>;\n");
    assert!(
        !messages.is_empty(),
        "an arrow inside JSX is checked: {messages:?}"
    );
}

#[test]
fn deeply_nested_elements_are_bounded_not_a_stack_overflow() {
    let deep = format!("{}{}", "<div>".repeat(500), "</div>".repeat(500));
    let messages = check_one(&format!("const a = {deep};\n"));
    assert!(
        messages.iter().any(|m| m.contains("too deep")),
        "{messages:?}"
    );
    // Nested expression-in-element chains stay linear in time.
    let mut nested = "<div />".to_string();
    for _ in 0..60 {
        nested = format!("<div>{{{nested}}}</div>");
    }
    let started = std::time::Instant::now();
    let messages = check_one(&format!("const a = {nested};\n"));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    let _ = messages;
}

#[test]
fn jsx_pragmas_override_the_factory_for_the_file() {
    let source = format!(
        "/** @jsx el */\n{PRELUDE}declare function el(tag: any, props: any, ...children: any[]): JSX.Element;\nconst a = <div />;\n"
    );
    let messages = check(&[("main.tsx", &source)], Some(JsxMode::React));
    assert!(messages.is_empty(), "{messages:?}");
    let missing = format!("{PRELUDE}const a = <div />;\n");
    let messages = check(&[("main.tsx", &missing)], Some(JsxMode::React));
    assert!(
        messages.iter().any(|m| m.contains("`React`")),
        "{messages:?}"
    );
}
