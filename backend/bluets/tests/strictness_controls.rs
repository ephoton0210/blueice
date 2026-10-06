// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public API controls for independent checking policies and lexical boundaries.

use blueice_bluets::{compile, CheckingOptions, CompilerOptions, MapLoader, ModuleSource};
use serde_json::{json, Value};
use std::{cell::RefCell, env, process::Command};

thread_local! {
    static ORACLE_CASES: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}

fn checked(source: &str, checking: CheckingOptions) -> blueice_bluets::Compilation {
    compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions {
            checking: Some(checking),
            ..CompilerOptions::default()
        },
    )
}

fn accepts(source: &str, options: CheckingOptions, expected: bool) {
    ORACLE_CASES.with_borrow_mut(|cases| {
        if let Some(cases) = cases {
            cases.push(json!({"source":source,"accepts":expected,"options":{
                "target":"es2022", "skipLibCheck":true,
                "noImplicitAny":options.no_implicit_any, "noImplicitThis":options.no_implicit_this,
                "strictNullChecks":options.strict_null_checks, "strictFunctionTypes":options.strict_function_types,
                "strictBindCallApply":options.strict_bind_call_apply, "strictPropertyInitialization":options.strict_property_initialization,
                "strictBuiltinIteratorReturn":options.strict_builtin_iterator_return,"alwaysStrict":options.always_strict,
                "useUnknownInCatchVariables":options.use_unknown_in_catch_variables,
                "noUnusedLocals":options.no_unused_locals,"noUnusedParameters":options.no_unused_parameters,
                "noImplicitReturns":options.no_implicit_returns,"noFallthroughCasesInSwitch":options.no_fallthrough_cases_in_switch,
                "exactOptionalPropertyTypes":options.exact_optional_property_types,"noUncheckedIndexedAccess":options.no_unchecked_indexed_access
            }}));
        }
    });
    let result = checked(source, options);
    assert_eq!(
        !result.has_errors(),
        expected,
        "{source}: {:#?}",
        result.diagnostics
    );
}

#[test]
fn unused_checks_resolve_shadowing_exports_and_underscore_parameters() {
    let options = CheckingOptions {
        no_unused_locals: true,
        no_unused_parameters: true,
        ..CheckingOptions::default()
    };
    accepts(
        "export function read(value: number): number { return value; }",
        options,
        true,
    );
    accepts(
        "export function read(_value: number): number { return 1; }",
        options,
        true,
    );
    accepts(
        "export function read(value: number): number { const value2: number = 1; return value2; }",
        options,
        false,
    );
    accepts("const unused: number = 1;", options, true);
    accepts("export {}; const unused: number = 1;", options, false);
    accepts("export const exposed: number = 1;", options, true);
    accepts(
        "export {}; function unused(): number { return 1; }",
        options,
        false,
    );
    accepts(
        "export {}; interface Unused { value: number; }",
        options,
        false,
    );
    accepts(
        "export {}; type Used = number; const value: Used = 1; console.log(value);",
        options,
        true,
    );
    accepts("export function read(value: number): number { { const value: number = 2; console.log(value); } return 1; }", options, false);
}

#[test]
fn typed_function_methods_keep_argument_and_result_checks() {
    let options = CheckingOptions::default();
    accepts("function read(value: number): number { return value; } const value: number = read.call(undefined, 1);", options, true);
    accepts(
        "function read(value: number): number { return value; } read.call(undefined, 'wrong');",
        options,
        false,
    );
    accepts("function read(value: number): number { return value; } const value: number = read.apply(undefined, [1]);", options, true);
    accepts(
        "function read(value: number): number { return value; } read.apply(undefined, ['wrong']);",
        options,
        false,
    );
    accepts("function read(value: number): number { return value; } const bound = read.bind(undefined, 1); const value: number = bound();", options, true);
    accepts(
        "function read(value: number): number { return value; } read.bind(undefined, 'wrong');",
        options,
        false,
    );
    accepts(
        "const fake: number = 1; fake.call(undefined, 1);",
        CheckingOptions {
            strict_bind_call_apply: false,
            ..options
        },
        false,
    );
}

#[test]
fn return_options_preserve_explicit_and_complete_paths() {
    let options = CheckingOptions {
        no_implicit_returns: true,
        ..CheckingOptions::default()
    };
    accepts(
        "function read(value: boolean) { if (value) { return 1; } else { return 2; } }",
        options,
        true,
    );
    accepts(
        "function read(value: boolean) { if (value) { return 1; } }",
        options,
        false,
    );
    accepts("function read(): void { console.log(1); }", options, true);
    accepts(
        "function read(value: boolean): void { if (value) { return; } }",
        options,
        true,
    );
    accepts(
        "function read(value: boolean): any { if (value) { return 1; } }",
        options,
        true,
    );
    accepts(
        "function read(): number {}",
        CheckingOptions {
            strict_null_checks: false,
            strict_property_initialization: false,
            ..options
        },
        false,
    );
    accepts(
        "function read(value: boolean): number { if (value) { return 1; } }",
        CheckingOptions {
            strict_null_checks: false,
            strict_property_initialization: false,
            no_implicit_returns: false,
            ..options
        },
        true,
    );
}

