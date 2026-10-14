// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public parser origins used by checking, declaration emission and the bridge.

use blueice_bluets::{
    compile, parse_module, CheckingOptions, CompilerOptions, Declaration, MapLoader, ModuleSource,
    Type,
};

fn checked(source: &str, no_implicit_override: bool) -> blueice_bluets::Compilation {
    compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions {
            checking: Some(CheckingOptions {
                no_implicit_override,
                ..CheckingOptions::default()
            }),
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn override_policy_changes_cache_identity_without_changing_valid_javascript() {
    let source = "export class Item { read(): number { return 1; } }";
    let disabled = checked(source, false);
    let enabled = checked(source, true);
    assert!(!disabled.has_errors() && !enabled.has_errors());
    let disabled = disabled.output.unwrap();
    let enabled = enabled.output.unwrap();
    assert_ne!(disabled.fingerprint, enabled.fingerprint);
    assert_eq!(
        disabled.artifacts["memory:///main.ts"].javascript,
        enabled.artifacts["memory:///main.ts"].javascript
    );
}

#[test]
fn override_policy_is_independent_and_reabstracting_requires_the_modifier() {
    assert!(!CheckingOptions::default().no_implicit_override);
    let source="abstract class B { abstract read(): number; } abstract class C extends B { abstract read(): number; }";
    assert!(!checked(source, false).has_errors());
    let enabled = checked(source, true);
    assert!(enabled.has_errors());
    assert_eq!(
        enabled.diagnostics[0].typescript.as_ref().unwrap().code,
        4116
    );
    let fixed = source.replace(
        "abstract read(): number; }",
        "abstract override read(): number; }",
    );
    // The base introduces the method and therefore has no override keyword.
    let fixed = fixed.replacen("abstract override read()", "abstract read()", 1);
    assert!(!checked(&fixed, true).has_errors());
}

#[test]
fn parameter_property_override_errors_retain_the_entire_declaration() {
    for (source,code,start) in [
        ("class C { constructor(public override value: number) {} }",4112,22),
        ("class B {} class C extends B { constructor(public override value: number) { super(); } }",4113,43),
    ] {
        let result=checked(source,true);
        let primary=result.diagnostics[0].typescript.as_ref().unwrap();
        assert_eq!(primary.code,code,"{:?}",result.diagnostics);
        assert_eq!(primary.span.start,start);
        assert_eq!(primary.span.end-start,29);
    }
    let source="abstract class B { abstract value: number; } class C extends B { constructor(public value: number) { super(); } }";
    assert!(!checked(source, true).has_errors());
}

#[test]
fn abstract_members_and_structural_obligations_retain_their_origins() {
    let source="interface Shape { value: number; } abstract class Base implements Shape { abstract readonly value: number; protected abstract read?(): number; abstract get current(): number; abstract set current(value: number); } class Item extends Base { override value = 3; override read(): number { return 3; } }";
    let module = parse_module("model.ts", source).unwrap();
    let Declaration::Class(base) = &module.declarations[1] else {
        panic!("class")
    };
    let origin = base.abstract_modifier.as_ref().unwrap();
    assert_eq!(&source[origin.start..origin.end], "abstract");
    assert_eq!(base.implements.len(), 1);
    let (Type::Named { name, .. }, span) = &base.implements[0] else {
        panic!("named obligation")
    };
    assert_eq!(name, "Shape");
    assert_eq!(&source[span.start..span.end], "Shape");
    assert!(base
        .members
        .iter()
        .all(|member| member.abstract_modifier.is_some()));
    assert!(base.members[1].method.as_ref().unwrap().optional);
    assert!(base
        .accessor_methods()
        .iter()
        .all(|method| method.body.is_none()));
    let Declaration::Class(item) = &module.declarations[2] else {
        panic!("class")
    };
    assert!(item
        .members
        .iter()
        .all(|member| member.override_modifier.is_some()));
}

#[test]
fn constructor_arrow_types_preserve_abstractness_and_parameters() {
    let module = parse_module(
        "constructors.ts",
        "type A = abstract new (value: number) => { value: number }; type B = new () => number;",
    )
    .unwrap();
    for (index, abstract_constructor) in [true, false].into_iter().enumerate() {
        let Declaration::TypeAlias(alias) = &module.declarations[index] else {
            panic!("type alias")
        };
        let Type::CallableRecord { fields, signatures } = &alias.value else {
            panic!("constructor signature")
        };
        assert!(fields.is_empty());
        assert_eq!(signatures.len(), 1);
        assert!(signatures[0].construct);
        assert!(signatures[0].constructor_arrow);
        assert_eq!(signatures[0].abstract_constructor, abstract_constructor);
        assert_eq!(
            signatures[0].parameters.len(),
            usize::from(abstract_constructor)
        );
    }
}

#[test]
fn grammar_errors_keep_typescript_codes_before_semantic_checking() {
    for (source, code) in [
        ("class C { abstract read(): number; }", 1244),
        (
            "abstract class C { private abstract read(): number; }",
            1243,
        ),
        (
            "abstract class C { abstract read(): number { return 1; } }",
            1245,
        ),
        (
            "abstract class C { abstract get value(): number { return 1; } }",
            1318,
        ),
        ("abstract class C { abstract value: number = 1; }", 1267),
        (
            "class C { override protected read(): number { return 1; } }",
            1029,
        ),
        ("class C { override constructor() {} }", 1089),
    ] {
        let diagnostics = parse_module("grammar.ts", source).unwrap_err();
        assert_eq!(
            diagnostics[0].typescript.as_ref().unwrap().code,
            code,
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn constructor_value_aliases_preserve_pinned_capabilities() {
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/typescript_oracle/modifiers-constructor-carriers.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "5.9.3");
    for case in reference["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([
                ModuleSource::new("memory:///main.ts", source),
                ModuleSource::new(
                    "memory:///base.ts",
                    "export class B { constructor(public value: number) {} }",
                ),
            ]),
            CompilerOptions {
                checking: Some(CheckingOptions::default()),
                ..CompilerOptions::default()
            },
        );
        let actual = result.diagnostics.iter().filter(|diagnostic| diagnostic.severity == blueice_bluets::Severity::Error)
            .map(|diagnostic| {
                let counterpart = diagnostic.typescript.as_ref().unwrap_or_else(|| panic!("{}: {diagnostic:?}", case["name"]));
                serde_json::json!({"code":counterpart.code,"message":counterpart.message,"start":counterpart.span.start,"length":counterpart.span.end-counterpart.span.start})
            }).collect::<Vec<_>>();
        assert_eq!(
            actual,
            *case["diagnostics"].as_array().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE pointing to TypeScript 5.9.3"]
fn constructor_capability_reference_matches_pinned_typescript() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_constructor_carriers.cjs",
    );
    let output = std::process::Command::new(
        std::env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into()),
    )
    .arg(script)
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
