// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Standard decorators against pinned TypeScript (J.5.3): each program is built
//! by `bluetsc` and by `tsc` for ES2022 and both outputs must print the same
//! thing under Node. A program logs every decorator application with its context,
//! the order of initializers and `addInitializer` callbacks, and what replaced
//! values do, so the evaluation order, the application order, the context
//! objects, `Symbol.metadata` and the field and accessor semantics are pinned
//! rather than the text. The offline tests pin refusals and the emitted shape.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct Program {
    name: &'static str,
    module: &'static str,
    files: &'static [(&'static str, &'static str)],
    /// The program ends by throwing; the error line is what is compared.
    fails: bool,
}

/// Shared helpers of every program: `Symbol.metadata` exists (Node has none), and
/// `rec` records an event.
const PRELUDE: &str = "const SymbolAny: any = Symbol;\nSymbolAny.metadata ??= Symbol.for(\"Symbol.metadata\");\nconst anyOf = (value: any): any => value;\nlet events: string[] = [];\nconst rec = (text: string): void => { events.push(text); };\nconst show = (): void => { console.log(events.join(\"\\n\")); events = []; };\nconst tag = (name: string): any => (value: any, ctx: any): any => { rec(\"apply \" + name + \" \" + ctx.kind + \" \" + String(ctx.name) + \" static=\" + ctx.static + \" private=\" + ctx.private); };\n";

