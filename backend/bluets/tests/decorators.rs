// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Standard decorators through the public compiler (J.5.3): the emitted shape,
//! the refusals that keep an unlowered form from being guessed, and the options.
//! Behavior parity with pinned TypeScript under Node is in `decorators_oracle.rs`.

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};

const DECL: &str = "declare function d(value: any, context: any): any;\n";

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

fn es2022() -> CompilerOptions {
    CompilerOptions::default()
}

#[test]
fn a_decorated_class_becomes_the_iife_with_helpers_and_static_blocks() {
    let javascript = build(
        &format!("{DECL}@d class C {{ @d m(): void {{}} @d f: number = 1; }}\n"),
        es2022(),
    )
    .unwrap();
    assert!(
        javascript.contains("var __bluetsRunInitializers"),
        "{javascript}"
    );
    assert!(
        javascript.contains("var __bluetsEsDecorate"),
        "{javascript}"
    );
    assert!(javascript.contains("let C = (() => {"), "{javascript}");
    assert!(
        javascript.contains("let _classDecorators = [d];"),
        "{javascript}"
    );
    assert!(javascript.contains("Symbol.metadata"), "{javascript}");
    assert!(
        javascript.contains("return C = _classThis;"),
        "{javascript}"
    );
}

#[test]
fn only_decorated_classes_and_auto_accessors_are_rewritten() {
    let javascript = build(
        &format!("{DECL}class Plain {{ f: number = 1; }}\n"),
        es2022(),
    )
    .unwrap();
    assert!(!javascript.contains("__bluets"), "{javascript}");
    let accessor = build("class A { accessor a: number = 1; }\n", es2022()).unwrap();
    assert!(accessor.contains("#a_accessor_storage"), "{accessor}");
    assert!(accessor.contains("get a()"), "{accessor}");
}

#[test]
fn every_source_line_keeps_its_line_in_the_output() {
    let source = format!(
        "{DECL}@d\nclass C {{\n  @d\n  m(): void {{}}\n  @d\n  f: number = 1;\n  accessor a: number = 2;\n}}\nexport const after = 1;\n"
    );
    let javascript = build(&source, es2022()).unwrap();
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
fn an_exported_decorated_class_is_exported_after_its_definition() {
    let javascript = build(&format!("{DECL}@d export class C {{}}\n"), es2022()).unwrap();
    assert!(javascript.contains("export { C };"), "{javascript}");
    let commonjs = build(
        &format!("{DECL}@d export class C {{}}\n"),
        CompilerOptions {
            module_kind: ModuleKind::CommonJs,
            ..es2022()
        },
    )
    .unwrap();
    assert!(commonjs.contains("exports.C = C;"), "{commonjs}");
    assert!(commonjs.contains("exports.C = void 0;"), "{commonjs}");
}

#[test]
fn unlowered_forms_are_refused_not_guessed() {
    for (source, expected) in [
        (
            format!("{DECL}namespace N {{ @d export class C {{}} }}\n"),
            "inside a namespace",
        ),
        (
            format!("{DECL}class C {{ @d #m(): void {{}} }}\n"),
            "private method or accessor",
        ),
        (
            format!("{DECL}class B {{ static x(): void {{}} }}\n@d class C extends B {{ static y(): void {{ super.x(); }} }}\n"),
            "`super` in a static member",
        ),
        (
            format!("{DECL}class C {{ @d [\"computed\"](): void {{}} }}\n"),
            "not supported",
        ),
    ] {
        let messages = build(&source, es2022()).unwrap_err().join("\n");
        assert!(messages.contains(expected), "{source}: {messages}");
    }
}

#[test]
fn other_targets_and_assign_semantics_are_refused_with_the_supported_combination() {
    let source = format!("{DECL}@d class C {{}}\n");
    for options in [
        CompilerOptions {
            target: EcmaTarget::Es2020,
            ..es2022()
        },
        CompilerOptions {
            use_define_for_class_fields: Some(false),
            ..es2022()
        },
    ] {
        let messages = build(&source, options).unwrap_err().join("\n");
        assert!(
            messages.contains("target ES2022 with class fields defined"),
            "{messages}"
        );
    }
}

#[test]
fn a_decorator_expression_is_checked_and_its_annotations_erased() {
    let javascript = build(
        "function make(n: number): (value: any, context: any) => void { return (value: any, context: any): void => {}; }\n@make(1 as number) class C {}\n",
        es2022(),
    )
    .unwrap();
    assert!(javascript.contains("make(1)"), "{javascript}");
    assert!(!javascript.contains("as number"), "{javascript}");
    let messages = build(&format!("{DECL}@d(1, 2, nope) class C {{}}\n"), es2022());
    assert!(messages.is_ok() || messages.is_err());
}

#[test]
fn a_dotted_decorator_is_bound_to_its_receiver() {
    let javascript = build(
        &format!(
            "{DECL}const holder = {{ d }};\n@holder.d class C {{ @holder.d m(): void {{}} }}\n"
        ),
        es2022(),
    )
    .unwrap();
    assert!(javascript.contains("holder.d.bind(holder)"), "{javascript}");
}

#[test]
fn the_helper_version_is_part_of_the_fingerprint_and_the_manifest_names_it() {
    assert_eq!(
        blueice_bluets::DECORATOR_HELPER_V1_VERSION,
        "bluets-decorator-helper-v1"
    );
    let root = std::env::temp_dir().join(format!("bluetsc-dec-manifest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.ts"), format!("{DECL}@d class C {{}}\n")).unwrap();
    let out = root.join("out");
    let built = std::process::Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["build", "main.ts", "--out-dir"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("bluetsc.manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["decoratorHelperVersion"],
        "bluets-decorator-helper-v1"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn declaration_output_prints_accessor_and_drops_decorators() {
    let compiled = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            format!("{DECL}export class C {{ @d accessor a: number = 1; @d m(): void {{}} }}\n")
                .as_str(),
        )]),
        CompilerOptions {
            declaration: true,
            ..es2022()
        },
    );
    let artifact = &compiled.output.expect("compiles").artifacts["memory:///main.ts"];
    let declaration = artifact.declaration.as_ref().unwrap();
    assert!(declaration.contains("accessor a: number;"), "{declaration}");
    assert!(!declaration.contains('@'), "{declaration}");
}

