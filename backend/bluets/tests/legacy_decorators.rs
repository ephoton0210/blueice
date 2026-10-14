// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Legacy decorators and decorator metadata through the public compiler (J.5.4):
//! the emitted shape, the refusals, the options and the CLI. Behavior parity with
//! pinned TypeScript under Node is in `legacy_decorators_oracle.rs`.

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

const DECL: &str = "declare function d(...args: any[]): any;\n";

fn legacy() -> CompilerOptions {
    CompilerOptions {
        experimental_decorators: true,
        ..CompilerOptions::default()
    }
}

fn metadata() -> CompilerOptions {
    CompilerOptions {
        emit_decorator_metadata: true,
        ..legacy()
    }
}

fn build(source: &str, options: CompilerOptions) -> Result<String, Vec<String>> {
    let compiled = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        options,
    );
    match compiled.output {
        Some(output) => Ok(output.artifacts["memory:///main.ts"].javascript.clone()),
        None => Err(compiled
            .diagnostics
            .into_iter()
            .map(|d| d.message)
            .collect()),
    }
}

#[test]
fn a_decorated_class_is_defined_then_decorated_by_calls_after_it() {
    let javascript = build(
        &format!("{DECL}@d class C {{ @d f: number = 1; @d m(@d a: number): void {{}} @d static s: number = 2; }}\n"),
        legacy(),
    )
    .unwrap();
    assert!(javascript.contains("var __bluetsDecorate"), "{javascript}");
    assert!(javascript.contains("var __bluetsParam"), "{javascript}");
    assert!(!javascript.contains("var __bluetsMetadata"), "{javascript}");
    assert!(javascript.contains("let C = class C"), "{javascript}");
    // Instance members first, then static ones, then the class.
    let f = javascript.find("C.prototype, \"f\"").unwrap();
    let m = javascript.find("C.prototype, \"m\"").unwrap();
    let s = javascript.find("C, \"s\"").unwrap();
    let class = javascript.find("C = __bluetsDecorate([d], C)").unwrap();
    assert!(f < m && m < s && s < class, "{javascript}");
    assert!(javascript.contains("__bluetsParam(0, d)"), "{javascript}");
}

#[test]
fn only_a_class_with_class_decorators_is_rewritten_into_a_let() {
    let javascript = build(
        &format!("{DECL}class C {{ @d m(): void {{}} }}\n"),
        legacy(),
    )
    .unwrap();
    assert!(
        javascript.starts_with("declare") || javascript.contains("class C {"),
        "{javascript}"
    );
    assert!(!javascript.contains("let C = class"), "{javascript}");
    assert!(
        javascript.contains("__bluetsDecorate([d], C.prototype, \"m\", null);"),
        "{javascript}"
    );
}

#[test]
fn metadata_is_emitted_only_with_the_option_and_serializes_types() {
    let source = format!(
        "{DECL}class Dep {{}}\ninterface Shape {{ x: number }}\nclass C {{ @d a: number[] = []; @d b: Shape = {{ x: 1 }}; @d c: string | number = 1; @d m(x: Dep): Dep {{ return x; }} }}\n"
    );
    assert!(!build(&source, legacy())
        .unwrap()
        .contains("__bluetsMetadata"));
    let javascript = build(&source, metadata()).unwrap();
    assert!(
        javascript.contains("__bluetsMetadata(\"design:type\", Array)"),
        "{javascript}"
    );
    assert!(
        javascript.contains("__bluetsMetadata(\"design:type\", Object)"),
        "{javascript}"
    );
    assert!(
        javascript.contains("__bluetsMetadata(\"design:paramtypes\", [Dep])"),
        "{javascript}"
    );
    assert!(
        javascript.contains("__bluetsMetadata(\"design:returntype\", Dep)"),
        "{javascript}"
    );
}

#[test]
fn metadata_for_a_type_it_cannot_resolve_is_refused_not_guessed() {
    let messages = build(
        &format!("{DECL}class C {{ @d f: Unknown = null as never; }}\n"),
        metadata(),
    );
    assert!(messages.is_err());
    let messages = build(
        &format!("{DECL}class C {{ @d m(): void {{}} @d n() {{ return 1; }} }}\n"),
        metadata(),
    );
    assert!(messages.is_err());
}

#[test]
fn every_source_line_keeps_its_line() {
    let source = format!(
        "{DECL}@d\nclass C {{\n  @d\n  m(\n    @d a: number,\n    b: string\n  ): void {{}}\n}}\nexport const after = 1;\n"
    );
    let javascript = build(&source, legacy()).unwrap();
    assert_eq!(
        javascript.matches('\n').count(),
        source.matches('\n').count(),
        "{javascript}"
    );
    assert_eq!(
        javascript.lines().last().unwrap(),
        "export const after = 1;"
    );
}