const ALL_KINDS: &str = r#"
const addInit = (name: string): any => (value: any, ctx: any): any => {
  rec("apply " + name + " " + ctx.kind + " " + String(ctx.name));
  ctx.addInitializer(function () { rec("init " + name + " this=" + (typeof this === "function" ? "class" : typeof this)); });
};
const wrapMethod = (name: string): any => (fn: any, ctx: any): any => {
  rec("apply " + name + " " + ctx.kind + " " + String(ctx.name));
  return function (...args: any[]): any { rec("call " + name); return fn.apply(this, args); };
};
const fieldInit = (name: string, add: number): any => (value: any, ctx: any): any => {
  rec("apply " + name + " " + ctx.kind + " " + String(ctx.name));
  return (initial: any): any => { rec("field " + name + " initial=" + initial); return initial + add; };
};
const accessorWrap = (name: string): any => (value: any, ctx: any): any => {
  rec("apply " + name + " " + ctx.kind + " " + String(ctx.name) + " " + typeof value.get + typeof value.set);
  return {
    get(): any { rec("get " + name); return value.get.call(this); },
    set(v: any): void { rec("set " + name); value.set.call(this, v); },
    init(v: any): any { rec("init-accessor " + name + " " + v); return v * 2; },
  };
};
class Plain {
  @addInit("sm") static sm(): number { return 1; }
  @wrapMethod("m") m(): number { return 2; }
  @addInit("m2") @wrapMethod("m2") m2(): number { return 3; }
  @wrapMethod("g") get g(): number { return 4; }
  @fieldInit("f", 10) f: number = 5;
  @fieldInit("sf", 100) static sf: number = 6;
  @fieldInit("noinit", 1) noinit: any;
  @accessorWrap("acc") accessor acc: number = 7;
  @fieldInit("p", 1000) #p: number = 8;
  other: number = 9;
  readP(): number { return this.#p; }
}
const p = new Plain();
rec("results " + Plain.sm() + " " + p.m() + " " + p.m2() + " " + p.g + " " + p.f + " " + Plain.sf + " " + p.noinit + " " + p.acc + " " + p.other + " " + p.readP());
p.acc = 1;
rec("acc " + p.acc);
show();
"#;

const CLASS_DECORATORS: &str = r#"
class Replacement { extra: number = 1; static kind: string = "replacement"; }
const replace = (label: string): any => (value: any, ctx: any): any => {
  rec("class " + label + " name=" + ctx.name + " kind=" + ctx.kind + " value=" + value.name);
  ctx.addInitializer(function () { rec("class-init " + label + " this=" + this.name); });
  return Replacement;
};
const addStatic = (): any => (value: any, ctx: any): any => { Object.assign(value, { added: "yes" }); };
@replace("r") @addStatic()
class C2 {
  static s: number = 1;
  static self: any = this;
  static { rec("static block this===C2: " + (this === C2)); }
  v: number = 2;
}
rec("C2 " + C2.name + " " + anyOf(C2).added + " " + anyOf(C2).kind + " " + new C2().extra);
class Keep { static s: number = 1; static self: any = this; static { rec("keep static block " + this.name); } v: number = 2; }
@tag("keep") class Kept { static s: number = 1; static self: any = this; static { rec("kept static block this===Kept " + (this === Kept)); } v: number = 3; }
rec("Kept " + Kept.name + " " + (Kept.self === Kept) + " " + new Kept().v);
class Base { constructor() { rec("Base ctor"); } }
@tag("D") class Derived extends Base {
  @tag("m") m(): void {}
  constructor() { super(); rec("Derived ctor"); }
}
new Derived();
show();
"#;

const ORDER_AND_FORMS: &str = r#"
const ns = { dec: function (value: any, ctx: any): void { rec("ns.dec this=" + (this === ns) + " " + ctx.kind); } };
const factory = (n: string): any => { rec("eval " + n); return (value: any, ctx: any): void => { rec("apply " + n + " " + ctx.kind + " " + String(ctx.name)); }; };
function make(): any { return factory("paren"); }
rec("before class");
@factory("c1") @factory("c2")
class Order {
  @factory("sf") static sf: any = rec("sf init");
  @factory("f1") @factory("f2") f: any = rec("f init");
  @factory("m") m(): void {}
  @ns.dec static sm(): void {}
  @(make()) pm(): void {}
  static { rec("static block"); }
  @factory("last") last: any = rec("last init");
}
rec("after class");
new Order();
show();
"#;

const METADATA: &str = r#"
const meta = (key: string): any => (value: any, ctx: any): void => { Reflect.set(ctx.metadata, key, (Reflect.get(ctx.metadata, key) ?? 0) + 1); rec("meta " + key + " " + (ctx.metadata === undefined)); };
@meta("a") class M1 { @meta("b") x: number = 1; }
@meta("c") class M2 extends M1 { @meta("d") y: number = 2; }
const m1 = anyOf(M1)[SymbolAny.metadata];
const m2 = anyOf(M2)[SymbolAny.metadata];
rec(JSON.stringify(m1) + " " + JSON.stringify(m2) + " " + (Object.getPrototypeOf(m2) === m1) + " " + Object.keys(m2).join(","));
show();
"#;

const ACCESS: &str = r#"
const grab = (name: string): any => (value: any, ctx: any): any => {
  rec("grab " + name + " has=" + typeof ctx.access.has + " get=" + typeof ctx.access.get + " set=" + typeof ctx.access.set);
  ctx.addInitializer(function () {
    const obj = this;
    rec("access " + name + " has=" + ctx.access.has(obj) + (ctx.access.get ? " get=" + (typeof ctx.access.get(obj) === "function" ? "fn" : ctx.access.get(obj)) : ""));
    if (ctx.access.set) { ctx.access.set(obj, 99); rec("after set " + name + " " + (ctx.access.get ? ctx.access.get(obj) : "?")); }
  });
};
class Acc {
  @grab("field") fieldValue: number = 1;
  @grab("method") method(): number { return 2; }
  @grab("getter") get g(): number { return 3; }
  @grab("setter") set s(v: number) { rec("setter " + v); }
  @grab("accessor") accessor a: number = 4;
  @grab("private") #hidden: number = 5;
}
new Acc();
show();
"#;

const STATIC_INJECTION: &str = r#"
const si = (name: string): any => (value: any, ctx: any): void => { ctx.addInitializer(function () { rec("init " + name + " " + typeof this); }); };
class S {
  @si("static method") static sm(): void {}
  static plain: any = rec("plain static field");
  @si("instance method") im(): void {}
  instancePlain: any = rec("plain instance field");
  static { rec("static block"); }
  constructor() { rec("ctor"); }
}
new S();
class NoFields {
  @si("only method") m(): void {}
  @si("static only") static s(): void {}
}
new NoFields();
class Der extends NoFields { constructor() { rec("before super"); super(); rec("after super"); } }
new Der();
show();
"#;

const AUTO_ACCESSORS: &str = r#"
class T {
  accessor a: number = 1;
  static accessor b: string = "s";
  accessor c: number | undefined;
  accessor d: number[] = [1, 2];
}
const t = new T();
rec("t " + t.a + " " + T.b + " " + t.c + " " + t.d + " " + Object.getOwnPropertyNames(T.prototype).join(","));
t.a = 10; T.b = "z"; t.c = 3;
rec("t " + t.a + " " + T.b + " " + t.c + " " + JSON.stringify(Object.keys(t)));
show();
"#;

const EXPORTS: &str = "export const marker = 1;\nconst d = (value: any, ctx: any): any => { console.log(\"decorated\", ctx.kind, ctx.name); };\n@d export class Exported { @d m(): void {} }\nexport @d class Exported2 {}\nconsole.log(typeof Exported, typeof Exported2, Exported.name, marker);\n";

/// Each of these throws while the class is defined; the error is the output.
const FAILURES: &[(&str, &str)] = &[
    ("method-returns-non-function", "const bad = (): any => (value: any, ctx: any): any => 5;\nclass A { @bad() m(): void {} }\n"),
    ("field-returns-non-function", "const bad = (): any => (value: any, ctx: any): any => 5;\nclass A { @bad() f: number = 1; }\n"),
    ("class-returns-non-function", "const bad = (): any => (value: any, ctx: any): any => 5;\n@bad() class A {}\n"),
    ("accessor-returns-non-object", "const bad = (): any => (value: any, ctx: any): any => 5;\nclass A { @bad() accessor a: number = 1; }\n"),
    ("addInitializer-non-function", "class A { @((v: any, ctx: any): void => { ctx.addInitializer(5); }) m(): void {} }\n"),
    ("addInitializer-after-decoration", "let saved: any;\nclass A { @((v: any, ctx: any): void => { saved = ctx; }) m(): void {} }\nsaved.addInitializer(() => {});\n"),
    ("decorator-not-callable", "const notCallable: any = 5;\nclass A { @notCallable m(): void {} }\n"),
];

fn files(list: Vec<(&'static str, &'static str)>) -> &'static [(&'static str, &'static str)] {
    Box::leak(list.into_boxed_slice())
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn programs() -> Vec<Program> {
    let mut list = Vec::new();
    for (name, body) in [
        ("all-kinds", ALL_KINDS),
        ("class-decorators", CLASS_DECORATORS),
        ("order-and-forms", ORDER_AND_FORMS),
        ("metadata", METADATA),
        ("access-objects", ACCESS),
        ("static-and-instance-injection", STATIC_INJECTION),
        ("auto-accessors-without-decorators", AUTO_ACCESSORS),
    ] {
        for module in ["es2022", "commonjs"] {
            list.push(Program {
                name: Box::leak(format!("{name}-{module}").into_boxed_str()),
                module,
                files: files(vec![(
                    "main.ts",
                    leak(format!("export {{}};\n{PRELUDE}{body}")),
                )]),
                fails: false,
            });
        }
    }
    for module in ["es2022", "commonjs"] {
        list.push(Program {
            name: Box::leak(format!("exports-{module}").into_boxed_str()),
            module,
            files: files(vec![("main.ts", EXPORTS)]),
            fails: false,
        });
    }
    for (name, body) in FAILURES {
        list.push(Program {
            name: Box::leak(format!("failure-{name}").into_boxed_str()),
            module: "es2022",
            files: files(vec![("main.ts", leak(format!("export {{}};\n{body}")))]),
            fails: true,
        });
    }
    list
}

fn run_program(program: &Program) -> (String, String, String) {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = env::temp_dir().join(format!(
        "bluets-dec-{}-{}",
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
    let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&input)
        .args([
            "build",
            "main.ts",
            "--target",
            "es2022",
            "--module",
            if commonjs { "commonjs" } else { "esnext" },
            "--out-dir",
        ])
        .arg(&blue)
        .output()
        .unwrap();
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
    let _ = Command::new(tsc)
        .current_dir(&input)
        .args([
            "--target",
            "ES2022",
            "--pretty",
            "false",
            "--skipLibCheck",
            "--module",
            program.module,
            "--outDir",
        ])
        .arg(&reference)
        .arg("main.ts")
        .output()
        .unwrap();
    assert!(
        reference.join("main.js").exists(),
        "{}: tsc emitted nothing",
        program.name
    );
    let blue_run = Command::new(&node)
        .arg(blue.join("main.js"))
        .output()
        .unwrap();
    let tsc_run = Command::new(&node)
        .arg(reference.join("main.js"))
        .output()
        .unwrap();
    let emitted = fs::read_to_string(blue.join("main.js")).unwrap();
    let _ = fs::remove_dir_all(&root);
    if program.fails {
        // The thrown error's `Name: message` line, which does not depend on paths.
        let line = |stderr: &[u8]| {
            String::from_utf8_lossy(stderr)
                .lines()
                .find(|line| line.contains("Error: "))
                .unwrap_or("")
                .to_string()
        };
        assert!(
            !blue_run.status.success() && !tsc_run.status.success(),
            "{}: both must throw",
            program.name
        );
        return (line(&blue_run.stderr), line(&tsc_run.stderr), emitted);
    }
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
fn pinned_standard_decorators_print_what_typescript_prints() {
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