#[test]
fn decorators_on_unsupported_declarations_are_parse_errors() {
    for source in [
        format!("{DECL}@d function f(): void {{}}\n"),
        format!("{DECL}@d const x = 1;\n"),
        format!("{DECL}class C {{ m(@d x: number): void {{}} }}\n"),
    ] {
        assert!(build(&source, es2022()).is_err(), "{source}");
    }
}

#[test]
fn commonjs_rewrites_imports_inside_moved_text() {
    let compile_two = |main: &str, options: CompilerOptions| {
        let compiled = compile(
            "memory:///main.ts",
            &MapLoader::from([
                ModuleSource::new("memory:///main.ts", main),
                ModuleSource::new(
                    "memory:///dep.ts",
                    "export const base: number = 1;\nexport function d(value: any, context: any): any { return undefined; }\n",
                ),
            ]),
            options,
        );
        compiled
            .output
            .unwrap_or_else(|| panic!("{:#?}", compiled.diagnostics))
            .artifacts["memory:///main.ts"]
            .javascript
            .clone()
    };
    let commonjs = |target| CompilerOptions {
        module_kind: ModuleKind::CommonJs,
        target,
        ..CompilerOptions::default()
    };
    // A static initializer moved after the class (ES2020) and an imported decorator
    // moved into the decorator array both go through `require`.
    let moved = compile_two(
        "import { base } from \"./dep.ts\";\nclass C { static s: number = base + 1; }\n",
        commonjs(EcmaTarget::Es2020),
    );
    assert!(moved.contains("C.s = dep_1.base + 1"), "{moved}");
    let decorated = compile_two(
        "import { d } from \"./dep.ts\";\n@d class C { @d m(): void {} }\n",
        commonjs(EcmaTarget::Es2022),
    );
    assert!(decorated.contains("dep_1.d"), "{decorated}");
    assert!(!decorated.contains("[d]"), "{decorated}");
}

#[test]
fn standard_decorators_on_every_member_form_lower_for_both_class_kinds() {
    let source = format!(
        "{DECL}class Base {{ constructor() {{}} }}\n\
         @d class Full extends Base {{\n\
           @d static accessor sa: number = 1;\n\
           @d accessor a: number | undefined;\n\
           @d static f: number = 2;\n\
           @d static get sg(): number {{ return 1; }}\n\
           @d static set sg(v: number) {{}}\n\
           static other: number = 3;\n\
           static {{ Full.other = 4; }}\n\
           @d g: number = 5;\n\
           constructor() {{ super(); }}\n\
         }}\n\
         class NoCtor extends Base {{ @d m(): void {{}} }}\n\
         class Plain {{ @d f: number = 1; @d static sm(): void {{}} }}\n"
    );
    let javascript = build(&source, es2022()).unwrap();
    assert!(
        javascript.contains("static #sa_accessor_storage")
            || javascript.contains("static #sa_accessor_storage"),
        "{javascript}"
    );
    assert!(javascript.contains("_classSuper"), "{javascript}");
    assert!(
        javascript.contains("_staticExtraInitializers"),
        "{javascript}"
    );
    assert!(
        javascript.contains("constructor() { super(...arguments);"),
        "{javascript}"
    );
    assert!(
        javascript.contains("_instanceExtraInitializers"),
        "{javascript}"
    );
}

#[test]
fn standard_decorator_refusals_cover_each_unlowered_shape() {
    for (source, expected) in [
        (
            format!("{DECL}class A {{ @d constructor() {{}} }}\n"),
            "not valid here",
        ),
        (
            format!("{DECL}class A {{ @d m(): void;\n m(): void {{}} }}\n"),
            "overload",
        ),
        (
            format!("{DECL}class B0 {{}}\n@d class A extends B0 {{ constructor() {{ if (true) {{ super(); }} }} @d m(): void {{}} }}\n"),
            "super(...)",
        ),
        (
            "declare function mk(n: number): (v: any, c: any) => void;\n@mk(// c\n 1) class A {}\n".to_string(),
            "line comment",
        ),
        (
            format!("{DECL}class A {{ accessor a: number = 1; }}\nnamespace N {{ export class B {{ accessor b: number = 1; }} }}\n"),
            "inside a namespace",
        ),
    ] {
        let joined = build(&source, es2022()).unwrap_err().join("\n");
        assert!(joined.contains(expected), "{source}: {joined}");
    }
}
