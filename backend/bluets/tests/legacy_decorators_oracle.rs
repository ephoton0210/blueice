// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Legacy (`experimentalDecorators`) decorators and decorator metadata against pinned TypeScript (J.5.4): each program is built
//! by `bluetsc` and by `tsc` for ES2022 and ES2020 and both outputs must print the same
//! thing under Node. A program logs every decorator application with its context,
//! the order of initializers and `addInitializer` callbacks, and what replaced
//! values do, so the evaluation order, the application order, the context
//! objects, `Symbol.metadata` and the field and accessor semantics are pinned
//! rather than the text. The offline tests pin refusals and the emitted shape.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Shared helpers of every program: `rec` records an event and `shape` describes
/// the arguments a decorator received.
const PRELUDE: &str = "const events: string[] = [];\nconst rec = (text: string): void => { events.push(text); };\nconst show = (): void => { console.log(events.join(\"\\n\")); };\nconst shape = (args: any[]): string => args.map((a: any, i: number) => i === 0 ? (typeof a === \"function\" ? \"class\" : typeof a === \"object\" ? \"proto\" : typeof a) : typeof a === \"object\" && a !== null && \"value\" in a || typeof a === \"object\" && a !== null && \"get\" in a ? \"descriptor\" : String(a)).join(\",\");\nconst tag = (name: string): any => (...args: any[]): void => { rec(name + \"(\" + shape(args) + \")\"); };\n";

/// A `Reflect.metadata` that records, so the metadata calls are visible.
const REFLECT: &str = "const reflectAny: any = Reflect;\nreflectAny.metadata = (key: string, value: any): any => (target: any, member: any): void => { rec(\"meta \" + key + \"=\" + (Array.isArray(value) ? \"[\" + value.map((v: any) => v === undefined ? \"undefined\" : v.name).join(\",\") + \"]\" : value === undefined ? \"undefined\" : value.name) + \" on \" + (typeof member === \"undefined\" ? \"class\" : String(member))); };\n";

const ORDER: &str = r#"
class Dep {}
@tag("C1") @tag("C2")
class C {
  @tag("sf") static sf: number = 1;
  @tag("f") f: string = "x";
  @tag("m") m(@tag("p0") a: number, @tag("p1") b: Dep): void {}
  @tag("sm") static sm(@tag("sp") a: number): number { return 1; }
  @tag("g") get g(): number { return 1; }
  set g(v: number) {}
  other: number = 2;
  constructor(@tag("cp0") private x: number, @tag("cp1") y: string) {}
}
class OnlyMembers { @tag("om") m(): void {} @tag("of") f: number = 1; }
rec("done " + new C(1, "y").f + " " + new OnlyMembers().f);
show();
"#;

const REPLACE: &str = r#"
class Other { v: number = 42; }
const swap = (): any => (target: any): any => Other;
const wrap = (): any => (target: any, key: string, descriptor: any): any => ({ value: function (): number { return 99; }, writable: true, configurable: true });
const change = (): any => (target: any, key: string): void => { Object.defineProperty(target, key, { value: "defined", writable: true, configurable: true }); };
@swap()
class Orig { @wrap() m(): number { return 1; } @change() f: string = "init"; static kind: string = "orig"; }
rec("replaced " + Reflect.get(Orig, "kind") + " " + Reflect.get(new Orig(), "v"));
class Keep { @wrap() m(): number { return 1; } }
rec("wrapped " + new Keep().m());
class Fld { @change() f: string = "init"; }
rec("field " + new Fld().f);
show();
"#;

const EXTENDS: &str = r#"
class Base { @tag("base-m") m(): void {} }
@tag("derived") class Derived extends Base { @tag("derived-m") m(): void {} constructor() { super(); } }
rec("derived " + String(new Derived() instanceof Base));
show();
"#;

const EXPORTS: &str = "const dec = (...args: any[]): void => { console.log(\"decorated\", typeof args[0], String(args[1])); };\nexport const marker = 1;\n@dec export class Exported { @dec m(): void {} }\nconsole.log(typeof Exported, Exported.name, marker);\n";

const METADATA: &str = r#"
class Dep {}
interface Shape { x: number }
type Alias = string;
enum Num { A, B }
enum Str { A = "a" }
const mark = (): any => (...args: any[]): void => {};
@mark()
class Meta {
  @mark() f1: number = 1;
  @mark() f2: string = "";
  @mark() f3: boolean = true;
  @mark() f4: number[] = [];
  @mark() f5: Shape = { x: 1 };
  @mark() f6: Alias = "";
  @mark() f7: Num = Num.A;
  @mark() f8: Str = Str.A;
  @mark() f9: () => void = () => {};
  @mark() f10: string | number = 1;
  @mark() f11: any = 1;
  @mark() f12: boolean | undefined;
  @mark() f13: [number, string] = [1, ""];
  @mark() f20: boolean | undefined;
  @mark() f14!: "lit";
  @mark() f15: Dep | null = null;
  @mark() f17: unknown;
  @mark() f19: Dep = new Dep();
  @mark() m(a: number, b: Dep, c: string[]): Dep { return new Dep(); }
  @mark() v(): void {}
  @mark() get g(): string { return ""; }
  constructor(a: number, b: Dep) {}
}
show();
"#;

