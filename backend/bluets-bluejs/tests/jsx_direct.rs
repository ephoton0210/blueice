// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct execution of classic JSX (J.5.2): the program is lowered by the direct
//! bridge and run by BlueJS with its own factory, and the JavaScript pinned `tsc`
//! emits for the same program is run by Node; the completion value must agree.
//! The automatic runtime, preserved JSX and a missing option are refused, naming
//! what to do instead.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use blueice_bluejs as bluejs;
use blueice_bluets::{CompilerOptions, JsxMode, MapLoader, ModuleSource};
use blueice_bluets_bluejs::compile_direct_script;

/// The pragmas must be in the file's leading comments, before any declaration.
const PRAGMAS: &str = "/** @jsx h */\n/** @jsxFrag Fr */\n";
const NAMESPACE: &str = "declare namespace JSX { interface Element { n?: number } interface IntrinsicElements { div: any; span: any; i: any } }\n";
const FACTORY: &str = "function h(tag: any, props: any, ...c: any[]): any { let total: number = 1 + c.length; if (props !== null) { if (props.n !== undefined) { total = total + props.n; } } let i: number = 0; while (i < c.length) { if (typeof c[i] === \"object\") { total = total + 0; } i = i + 1; } return total; }\nconst Fr: any = \"fragment\";\n";

/// Each program completes with a number: its last expression statement.
const BODIES: &[(&str, &str)] = &[
    (
        "attributes, children and nesting",
        "const a: any = <div n={5}>a{3}<span /></div>;\na;",
    ),
    (
        "a fragment, whose factory is the fragment name",
        "const a: any = <><i /><i /></>;\na;",
    ),
    (
        "spread attributes",
        "const o: any = { n: 2 };\nconst a: any = <div {...o} />;\na;",
    ),
    (
        "text trimming and entities count as one child each",
        "const a: any = <div>\n  one\n  two\n  &lt;&amp;\n</div>;\na;",
    ),
    (
        "an element as an attribute value and an expression sequence",
        "const a: any = <div n=<i /> >{(1, 2)}</div>;\na;",
    ),
    (
        "string attributes with entities",
        "const a: any = <div title=\"a &amp; b\" n={2} />;\na;",
    ),
    (
        "nested elements inside expressions",
        "const a: any = <div>{<i />}{<span n={1}>{<i />}</span>}</div>;\na;",
    ),
];

fn options() -> CompilerOptions {
    CompilerOptions {
        jsx: Some(JsxMode::React),
        ..CompilerOptions::default()
    }
}

fn source(body: &str) -> String {
    format!("{PRAGMAS}{NAMESPACE}{FACTORY}{body}\n")
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn the_direct_bridge_runs_classic_jsx_to_the_value_typescript_and_node_compute() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = env::temp_dir().join(format!("bluets-bluejs-jsx-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (index, (name, body)) in BODIES.iter().enumerate() {
        let program = source(body);
        let artifact = compile_direct_script(
            "memory:///direct.tsx",
            &MapLoader::from([ModuleSource::new("memory:///direct.tsx", program.as_str())]),
            options(),
        )
        .unwrap_or_else(|error| panic!("{name}: the bridge refused it: {error:?}"));
        let blue = match bluejs::Vm::default().execute(&artifact.bytecode).unwrap() {
            bluejs::Value::Number(number) => format!("{number}"),
            other => panic!("{name}: unexpected completion value {other:?}"),
        };
        let directory = root.join(index.to_string());
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("program.tsx"), &program).unwrap();
        let emitted = Command::new(&tsc)
            .args([
                "--target", "ES2022", "--jsx", "react", "--pretty", "false", "--outDir",
            ])
            .arg(directory.join("out"))
            .arg(directory.join("program.tsx"))
            .output()
            .unwrap();
        let javascript = fs::read_to_string(directory.join("out").join("program.js"))
            .unwrap_or_else(|_| panic!("{name}: tsc emitted nothing: {emitted:?}"));
        let printed = Command::new(&node)
            .arg("-p")
            .arg(&javascript)
            .output()
            .unwrap();
        assert!(printed.status.success(), "{name}: node failed: {printed:?}");
        let reference = String::from_utf8_lossy(&printed.stdout).trim().to_string();
        assert_eq!(blue, reference, "{name}: BlueJS and Node disagree");
    }
    let _ = fs::remove_dir_all(&root);
}