#[test]
fn catch_annotations_and_unchecked_writes_remain_independent() {
    let options = CheckingOptions {
        use_unknown_in_catch_variables: false,
        no_unchecked_indexed_access: true,
        ..CheckingOptions::default()
    };
    accepts(
        "function read(): void { try { throw 1; } catch (error) { const value: number = error; } }",
        options,
        true,
    );
    accepts("function read(): void { try { throw 1; } catch (error: unknown) { const value: number = error; } }", options, false);
    accepts(
        "const values: number[] = [1]; values[0] = 2;",
        options,
        true,
    );
    accepts(
        "const values: number[] = [1]; values[0] = 'wrong';",
        options,
        false,
    );
    accepts(
        "const values: number[] = [1]; const value: number = values[0];",
        options,
        false,
    );
}

#[test]
fn valid_diagnostic_policies_change_cache_identity_without_changing_javascript() {
    let source = "export function read(value: number): number { return value + 1; }";
    let strict = checked(source, CheckingOptions::default());
    let relaxed = checked(
        source,
        CheckingOptions {
            no_implicit_any: false,
            strict_function_types: false,
            ..CheckingOptions::default()
        },
    );
    assert!(!strict.has_errors() && !relaxed.has_errors());
    let strict = strict.output.unwrap();
    let relaxed = relaxed.output.unwrap();
    assert_ne!(strict.fingerprint, relaxed.fingerprint);
    assert_eq!(
        strict.artifacts["memory:///main.ts"].javascript,
        relaxed.artifacts["memory:///main.ts"].javascript
    );
}

#[test]
fn contextual_and_defaulted_parameters_do_not_become_implicit_any() {
    accepts("function run(callback: (value: number) => number): number { return callback(1); } run(value => value + 1);", CheckingOptions::default(), true);
    accepts(
        "const read: (value: number) => number = value => value + 1;",
        CheckingOptions::default(),
        true,
    );
    accepts(
        "function read(value = 1): number { return value; }",
        CheckingOptions::default(),
        true,
    );
    accepts(
        "function read(value): number { return value; }",
        CheckingOptions::default(),
        false,
    );
}

#[test]
fn switch_cases_distinguish_conditional_and_nested_breaks() {
    let options = CheckingOptions {
        no_fallthrough_cases_in_switch: true,
        ..CheckingOptions::default()
    };
    accepts(
        "function run(value: number): void { switch (value) { case 1: console.log(1); break; case 2: break; } }",
        options,
        true,
    );
    accepts(
        "function run(value: number): void { switch (value) { case 1: case 2: break; } }",
        options,
        true,
    );
    accepts(
        "function run(value: number): void { switch (value) { case 1: while (true) { break; } case 2: break; } }",
        options,
        false,
    );
    accepts(
        "function run(value: number): void { switch (value) { case 1: if (value > 1) { break; } case 2: break; } }",
        options,
        false,
    );
    accepts(
        "function run(value: number): void { switch (value) { case 1: if (value > 1) { break; } else { break; } case 2: break; } }",
        options,
        true,
    );
    accepts(
        "function run(value: number): void { switch (value) { case 1: { console.log(1); break; } case 2: break; } }",
        options,
        true,
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn boundary_controls_match_pinned_typescript() {
    ORACLE_CASES.with_borrow_mut(|cases| *cases = Some(Vec::new()));
    unused_checks_resolve_shadowing_exports_and_underscore_parameters();
    typed_function_methods_keep_argument_and_result_checks();
    return_options_preserve_explicit_and_complete_paths();
    catch_annotations_and_unchecked_writes_remain_independent();
    contextual_and_defaulted_parameters_do_not_become_implicit_any();
    switch_cases_distinguish_conditional_and_nested_breaks();
    let cases = ORACLE_CASES.with_borrow_mut(|cases| cases.take().unwrap());
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE").expect("set BLUEICE_BLUETSC_ORACLE");
    let script = r#"
const ts = require(require('path').resolve(process.argv[1], '../../lib/typescript.js'));
if (ts.version !== '5.9.3') throw Error(ts.version);
for (const row of JSON.parse(process.argv[2])) {
 const converted = ts.convertCompilerOptionsFromJson(row.options, process.cwd());
 const options = converted.options, file = require('path').join(process.cwd(), 'control.ts');
 const host = ts.createCompilerHost(options), get = host.getSourceFile.bind(host);
 host.getSourceFile = (name, ...args) => name === file ? ts.createSourceFile(file, row.source, options.target, true) : get(name, ...args);
 const program = ts.createProgram([file], options, host);
 const diagnostics = [...converted.errors, ...ts.getPreEmitDiagnostics(program)];
 if ((diagnostics.length === 0) !== row.accepts) throw Error(JSON.stringify({source:row.source, diagnostics:diagnostics.map(d => ({code:d.code,message:ts.flattenDiagnosticMessageText(d.messageText, ' ')}))}));
}
"#;
    let output = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(tsc)
        .arg(serde_json::to_string(&cases).unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