#[test]
fn export_forms_and_commonjs_and_both_targets_work() {
    for target in [EcmaTarget::Es2022, EcmaTarget::Es2020] {
        let javascript = build(
            &format!("{DECL}@d export class C {{ static s: number = 1; }}\n"),
            CompilerOptions { target, ..legacy() },
        )
        .unwrap();
        assert!(javascript.contains("export { C };"), "{javascript}");
    }
    let commonjs = build(
        &format!("{DECL}@d export class C {{}}\n"),
        CompilerOptions {
            module_kind: ModuleKind::CommonJs,
            ..legacy()
        },
    )
    .unwrap();
    assert!(commonjs.contains("exports.C = C;"), "{commonjs}");
}

#[test]
fn parameter_decorators_need_the_legacy_option() {
    let source = format!("{DECL}class C {{ m(@d a: number): void {{}} }}\n");
    let messages = build(&source, CompilerOptions::default())
        .unwrap_err()
        .join("\n");
    assert!(messages.contains("experimentalDecorators"), "{messages}");
    assert!(build(&source, legacy()).is_ok());
}

#[test]
fn legacy_mode_does_not_use_the_standard_helpers() {
    let javascript = build(
        &format!("{DECL}@d class C {{ @d m(): void {{}} }}\n"),
        legacy(),
    )
    .unwrap();
    assert!(!javascript.contains("__bluetsEsDecorate"), "{javascript}");
    assert!(!javascript.contains("Symbol.metadata"), "{javascript}");
}