fn refusal(options: CompilerOptions, body: &str) -> String {
    let program = source(body);
    let error = compile_direct_script(
        "memory:///direct.tsx",
        &MapLoader::from([ModuleSource::new("memory:///direct.tsx", program.as_str())]),
        options,
    )
    .err()
    .expect("the program is refused");
    format!("{error:?}")
}

#[test]
fn automatic_preserved_and_unconfigured_jsx_are_refused_with_the_supported_route() {
    let element = "const a: any = <div />;\na;";
    let automatic = refusal(
        CompilerOptions {
            jsx: Some(JsxMode::ReactJsx),
            ..CompilerOptions::default()
        },
        element,
    );
    assert!(automatic.contains("automatic JSX runtime"), "{automatic}");
    let preserved = refusal(
        CompilerOptions {
            jsx: Some(JsxMode::Preserve),
            ..CompilerOptions::default()
        },
        element,
    );
    assert!(
        preserved.contains("preserved JSX is not executable"),
        "{preserved}"
    );
    let none = refusal(CompilerOptions::default(), element);
    assert!(none.contains("`jsx` option"), "{none}");
}

#[test]
fn a_classic_program_with_its_own_factory_runs_and_the_factory_receives_the_call_shape() {
    let program = source("const a: any = <div n={5}>a{3}<span /></div>;\na;");
    let artifact = compile_direct_script(
        "memory:///direct.tsx",
        &MapLoader::from([ModuleSource::new("memory:///direct.tsx", program.as_str())]),
        options(),
    )
    .unwrap();
    // div: 1 + 3 children + n(5); the span child adds nothing here.
    match bluejs::Vm::default().execute(&artifact.bytecode).unwrap() {
        bluejs::Value::Number(number) => assert_eq!(number, 9.0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_factory_that_is_not_in_the_program_is_refused_not_bound_implicitly() {
    let program = "declare namespace JSX { interface Element {} interface IntrinsicElements { div: any } }\nconst a: any = <div />;\na;\n";
    let error = compile_direct_script(
        "memory:///direct.tsx",
        &MapLoader::from([ModuleSource::new("memory:///direct.tsx", program)]),
        options(),
    )
    .err()
    .expect("no `React` in the program");
    assert!(
        format!("{error:?}").contains("cannot find the name `React`"),
        "{error:?}"
    );
}

#[test]
fn direct_jsx_covers_fragments_spreads_member_tags_and_attribute_elements() {
    let source = |body: &str| {
        format!(
            "/** @jsx h */\n/** @jsxFrag Fr */\n{NAMESPACE}function h(tag: any, props: any, ...c: any[]): any {{ return c.length + (props === null ? 0 : 100); }}\nconst Fr: any = 0;\nconst tools: any = {{ Item: 1 }};\nconst o: any = {{ a: 1 }};\nconst list: any = [1, 2, 3];\n{body}\n"
        )
    };
    let run = |body: &str| {
        let program = source(body);
        let artifact = compile_direct_script(
            "memory:///direct.tsx",
            &MapLoader::from([ModuleSource::new("memory:///direct.tsx", program.as_str())]),
            options(),
        )
        .unwrap_or_else(|error| panic!("{body}: {error:?}"));
        match bluejs::Vm::default().execute(&artifact.bytecode).unwrap() {
            bluejs::Value::Number(number) => number,
            other => panic!("{other:?}"),
        }
    };
    // Children count, plus 100 when props are not null.
    assert_eq!(run("const a: any = <></>;\na;"), 0.0);
    assert_eq!(run("const a: any = <div {...o} />;\na;"), 100.0);
    assert_eq!(run("const a: any = <div>{...list}</div>;\na;"), 3.0);
    assert_eq!(run("const a: any = <tools.Item />;\na;"), 0.0);
    assert_eq!(run("const a: any = <div x=<span /> />;\na;"), 100.0);
    assert_eq!(
        run("const a: any = <div>{/* c */}text<span />  </div>;\na;"),
        3.0
    );
}

#[test]
fn direct_jsx_refuses_a_malformed_element_and_a_react_native_mode() {
    let program = source("const a: any = <div>;\na;");
    assert!(compile_direct_script(
        "memory:///direct.tsx",
        &MapLoader::from([ModuleSource::new("memory:///direct.tsx", program.as_str())]),
        options(),
    )
    .is_err());
    let element = "const a: any = <div />;\na;";
    let native = refusal(
        CompilerOptions {
            jsx: Some(JsxMode::ReactNative),
            ..CompilerOptions::default()
        },
        element,
    );
    assert!(
        native.contains("preserved JSX is not executable"),
        "{native}"
    );
}
