// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX emit against pinned TypeScript (J.5.2): each program is built by `bluetsc`
//! and by `tsc` for the same JSX mode and both outputs must print the same thing
//! under Node, with a recording stand-in for the React runtime. That pins which
//! calls are made, with which props and children, in which module system, rather
//! than the text. The offline tests pin the emitted shape and the output names.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One program: a name, the `bluetsc`/`tsc` flags (`tsc` spelling; the module
/// system is part of them) and its files, the first being the entry.
struct Program {
    name: &'static str,
    flags: &'static [&'static str],
    files: &'static [(&'static str, &'static str)],
}

const PRELUDE: &str = "declare namespace JSX { interface Element { tag?: string } interface IntrinsicElements { div: any; span: any; i: any } }\nconst show = (value: any): void => console.log(JSON.stringify(value, (key: string, item: any) => typeof item === \"function\" ? \"fn:\" + item.name : item));\n";

const CLASSIC: &str = "/** @jsx h */\n/** @jsxFrag Fr */\nfunction h(tag: any, props: any, ...children: any[]): any { return { tag: typeof tag === \"function\" ? \"fn:\" + tag.name : tag, props, children }; }\nconst Fr = \"fragment\";\n";

const BODY: &str = r#"
function Comp(props: any): any { return props; }
const ns = { Inner: function Inner(props: any): any { return props; } };
const n = 3;
const obj = { spread: true, key: "from-spread" };
show(<div id="a" className='x&amp;y' hidden data-x={n} aria-label="l">plain</div>);
show(<div>
  first line
  second   line
  {n}
  trailing
</div>);
show(<div>   </div>);
show(<div>{" "}x{" "}</div>);
show(<div>&lt;&gt;&amp;&quot;&nbsp;&copy;&#65;&#x42;&unknown;</div>);
show(<Comp a={1} {...obj} b="two">child{n}<span /></Comp>);
show(<ns.Inner title="member" />);
show(<><div />text<span /></>);
show(<div {...{ a: 1 }} b={2} />);
show(<div a={(x: number): number => x + 1} />);
show(<div>{[1, 2].map((v: number) => <i>{v}</i>)}</div>);
show(<div x:y="ns" />);
show(<div a=<span /> />);
show(<div>{...[1, 2]}</div>);
show(<div>{/* comment */}</div>);
show(<div>{(n, n + 1)}</div>);
show(<div title={`t${n}`} />);
show(<Comp key="k" id="i" />);
show(<Comp id="i" key={n} />);
show(<Comp {...obj} key="after-spread" />);
show(<Comp {...{ lit: 1 }} key="after-literal" />);
show(<div key="only-key" />);
"#;

fn files(list: Vec<(&'static str, &'static str)>) -> &'static [(&'static str, &'static str)] {
    Box::leak(list.into_boxed_slice())
}