fn files(list: Vec<(&'static str, &'static str)>) -> &'static [(&'static str, &'static str)] {
    Box::leak(list.into_boxed_slice())
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn programs() -> Vec<Program> {
    let mut list = Vec::new();
    for (name, body, metadata) in [
        ("order-and-forms", ORDER, false),
        ("order-and-forms-with-metadata", ORDER, true),
        ("replacement-and-descriptors", REPLACE, false),
        ("extends", EXTENDS, false),
        ("metadata-types", METADATA, true),
    ] {
        for (module, target) in [
            ("es2022", "es2022"),
            ("commonjs", "es2022"),
            ("es2022", "es2020"),
        ] {
            list.push(Program {
                name: Box::leak(format!("{name}-{module}-{target}").into_boxed_str()),
                module,
                target,
                metadata,
                files: files(vec![(
                    "main.ts",
                    leak(format!(
                        "export {{}};\n{PRELUDE}{}{body}",
                        if metadata { REFLECT } else { "" }
                    )),
                )]),
            });
        }
    }
    for (module, target) in [
        ("es2022", "es2022"),
        ("commonjs", "es2022"),
        ("es2022", "es2020"),
    ] {
        list.push(Program {
            name: Box::leak(format!("exports-{module}-{target}").into_boxed_str()),
            module,
            target,
            metadata: false,
            files: files(vec![("main.ts", EXPORTS)]),
        });
    }
    list
}

struct Program {
    name: &'static str,
    module: &'static str,
    target: &'static str,
    metadata: bool,
    files: &'static [(&'static str, &'static str)],
}

fn run_program(program: &Program) -> (String, String, String) {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = env::temp_dir().join(format!(
        "bluets-legacy-{}-{}",
        std::process::id(),
        program.name
    ));
    let _ = fs::remove_dir_all(&root);
    let input = root.join("src");
    let blue = root.join("blue");
    let reference = root.join("tsc");
    for directory in [&input, &reference] {
        fs::create_dir_all(directory).unwrap();
    }
    for (file, text) in program.files {
        fs::write(input.join(file), text).unwrap();
    }
    let commonjs = program.module == "commonjs";
    let mut command = Command::new(env!("CARGO_BIN_EXE_bluetsc"));
    command
        .current_dir(&input)
        .args([
            "build",
            "main.ts",
            "--experimental-decorators",
            "--target",
            program.target,
            "--module",
            if commonjs { "commonjs" } else { "esnext" },
            "--out-dir",
        ])
        .arg(&blue);
    if program.metadata {
        command.arg("--emit-decorator-metadata");
    }
    let built = command.output().unwrap();
    assert!(
        built.status.success(),
        "{}: bluetsc failed: {}",
        program.name,
        String::from_utf8_lossy(&built.stderr)
    );
    for directory in [&blue, &reference] {
        fs::write(
            directory.join("package.json"),
            if commonjs {
                r#"{"type":"commonjs"}"#
            } else {
                r#"{"type":"module"}"#
            },
        )
        .unwrap();
    }
    let mut command = Command::new(tsc);
    command
        .current_dir(&input)
        .args([
            "--target",
            &program.target.to_uppercase(),
            "--pretty",
            "false",
            "--skipLibCheck",
            "--experimentalDecorators",
            "--module",
            program.module,
            "--outDir",
        ])
        .arg(&reference)
        .arg("main.ts");
    if program.metadata {
        command.arg("--emitDecoratorMetadata");
    }
    let _ = command.output().unwrap();
    assert!(
        reference.join("main.js").exists(),
        "{}: tsc emitted nothing",
        program.name
    );
    let blue_run = Command::new(&node)
        .env("FORCE_COLOR", "0")
        .arg(blue.join("main.js"))
        .output()
        .unwrap();
    let tsc_run = Command::new(&node)
        .env("FORCE_COLOR", "0")
        .arg(reference.join("main.js"))
        .output()
        .unwrap();
    let emitted = fs::read_to_string(blue.join("main.js")).unwrap();
    let _ = fs::remove_dir_all(&root);
    assert!(
        blue_run.status.success(),
        "{}: BlueTSC output failed: {}\n{emitted}",
        program.name,
        String::from_utf8_lossy(&blue_run.stderr),
    );
    assert!(
        tsc_run.status.success(),
        "{}: tsc output failed: {}",
        program.name,
        String::from_utf8_lossy(&tsc_run.stderr)
    );
    (
        String::from_utf8_lossy(&blue_run.stdout).into_owned(),
        String::from_utf8_lossy(&tsc_run.stdout).into_owned(),
        emitted,
    )
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_legacy_decorators_print_what_typescript_prints() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    let only = env::var("BLUETS_DECORATOR_PROGRAM").ok();
    for program in programs() {
        if only
            .as_deref()
            .is_some_and(|name| !program.name.contains(name))
        {
            continue;
        }
        let (blue, reference, emitted) = run_program(&program);
        assert!(
            !reference.trim().is_empty(),
            "{}: the program printed nothing",
            program.name
        );
        assert_eq!(
            blue, reference,
            "{}: BlueTSC and TypeScript print different output\n{emitted}",
            program.name
        );
    }
}
