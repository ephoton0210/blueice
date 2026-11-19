// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assertion semantics and owner mutation policy meet at the public boundary.

use blueice_bluets::{
    compile, compile_program, CheckingOptions, CompilerOptions, Diagnostic, DiagnosticCode,
    EcmaTarget, JavaScriptLayout, ModuleKind, ModuleLoader, ModuleSource, Severity, SourceSpan,
    Type,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::{cell::RefCell, process::Command};

const MAIN: &str = "memory:///main.ts";

fn input() -> Value {
    serde_json::from_str(include_str!("fixtures/assertion_results/cases.json")).unwrap()
}

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/assertion_results/reference.json")).unwrap()
}

/// Only the authored fixture graph is available; resolution never reads a file.
struct FixtureLoader(BTreeMap<String, String>);

impl FixtureLoader {
    fn new(entry: &Value) -> Self {
        let mut files = BTreeMap::from([(
            "main.ts".to_owned(),
            entry["source"].as_str().unwrap().to_owned(),
        )]);
        if let Some(extra) = entry["files"].as_object() {
            files.extend(
                extra
                    .iter()
                    .map(|(name, text)| (name.to_owned(), text.as_str().unwrap().to_owned())),
            );
        }
        Self(files)
    }
}

impl ModuleLoader for FixtureLoader {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.0
            .get(id.strip_prefix("memory:///").ok_or("foreign fixture id")?)
            .map(|source| ModuleSource::new(id, source))
            .ok_or_else(|| format!("unowned fixture source {id}"))
    }

    fn resolve(&self, _from: &str, specifier: &str) -> Result<String, String> {
        let name = specifier.strip_prefix("./").ok_or("foreign fixture edge")?;
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("fixture edge escapes the frozen graph".into());
        }
        let stem = name.strip_suffix(".js").unwrap_or(name);
        [
            name.to_owned(),
            format!("{stem}.ts"),
            format!("{stem}.d.ts"),
        ]
        .into_iter()
        .find(|name| self.0.contains_key(name))
        .map(|name| format!("memory:///{name}"))
        .ok_or_else(|| format!("unowned fixture edge {specifier}"))
    }
}

fn options(profile: &Value) -> CompilerOptions {
    CompilerOptions {
        target: if profile["compilerOptions"]["target"] == "ES5" {
            EcmaTarget::Es5
        } else {
            EcmaTarget::Es2022
        },
        module_kind: if profile["compilerOptions"]["module"] == "CommonJS" {
            ModuleKind::CommonJs
        } else {
            ModuleKind::Esm
        },
        libraries: Some(vec![EcmaTarget::Es5]),
        checking: Some(CheckingOptions {
            strict_null_checks: profile["compilerOptions"]["strictNullChecks"] == true,
            ..CheckingOptions::default()
        }),
        javascript_layout: JavaScriptLayout::TypeScript,
        always_strict: true,
        declaration: true,
        declaration_map: true,
        source_map: true,
        inline_sources: false,
        resolver_fingerprint: "assertion-results-owned-fixture-v1".into(),
        ..CompilerOptions::default()
    }
}

fn description(code: u32, message: &str, span: &SourceSpan, loader: &FixtureLoader) -> Value {
    let name = span
        .module
        .strip_prefix("memory:///")
        .unwrap_or(&span.module);
    let Some(source) = loader.0.get(name) else {
        return json!({"unexpectedModule":name,"code":code,"message":message});
    };
    let before = &source[..span.start];
    json!({
        "code":code,"message":message,"file":name,
        "position":{
            "line":before.bytes().filter(|byte| *byte == b'\n').count()+1,
            "column":before.rsplit('\n').next().unwrap().encode_utf16().count()+1,
            "length":source[span.start..span.end].encode_utf16().count(),
        },
        "relatedInformation":[],
    })
}

fn diagnostic(diagnostic: &Diagnostic, loader: &FixtureLoader) -> Value {
    let Some(native) = &diagnostic.typescript else {
        return json!({"unmatchedBlueDiagnostic":diagnostic.to_json()});
    };
    let mut value = description(native.code, &native.message, &native.span, loader);
    value["relatedInformation"] = json!(native
        .related_information
        .iter()
        .map(|row| description(row.code, &row.message, &row.span, loader))
        .collect::<Vec<_>>());
    value
}