fn programs() -> Vec<Program> {
    // The leaked strings live for the whole test process.
    fn leak(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }
    let classic = leak(format!("{CLASSIC}{PRELUDE}{BODY}"));
    let automatic = leak(format!("{PRELUDE}export {{}};{BODY}"));
    let importing = leak(format!(
        "/** @jsx make */\nimport make from \"./factory.ts\";\nimport * as lib from \"./factory.ts\";\n{PRELUDE}show(<div id=\"x\">{{lib.label}}</div>);\n"
    ));
    vec![
        Program {
            name: "classic-esm",
            flags: &["--jsx", "react", "--module", "es2022"],
            files: files(vec![("main.tsx", classic)]),
        },
        Program {
            name: "classic-cjs",
            flags: &["--jsx", "react", "--module", "commonjs"],
            files: files(vec![("main.tsx", classic)]),
        },
        Program {
            name: "classic-options",
            flags: &["--jsx", "react", "--module", "es2022", "--jsxFactory", "h", "--jsxFragmentFactory", "Fr"],
            files: files(vec![(
                "main.tsx",
                leak(format!(
                    "function h(tag: any, props: any, ...children: any[]): any {{ return {{ tag, props, children }}; }}\nconst Fr = \"fragment\";\n{PRELUDE}show(<div id=\"a\">x<>y</></div>);\n"
                )),
            )]),
        },
        Program {
            name: "classic-imported-factory-cjs",
            flags: &["--jsx", "react", "--module", "commonjs"],
            files: files(vec![
                ("main.tsx", importing),
                (
                    "factory.ts",
                    "export default function make(tag: any, props: any, ...children: any[]): any { return { tag, props, children }; }\nexport const label = \"L\";\n",
                ),
            ]),
        },
        Program {
            name: "automatic-esm",
            flags: &["--jsx", "react-jsx", "--module", "es2022"],
            files: files(vec![("main.tsx", automatic)]),
        },
        Program {
            name: "automatic-cjs",
            flags: &["--jsx", "react-jsx", "--module", "commonjs"],
            files: files(vec![("main.tsx", automatic)]),
        },
        Program {
            name: "automatic-dev-esm",
            flags: &["--jsx", "react-jsxdev", "--module", "es2022"],
            files: files(vec![("main.tsx", automatic)]),
        },
        Program {
            name: "automatic-dev-cjs",
            flags: &["--jsx", "react-jsxdev", "--module", "commonjs"],
            files: files(vec![("main.tsx", automatic)]),
        },
        Program {
            name: "automatic-import-source",
            flags: &["--jsx", "react-jsx", "--module", "es2022", "--jsxImportSource", "alt"],
            files: files(vec![("main.tsx", leak(format!("export {{}};{PRELUDE}show(<div id=\"s\">x</div>);\n")))]),
        },
        Program {
            name: "pragma-import-source-and-runtime",
            flags: &["--jsx", "react", "--module", "es2022"],
            files: files(vec![(
                "main.tsx",
                leak(format!("/** @jsxRuntime automatic */\n/** @jsxImportSource alt */\nexport {{}};{PRELUDE}show(<div id=\"p\">x</div>);\n")),
            )]),
        },
        Program {
            name: "pragma-classic-overrides-automatic",
            flags: &["--jsx", "react-jsx", "--module", "es2022"],
            files: files(vec![(
                "main.tsx",
                leak(format!("/** @jsxRuntime classic */\n/** @jsx h */\nexport {{}};\nfunction h(tag: any, props: any, ...c: any[]): any {{ return {{ tag, props, c }}; }}\n{PRELUDE}show(<div id=\"c\">x</div>);\n")),
            )]),
        },
        Program {
            name: "use-strict-prologue-keeps-the-import-after-it",
            flags: &["--jsx", "react-jsx", "--module", "es2022"],
            files: files(vec![("main.tsx", leak(format!("\"use strict\";\nexport {{}};{PRELUDE}show(<div />);\n")))]),
        },
    ]
}

const RUNTIME_CJS: &str = "const nm = (t) => typeof t === 'function' ? 'fn:' + t.name : (typeof t === 'symbol' ? 'sym:' + t.description : t);\nconst make = (kind) => (type, props, ...rest) => ({ kind, t: nm(type), props, rest });\nexports.jsx = make('jsx'); exports.jsxs = make('jsxs'); exports.jsxDEV = make('jsxDEV');\nexports.Fragment = Symbol('Fragment');\nexports.createElement = (type, props, ...children) => ({ kind: 'createElement', t: nm(type), props, children });\n";
const RUNTIME_MJS: &str = "const nm = (t) => typeof t === 'function' ? 'fn:' + t.name : (typeof t === 'symbol' ? 'sym:' + t.description : t);\nconst make = (kind) => (type, props, ...rest) => ({ kind, t: nm(type), props, rest });\nexport const jsx = make('jsx'), jsxs = make('jsxs'), jsxDEV = make('jsxDEV');\nexport const Fragment = Symbol('Fragment');\nexport const createElement = (type, props, ...children) => ({ kind: 'createElement', t: nm(type), props, children });\n";

/// A recording stand-in for a JSX runtime package called `name`.
fn install_runtime(directory: &Path, name: &str) {
    let package = directory.join("node_modules").join(name);
    fs::create_dir_all(&package).unwrap();
    for file in ["jsx-runtime", "jsx-dev-runtime", "index"] {
        fs::write(package.join(format!("{file}.cjs")), RUNTIME_CJS).unwrap();
        fs::write(package.join(format!("{file}.mjs")), RUNTIME_MJS).unwrap();
    }
    fs::write(
        package.join("package.json"),
        r#"{"name":"x","exports":{".":{"import":"./index.mjs","require":"./index.cjs"},"./jsx-runtime":{"import":"./jsx-runtime.mjs","require":"./jsx-runtime.cjs"},"./jsx-dev-runtime":{"import":"./jsx-dev-runtime.mjs","require":"./jsx-dev-runtime.cjs"}}}"#,
    )
    .unwrap();
}