#[test]
fn the_options_are_in_the_fingerprint_and_the_manifest_and_validated() {
    let root = std::env::temp_dir().join(format!("bluetsc-legacy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("main.ts"),
        format!("{DECL}class C {{ @d m(@d a: number): void {{}} }}\n"),
    )
    .unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&root)
            .args(args)
            .output()
            .unwrap()
    };
    let out = root.join("out");
    let built = run(&[
        "build",
        "main.ts",
        "--experimental-decorators",
        "--emit-decorator-metadata",
        "--out-dir",
        out.to_str().unwrap(),
    ]);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("bluetsc.manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["experimentalDecorators"], true);
    assert_eq!(manifest["emitDecoratorMetadata"], true);
    assert_eq!(
        manifest["legacyDecoratorHelperVersion"],
        "bluets-legacy-decorator-helper-v1"
    );
    let alone = run(&["check", "main.ts", "--emit-decorator-metadata"]);
    assert!(!alone.status.success());
    assert!(String::from_utf8_lossy(&alone.stderr).contains("requires --experimental-decorators"));
    std::fs::write(
        root.join("bluetsc.json"),
        r#"{"entries":["main.ts"],"emitDecoratorMetadata":true}"#,
    )
    .unwrap();
    let config = run(&["check", "--config", "bluetsc.json"]);
    assert!(String::from_utf8_lossy(&config.stderr).contains("requires experimentalDecorators"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn legacy_checking_resolves_the_decorator_call() {
    for (source, expected) in [
        ("declare function two(a: any, b: any): void;\n@two class C {}\n", "must accept 1 argument"),
        ("declare function one(a: any): void;\nclass C { @one f: number = 1; }\n", "must accept 2 argument"),
        ("declare function bad(a: any, b: string): number;\nclass C { @bad f: number = 1; }\n", "cannot return"),
        ("declare function p(a: any, b: any, c: number): void;\nclass C { m(@p a: number): void {} }\n", ""),
    ] {
        let result = build(source, legacy());
        if expected.is_empty() {
            assert!(result.is_ok(), "{source}: {result:?}");
        } else {
            let messages = result.unwrap_err().join("\n");
            assert!(messages.contains(expected), "{source}: {messages}");
        }
    }
}

#[test]
fn metadata_serializes_every_type_kind_like_typescript() {
    let source = format!(
        "{DECL}class Dep {{}}\ninterface Shape {{ x: number }}\ntype Alias = string;\ntype Chain = Alias;\nenum Num {{ A, B }}\nenum Str {{ A = \"a\" }}\nclass M {{\n\
         @d a!: 1;\n@d b!: \"s\";\n@d c!: true;\n@d d: void;\n@d e!: null;\n@d f: undefined;\n@d g: [number, string] = [1, \"\"];\n@d h: () => void = () => {{}};\n\
         @d i: Shape = {{ x: 1 }};\n@d j: {{ a: number }} = {{ a: 1 }};\n@d k: Chain = \"\";\n@d l: Num = Num.A;\n@d m: Str = Str.A;\n@d n: number | string = 1;\n@d o: string | null = null;\n@d p: Dep | undefined;\n@d q: Promise<number> = Promise.resolve(1);\n@d s: any = 1;\n@d t: unknown;\n@d u: number | 1 = 1;\n}}\n"
    );
    let javascript = build(&source, metadata()).unwrap();
    for (member, expected) in [
        ("\"a\"", "Number"),
        ("\"b\"", "String"),
        ("\"c\"", "Boolean"),
        ("\"d\"", "void 0"),
        ("\"e\"", "void 0"),
        ("\"f\"", "void 0"),
        ("\"g\"", "Array"),
        ("\"h\"", "Function"),
        ("\"i\"", "Object"),
        ("\"j\"", "Object"),
        ("\"k\"", "String"),
        ("\"l\"", "Number"),
        ("\"m\"", "String"),
        ("\"n\"", "Object"),
        ("\"o\"", "String"),
        ("\"p\"", "Dep"),
        ("\"q\"", "Promise"),
        ("\"s\"", "Object"),
        ("\"t\"", "Object"),
        ("\"u\"", "Number"),
    ] {
        let marker = format!("], M.prototype, {member}, void 0)");
        let at = javascript
            .find(&marker)
            .unwrap_or_else(|| panic!("{member}: {javascript}"));
        let statement = &javascript[javascript[..at].rfind("__bluetsDecorate").unwrap()..at];
        assert!(
            statement.contains(&format!("__bluetsMetadata(\"design:type\", {expected})")),
            "{member} -> {expected}: {statement}"
        );
    }
}

#[test]
fn accessor_pairs_describe_the_pair_and_need_an_annotation() {
    let javascript = build(
        &format!("{DECL}class A {{ @d get g(): number {{ return 1; }} set g(v: number) {{}} @d static set s(v: string) {{}} }}\n"),
        metadata(),
    )
    .unwrap();
    assert!(
        javascript.contains("__bluetsMetadata(\"design:paramtypes\", [Number])"),
        "{javascript}"
    );
    assert!(javascript.contains("A, \"s\", null"), "{javascript}");
    let refused = build(
        &format!("{DECL}class A {{ @d get g() {{ return 1; }} }}\n"),
        metadata(),
    );
    assert!(refused.is_err());
}

#[test]
fn decorated_namespace_exports_receive_the_replacement_class() {
    let javascript = build(
        &format!("{DECL}namespace N {{ @d export class C {{}} }}\n"),
        legacy(),
    )
    .unwrap();
    let decorated = javascript.find("C = __bluetsDecorate").unwrap();
    let exported = javascript.find("N.C = C;").unwrap();
    assert!(decorated < exported, "{javascript}");
    assert!(!javascript.contains("@d"), "{javascript}");
}

#[test]
fn unlowered_legacy_forms_are_refused_with_a_reason() {
    for (source, expected) in [
        (
            format!("{DECL}class A {{ @d accessor a: number = 1; }}\n"),
            "auto-accessors",
        ),
        (
            format!("{DECL}class A {{ @d #p: number = 1; }}\n"),
            "private names",
        ),
        (
            format!(
                "{DECL}class A {{ @d get g(): number {{ return 1; }} @d set g(v: number) {{}} }}\n"
            ),
            "getter and the setter",
        ),
        (
            "declare function mk(n: number): (v: any) => void;\n@mk(// comment\n1) class A {}\n"
                .to_string(),
            "line comment",
        ),
    ] {
        let messages = build(&source, legacy());
        let joined = messages.unwrap_err().join("\n");
        assert!(joined.contains(expected), "{source}: {joined}");
    }
}

#[test]
fn legacy_decorators_on_static_and_constructor_forms() {
    let javascript = build(
        &format!("{DECL}class B0 {{}}\n@d class A extends B0 {{ constructor(@d private x: number) {{ super(); }} @d static m(@d a: number): void {{}} @d static f: number = 1; }}\n"),
        legacy(),
    )
    .unwrap();
    assert!(
        javascript.contains("A = __bluetsDecorate([d, __bluetsParam(0, d)], A);"),
        "{javascript}"
    );
    assert!(javascript.contains("A, \"m\", null"), "{javascript}");
    assert!(javascript.contains("A, \"f\", void 0"), "{javascript}");
}