#[test]
fn assertion_semantic_results_match_the_original_native_matrix() {
    let input = input();
    let reference = reference();
    let mut differences = Vec::new();
    let mut observations = 0;
    for wanted in reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "semantic" && row["representation"] == "original")
    {
        let entry = input["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == wanted["form"])
            .unwrap();
        let profile = input["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|profile| profile["id"] == wanted["profile"])
            .unwrap();
        let loader = FixtureLoader::new(entry);
        let result = compile(MAIN, &loader, options(profile));
        let diagnostics: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .map(|row| diagnostic(row, &loader))
            .collect();
        if result.output.is_some() != wanted["accepts"].as_bool().unwrap()
            || json!(diagnostics) != wanted["diagnostics"]
        {
            differences.push(format!(
                "{}: output={}, actual={diagnostics:?}, Native={}",
                wanted["id"],
                result.output.is_some(),
                wanted["diagnostics"]
            ));
        }
        observations += 1;
    }
    assert_eq!(observations, 136);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn unknown_asserted_export_exposes_the_semantic_number_type() {
    let input = input();
    let entry = input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "unknown-cast-inferred-result")
        .unwrap();
    let result = compile(
        MAIN,
        &FixtureLoader::new(entry),
        options(&input["profiles"][0]),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let symbol = result.checked.as_ref().unwrap().modules[MAIN]
        .symbols
        .iter()
        .find(|symbol| symbol.name == "result")
        .unwrap();
    assert_eq!(symbol.value_type, Some(Type::Number));
}

#[test]
fn assertion_runtime_origins_refuse_at_the_authored_mutation() {
    let input = input();
    let sites: Value = serde_json::from_str(include_str!(
        "fixtures/assertion_results/mutation-sites.json"
    ))
    .unwrap();
    let mut differences = Vec::new();
    let mut observations = 0;
    for entry in input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["role"] == "owner")
    {
        let expected = entry["bluePolicyExpectedAccepts"].as_bool().unwrap();
        let mutation = &sites["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == entry["id"])
            .unwrap()["sites"][0];
        for profile in input["profiles"].as_array().unwrap() {
            for legacy in [false, true] {
                let mut options = options(profile);
                if legacy {
                    options.checking = None;
                }
                let result = compile(MAIN, &FixtureLoader::new(entry), options);
                let policy_at_mutation = |row: &Diagnostic| {
                    row.severity == Severity::Error
                        && (row.message.contains("readonly") || row.message.contains("opaque"))
                        && row.span.module == MAIN
                        && row.span.start < mutation["endByte"].as_u64().unwrap() as usize
                        && row.span.end > mutation["startByte"].as_u64().unwrap() as usize
                };
                let correct = if expected {
                    result.output.is_some() && !result.has_errors()
                } else {
                    result.output.is_none()
                        && result.diagnostics.iter().any(policy_at_mutation)
                        && result
                            .diagnostics
                            .iter()
                            .filter(|row| row.severity == Severity::Error)
                            .all(policy_at_mutation)
                };
                if !correct {
                    differences.push(format!(
                        "{} {} legacy={legacy}: output={}, diagnostics={:?}",
                        entry["id"],
                        profile["id"],
                        result.output.is_some(),
                        result.diagnostics
                    ));
                }
                observations += 1;
            }
        }
    }
    assert_eq!(observations, 160);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn asserted_records_remain_structural_generic_inference_sources() {
    let source = "function read<T>(value: {value: T}): T { return value.value; }\n\
        declare const hidden: unknown;\n\
        export const result = read(hidden as {value: number});\n";
    let result = compile(
        MAIN,
        &FixtureLoader::new(&json!({"source":source})),
        options(&input()["profiles"][0]),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let symbol = result.checked.as_ref().unwrap().modules[MAIN]
        .symbols
        .iter()
        .find(|symbol| symbol.name == "result")
        .unwrap();
    assert_eq!(symbol.value_type, Some(Type::Number));
}

#[test]
fn annotations_forwarding_and_candidate_joins_cannot_clear_runtime_origins() {
    let cases = [
        ("known-readonly-annotated-result", "function cast(value: Event): {value: number} { return value as {value: number}; }\n", "cast(value)"),
        ("forward-declaration", "function forward(value: unknown): {value: number} { return cast(value); }\nfunction cast(value: unknown): {value: number} { return value as {value: number}; }\n", "forward(value)"),
        ("annotated-callable-alias", "function cast<T>(value: unknown): T { return value as T; }\nconst alias: <T>(value: unknown) => T = cast;\n", "alias<{value: number}>(value)"),
        ("covariant-candidate-join", "function cast<T>(value: unknown): T { return value as T; }\nfunction choose<T>(first: T, second: T): T { return first; }\n", "choose({value: 0}, cast<{value: number}>(value))"),
        ("conditional-candidate-join", "declare const flag: boolean;\nfunction cast<T>(value: unknown): T { return value as T; }\n", "flag ? cast<{value: number}>(value) : {value: 0}"),
        ("named-callable-alias", "function cast<T>(value: unknown): T { return value as T; }\ntype Cast = <T>(value: unknown) => T;\nconst alias: Cast = cast;\n", "alias<{value: number}>(value)"),
    ];
    let mut differences = Vec::new();
    for (name, functions, expression) in cases {
        let source = format!("interface Event {{ readonly value: number; }}\n{functions}function write(value: Event): void {{ const mutable: {{value: number}} = {expression}; mutable.value = 2; }}\nexport const result = 0;\n");
        let start = source.find("mutable.value = 2").unwrap();
        for legacy in [false, true] {
            let mut options = options(&input()["profiles"][0]);
            if legacy {
                options.checking = None;
            }
            let result = compile(
                MAIN,
                &FixtureLoader::new(&json!({"source":source})),
                options,
            );
            let at_mutation = |row: &Diagnostic| {
                row.severity == Severity::Error
                    && (row.message.contains("readonly") || row.message.contains("opaque"))
                    && row.span.module == MAIN
                    && row.span.start <= start
                    && row.span.end >= start + "mutable.value".len()
            };
            if result.output.is_some()
                || !result.diagnostics.iter().any(at_mutation)
                || !result
                    .diagnostics
                    .iter()
                    .filter(|row| row.severity == Severity::Error)
                    .all(at_mutation)
            {
                differences.push(format!(
                    "{name} legacy={legacy}: output={}, {:?}",
                    result.output.is_some(),
                    result.diagnostics
                ));
            }
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn assertion_results_keep_the_configured_expansion_limit() {
    let input = input();
    let mut policy = options(&input["profiles"][0]);
    policy.limits.max_type_expansions = 0;
    let result = compile(MAIN, &FixtureLoader::new(&input["cases"][0]), policy);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|row| row.code == DiagnosticCode::ResourceLimit));
}

#[test]
fn optional_reads_keep_nullable_semantics_and_runtime_origins() {
    let positive = [
        "function read(): {value: number} | undefined { return {get value(): number {return 42;}}; }\nexport const result = read()?.value ?? 0;\n",
        "function read(): {value: number} | undefined { return {get value(): number {return 42;}}; }\nexport const result = (read()?.value ?? 0) + 0;\n",
        "type Result = {value: number} | undefined;\nfunction read(): Result { return {get value(): number {return 42;}}; }\nexport const result = read()?.value ?? 0;\n",
        "interface Event {readonly value: number;}\nfunction cast(value: unknown): {value: number} {return value as {value: number};}\nfunction write(event: Event, cast: (value: number) => {value: number}): void {const mutable = cast(1); mutable.value = 2;}\nexport const result = 0 + 0;\n",
    ];
    for source in positive {
        let result = compile(
            MAIN,
            &FixtureLoader::new(&json!({"source": source})),
            options(&input()["profiles"][0]),
        );
        assert!(!result.has_errors(), "{source}\n{:?}", result.diagnostics);
        let declaration = result.output.unwrap().artifacts[MAIN]
            .declaration
            .clone()
            .unwrap();
        assert!(
            declaration.contains("export declare const result: number;"),
            "{declaration}"
        );
    }
    let source = "interface Event {readonly child: {readonly value: number};}\nfunction cast(value: unknown): {child: {value: number}} | undefined {return value as {child: {value: number}} | undefined;}\nfunction write(value: Event): void {const mutable = cast(value)?.child ?? {value: 0}; mutable.value = 2;}\nexport const result = 0;\n";
    let mutation = source.find("mutable.value = 2").unwrap();
    for legacy in [false, true] {
        let mut policy = options(&input()["profiles"][0]);
        if legacy {
            policy.checking = None;
        }
        let result = compile(
            MAIN,
            &FixtureLoader::new(&json!({"source": source})),
            policy,
        );
        let errors = result
            .diagnostics
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .collect::<Vec<_>>();
        assert!(
            result.output.is_none() && !errors.is_empty(),
            "legacy={legacy}: {:?}",
            result.diagnostics
        );
        assert!(
            errors.iter().all(|row| row.span.start <= mutation
                && row.span.end >= mutation + "mutable.value".len()
                && (row.message.contains("readonly") || row.message.contains("opaque"))),
            "legacy={legacy}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn qualified_operations_keep_native_semantics() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/assertion_results/operations.json")).unwrap();
    let mut differences = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let result = compile(
            MAIN,
            &FixtureLoader::new(case),
            CompilerOptions {
                target: EcmaTarget::Es2022,
                libraries: Some(vec![EcmaTarget::Es2022]),
                checking: Some(CheckingOptions::default()),
                declaration: true,
                ..CompilerOptions::default()
            },
        );
        let declaration = result
            .output
            .as_ref()
            .and_then(|output| output.artifacts[MAIN].declaration.as_deref());
        let codes = result
            .diagnostics
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .filter_map(|row| row.typescript.as_ref().map(|row| row.code))
            .collect::<Vec<_>>();
        if result.output.is_some() != case["accepts"].as_bool().unwrap()
            || declaration != case["declaration"].as_str()
            || json!(codes) != case["diagnosticCodes"]
        {
            differences.push(format!(
                "{}: declaration={declaration:?}, diagnostics={:?}, expected={case}",
                case["id"], result.diagnostics
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn generic_assertion_results_reject_readonly_container_writes_at_the_mutation() {
    for assertion in ["value as T", "value as unknown as T"] {
        for (target, protected) in [
            ("number[]", "readonly number[]"),
            ("[number]", "readonly [number]"),
        ] {
            let source = format!("declare const protectedValue: {protected};\nfunction cast<T>(value: unknown): T {{return {assertion};}}\nconst mutable = cast<{target}>(protectedValue);\nmutable[0] = 2;\n");
            let mutation = source.find("mutable[0] = 2").unwrap();
            for legacy in [false, true] {
                let result = compile(
                    MAIN,
                    &FixtureLoader::new(&json!({"source":source})),
                    CompilerOptions {
                        target: EcmaTarget::Es2022,
                        checking: (!legacy).then(CheckingOptions::default),
                        declaration: true,
                        ..CompilerOptions::default()
                    },
                );
                let errors = result
                    .diagnostics
                    .iter()
                    .filter(|row| row.severity == Severity::Error)
                    .collect::<Vec<_>>();
                assert!(
                    result.output.is_none() && !errors.is_empty(),
                    "legacy={legacy}, {assertion}, {protected}: {:?}",
                    result.diagnostics
                );
                assert!(
                    errors.iter().all(|row| row.span.start <= mutation
                        && row.span.end >= mutation + "mutable[0]".len()
                        && (row.message.contains("readonly") || row.message.contains("opaque"))),
                    "legacy={legacy}, {assertion}, {protected}: {:?}",
                    result.diagnostics
                );
            }
        }
    }
    for source in [
        "declare const hidden: unknown;\nfunction cast<T>(value: unknown): T {return value as T;}\nconst mutable = cast<number[]>(hidden);\nmutable[0] = 2;\n",
        "declare const protectedValue: readonly number[];\ndeclare const explicit: any;\nexplicit[0] = 2;\n",
    ] {
        for legacy in [false, true] {
            let result = compile(
                MAIN,
                &FixtureLoader::new(&json!({"source":source})),
                CompilerOptions {
                    target: EcmaTarget::Es2022,
                    checking: (!legacy).then(CheckingOptions::default),
                    declaration: true,
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{source}: {:?}", result.diagnostics);
            assert!(result.output.is_some());
        }
    }
}

#[test]
fn qualified_operations_preserve_owner_mutation_denials() {
    let expressions = [
        "(hidden as {read(): {value: number}}).read()",
        "(hidden as [{value: number}])[0]",
        "(hidden as {value: number} | undefined) ?? {value: 0}",
        "(hidden as undefined) ?? {value: 0}",
        "(hidden as {value: number}) || {value: 0}",
        "(hidden as {value: number}) && {value: 0}",
        "(hidden as () => {value: number}).call(undefined)",
        "(hidden as () => {value: number}).bind(undefined)()",
    ];
    let mut differences = Vec::new();
    for expression in expressions {
        let source = format!("declare const hidden: unknown;\ndeclare const readonlyOwner: {{readonly value: number}};\nconst mutable = {expression};\nmutable.value = 2;\n");
        let mutation = source.find("mutable.value = 2").unwrap();
        for legacy in [false, true] {
            let result = compile(
                MAIN,
                &FixtureLoader::new(&json!({"source":source})),
                CompilerOptions {
                    target: EcmaTarget::Es2022,
                    checking: (!legacy).then(CheckingOptions::default),
                    declaration: true,
                    ..CompilerOptions::default()
                },
            );
            let errors = result
                .diagnostics
                .iter()
                .filter(|row| row.severity == Severity::Error)
                .collect::<Vec<_>>();
            if result.output.is_some()
                || errors.is_empty()
                || !errors.iter().all(|row| {
                    row.span.start <= mutation
                        && row.span.end >= mutation + "mutable.value".len()
                        && (row.message.contains("readonly") || row.message.contains("opaque"))
                })
            {
                differences.push(format!(
                    "{expression}, legacy={legacy}: {:?}",
                    result.diagnostics
                ));
            }
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn asserted_primitives_keep_arithmetic_results_and_operand_errors() {
    let source = "declare const hidden: unknown;\nexport const sum = (hidden as number) + 1;\nexport const product = (hidden as number) * 2;\nexport const text = (hidden as string) + '';\n";
    let result = compile(
        MAIN,
        &FixtureLoader::new(&json!({"source": source})),
        options(&input()["profiles"][0]),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let declaration = result.output.unwrap().artifacts[MAIN]
        .declaration
        .clone()
        .unwrap();
    assert_eq!(declaration, "export declare const sum: number;\nexport declare const product: number;\nexport declare const text: string;\n");
    let source =
        "declare const hidden: unknown;\nexport const invalid = (hidden as number) * 'wrong';\n";
    let result = compile(
        MAIN,
        &FixtureLoader::new(&json!({"source": source})),
        options(&input()["profiles"][0]),
    );
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|row| row.typescript.as_ref().is_some_and(|row| row.code == 2363)),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn assertion_results_keep_the_total_source_budget() {
    let input = input();
    let mut policy = options(&input["profiles"][0]);
    policy.limits.max_total_source_bytes = input["cases"][0]["source"].as_str().unwrap().len() - 1;
    let result = compile(MAIN, &FixtureLoader::new(&input["cases"][0]), policy);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|row| row.code == DiagnosticCode::ResourceLimit));
}

#[test]
fn class_constructor_recursion_uses_the_public_semantic_identity() {
    let source = include_str!(
        "fixtures/typescript_oracle/dynamic-declarations-expression-static-self/main.ts"
    );
    let result = compile(
        MAIN,
        &FixtureLoader::new(&json!({"source": source})),
        CompilerOptions {
            target: EcmaTarget::Es2022,
            declaration: true,
            checking: Some(CheckingOptions::default()),
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.output.unwrap().artifacts[MAIN].declaration.as_deref().unwrap(),
        "export declare const C: {\n    new (): {\n        value: number;\n    };\n    read(): /*elided*/ any;\n};\n");
}

#[test]
fn assertion_results_preserve_static_no_emit_authority() {
    let input = input();
    let mut policy = options(&input["profiles"][0]);
    policy.no_emit = true;
    let result = compile(MAIN, &FixtureLoader::new(&input["cases"][0]), policy);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.checked.is_some());
    assert!(result.output.is_none());
}

#[test]
fn qualified_async_and_generator_results_keep_their_protocols() {
    let source = "declare const hidden: unknown;\nexport class C { async read(): Promise<number> {return hidden as number;} *iterate(): Generator<number, number, unknown> {yield 1; return hidden as number;} }\n";
    let policy = CompilerOptions {
        target: EcmaTarget::Es2022,
        checking: Some(CheckingOptions::default()),
        declaration: true,
        ..CompilerOptions::default()
    };
    let result = compile(
        MAIN,
        &FixtureLoader::new(&json!({"source":source})),
        policy.clone(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let declaration = result.output.unwrap().artifacts[MAIN]
        .declaration
        .clone()
        .unwrap();
    assert!(
        declaration.contains("read(): Promise<number>;"),
        "{declaration}"
    );
    assert!(
        declaration.contains("iterate(): Generator<number, number, unknown>;"),
        "{declaration}"
    );

    let source = "interface Event {readonly value: number;}\nasync function cast(value: unknown): Promise<{value: number}> {return value as {value: number};}\nasync function write(value: Event): Promise<void> {const mutable = await cast(value); mutable.value = 2;}\n";
    let mutation = source.find("mutable.value = 2").unwrap();
    for legacy in [false, true] {
        let mut policy = policy.clone();
        if legacy {
            policy.checking = None;
        }
        let result = compile(MAIN, &FixtureLoader::new(&json!({"source":source})), policy);
        let errors = result
            .diagnostics
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .collect::<Vec<_>>();
        assert!(
            result.output.is_none() && !errors.is_empty(),
            "legacy={legacy}: {:?}",
            result.diagnostics
        );
        assert!(
            errors.iter().all(|row| row.span.start <= mutation
                && row.span.end >= mutation + "mutable.value".len()
                && (row.message.contains("readonly") || row.message.contains("opaque"))),
            "legacy={legacy}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn commonjs_binding_spelling_honors_the_selected_javascript_layout() {
    let loader = FixtureLoader::new(
        &json!({"source":"import {read} from './factory.js';\nexport const result = read();\n", "files":{"factory.ts":"export function read(): number {return 1;}\n"}}),
    );
    for (layout, expected) in [
        (JavaScriptLayout::PreserveSource, "factory_1"),
        (JavaScriptLayout::TypeScript, "factory_js_1"),
    ] {
        let result = compile(
            MAIN,
            &loader,
            CompilerOptions {
                target: EcmaTarget::Es2022,
                module_kind: ModuleKind::CommonJs,
                javascript_layout: layout,
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let javascript = &result.output.unwrap().artifacts[MAIN].javascript;
        assert!(
            javascript.contains(&format!("const {expected} = require(\"./factory.js\");")),
            "{layout:?}: {javascript}"
        );
    }
}

#[test]
fn assertion_results_do_not_load_an_unselected_owner_source() {
    struct ObservedLoader {
        loader: FixtureLoader,
        loads: RefCell<Vec<String>>,
    }
    impl ModuleLoader for ObservedLoader {
        fn load(&self, id: &str) -> Result<ModuleSource, String> {
            self.loads.borrow_mut().push(id.to_owned());
            self.loader.load(id)
        }
        fn resolve(&self, from: &str, specifier: &str) -> Result<String, String> {
            self.loader.resolve(from, specifier)
        }
    }
    let input = input();
    let entry = input["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "imported-unknown-function-literal")
        .unwrap();
    let mut fixture = FixtureLoader::new(entry);
    fixture.0.insert(
        "outside.ts".into(),
        "export const unselected: number = 'invalid';\n".into(),
    );
    let loader = ObservedLoader {
        loader: fixture,
        loads: RefCell::new(Vec::new()),
    };
    let result = compile_program(&[MAIN.to_owned()], &loader, options(&input["profiles"][0]));
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(loader
        .loads
        .borrow()
        .iter()
        .any(|id| id == "memory:///factory.ts"));
    assert!(!loader
        .loads
        .borrow()
        .iter()
        .any(|id| id == "memory:///outside.ts"));
    assert!(!result.project.modules.contains_key("memory:///outside.ts"));
}

#[test]
#[ignore = "requires pinned TypeScript/Acorn and the entire frozen assertion producer/consumer/runtime contract"]
fn assertion_results_match_the_complete_native_contract() {
    let compiler = std::env::var_os("BLUEICE_ASSERTION_RESULTS_COMPILER")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/verify_assertion_results.cjs");
    let output = Command::new("node")
        .arg(script)
        .arg(compiler)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