fn run_program(program: &Program) -> (String, String) {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    let root = env::temp_dir().join(format!(
        "bluets-jsx-{}-{}",
        std::process::id(),
        program.name
    ));
    let _ = fs::remove_dir_all(&root);
    let input = root.join("src");
    let blue = root.join("blue");
    let reference = root.join("tsc");
    for directory in [&input, &blue, &reference] {
        fs::create_dir_all(directory).unwrap();
    }
    for (file, text) in program.files {
        fs::write(input.join(file), text).unwrap();
    }
    let commonjs = program
        .flags
        .windows(2)
        .any(|pair| pair == ["--module", "commonjs"]);
    // BlueTSC, with the same flags in its own spelling.
    let blue_flags: Vec<String> = program
        .flags
        .iter()
        .map(|flag| match *flag {
            "--jsxFactory" => "--jsx-factory".to_string(),
            "--jsxFragmentFactory" => "--jsx-fragment-factory".to_string(),
            "--jsxImportSource" => "--jsx-import-source".to_string(),
            "es2022" if false => String::new(),
            other => other.to_string(),
        })
        .collect();
    let mut command = Command::new(env!("CARGO_BIN_EXE_bluetsc"));
    command
        .current_dir(&input)
        .args(["build", "main.tsx", "--out-dir"])
        .arg(&blue);
    let mut skip = false;
    for (index, flag) in blue_flags.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        if flag == "--module" {
            // `bluetsc` knows `esnext` and `commonjs`.
            let value = if blue_flags[index + 1] == "commonjs" {
                "commonjs"
            } else {
                "esnext"
            };
            command.args(["--module", value]);
            skip = true;
        } else {
            command.arg(flag);
        }
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
        for name in ["react", "alt"] {
            install_runtime(directory, name);
        }
    }
    // tsc, run from the source directory so the dev file name is the same.
    let mut command = Command::new(tsc);
    command
        .current_dir(&input)
        .args([
            "--target",
            "ES2022",
            "--pretty",
            "false",
            "--allowImportingTsExtensions",
            "--rewriteRelativeImportExtensions",
            "--skipLibCheck",
            "--outDir",
        ])
        .arg(&reference)
        .args(program.flags)
        .arg("main.tsx");
    let _ = command.output().unwrap();
    assert!(
        reference.join("main.js").exists(),
        "{}: tsc emitted nothing",
        program.name
    );
    let blue_run = Command::new(&node)
        .arg(blue.join("main.js"))
        .current_dir(&blue)
        .output()
        .unwrap();
    let tsc_run = Command::new(&node)
        .arg(reference.join("main.js"))
        .current_dir(&reference)
        .output()
        .unwrap();
    assert!(
        blue_run.status.success(),
        "{}: BlueTSC output failed: {}\n{}",
        program.name,
        String::from_utf8_lossy(&blue_run.stderr),
        fs::read_to_string(blue.join("main.js")).unwrap()
    );
    assert!(
        tsc_run.status.success(),
        "{}: tsc output failed: {}",
        program.name,
        String::from_utf8_lossy(&tsc_run.stderr)
    );
    let emitted = fs::read_to_string(blue.join("main.js")).unwrap();
    let _ = fs::remove_dir_all(&root);
    (
        String::from_utf8_lossy(&blue_run.stdout).into_owned() + &format!("\n--\n{emitted}"),
        String::from_utf8_lossy(&tsc_run.stdout).into_owned(),
    )
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_jsx_emit_prints_what_typescript_prints_in_every_mode() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));
    for program in programs() {
        let (blue, reference) = run_program(&program);
        let blue_output = blue.split("\n--\n").next().unwrap();
        assert!(
            !reference.trim().is_empty(),
            "{}: the program printed nothing",
            program.name
        );
        assert_eq!(
            blue_output,
            reference,
            "{}: BlueTSC and TypeScript print different output\n{}",
            program.name,
            blue.split("\n--\n").nth(1).unwrap_or("")
        );
    }
}
