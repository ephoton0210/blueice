// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A retained source callable finishes before its independently unmapped class brace.
use blueice_bluets::{
    compile, parse_module, CompilerOptions, Declaration, EcmaTarget, MapLoader, ModuleKind,
    ModuleSource,
};
use std::io::Write;
use std::process::{Command, Stdio};

fn position(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset];
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

#[test]
fn retained_original_last_callable_and_class_close_keep_separate_native_ends() {
    let mut rows = Vec::new();
    for prefix in ["", "/* G-E5 🧭 */\r\n"] {
        for member in [
            "read():number{return 40;}",
            "constructor(value:number){void value;}",
            "get value():number{return 40;}",
            "set value(value:number){void value;}",
        ] {
            for trailing_field in [false, true] {
                for spaced in [false, true] {
                    let source = format!(
                        "{prefix}export class C {{{member}{}{}}}\r\n",
                        if trailing_field {
                            "offset:number=40;"
                        } else {
                            ""
                        },
                        if spaced { " " } else { "" }
                    );
                    let parsed = parse_module("memory:///main.ts", &source).unwrap();
                    let Declaration::Class(class) = &parsed.declarations[0] else {
                        unreachable!()
                    };
                    let expected = position(&source, class.members[0].span.end);
                    for target in [EcmaTarget::Es2015, EcmaTarget::Es2022, EcmaTarget::EsNext] {
                        for module_kind in [
                            ModuleKind::CommonJs,
                            ModuleKind::Esm,
                            ModuleKind::Amd,
                            ModuleKind::Umd,
                            ModuleKind::System,
                        ] {
                            for define in [false, true] {
                                let mut outputs = Vec::new();
                                for maps in [false, true] {
                                    let id = "memory:///main.ts";
                                    let result = compile(
                                        id,
                                        &MapLoader::from([ModuleSource::new(id, &source)]),
                                        CompilerOptions {
                                            target,
                                            module_kind,
                                            use_define_for_class_fields: Some(define),
                                            source_map: maps,
                                            inline_sources: true,
                                            libraries: Some(vec![EcmaTarget::Es2022]),
                                            ..CompilerOptions::default()
                                        },
                                    );
                                    assert!(!result.has_errors(), "{:?}", result.diagnostics);
                                    let artifact = &result.output.unwrap().artifacts[id];
                                    let code = &artifact.javascript;
                                    outputs.push(
                                        code.lines()
                                            .filter(|line| {
                                                !line.starts_with("//# sourceMappingURL=")
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n"),
                                    );
                                    if !maps {
                                        continue;
                                    }
                                    rows.push(serde_json::json!({
                                    "source":source,"target":match target {EcmaTarget::Es2015=>"ES2015",EcmaTarget::Es2022=>"ES2022",_=>"ESNext"},
                                    "module":match module_kind {ModuleKind::CommonJs=>"CommonJS",ModuleKind::Esm=>"ESNext",ModuleKind::Amd=>"AMD",ModuleKind::Umd=>"UMD",_=>"System"},
                                    "kind":if member.starts_with("constructor") { "Constructor" } else if member.starts_with("get") { "GetAccessor" } else if member.starts_with("set") { "SetAccessor" } else { "MethodDeclaration" },
                                    "define":define,"code":code,"raw":artifact.source_map.as_ref().unwrap().to_json()
                                }));
                                    let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
                                    let start = tokens
                                        .windows(2)
                                        .position(|part| part[0].is("class") && part[1].is("C"))
                                        .unwrap();
                                    let open = start
                                        + tokens[start..]
                                            .iter()
                                            .position(|token| token.is("{"))
                                            .unwrap();
                                    let (mut depth, mut body_end, mut class_close) =
                                        (0, None, None);
                                    for token in &tokens[open..] {
                                        if token.is("{") {
                                            depth += 1;
                                        } else if token.is("}") {
                                            depth -= 1;
                                            if depth == 1 {
                                                body_end = Some(token.end);
                                            } else if depth == 0 {
                                                class_close = Some(token.start);
                                                break;
                                            }
                                        }
                                    }
                                    let map = artifact.source_map.as_ref().unwrap();
                                    let owner = |offset| {
                                        let (line, column) = position(code, offset);
                                        map.original_position_at(
                                            code, line, column, 100_000, 32_000_000,
                                        )
                                        .unwrap()
                                        .map(|owner| (owner.line, owner.column_utf16))
                                    };
                                    assert_eq!(
                                    owner(body_end.unwrap()),
                                    Some(expected),
                                    "{target:?} {module_kind:?} define={define} spaced={spaced} {member} callable.end"
                                );
                                    assert_eq!(
                                        owner(class_close.unwrap()),
                                        None,
                                        "class closing token remains independently generated"
                                    );
                                }
                                assert_eq!(
                                    outputs[0], outputs[1],
                                    "map/no-map executable text parity"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(rows.len(), 960);
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_retained_callable_ends.cjs",
    );
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}
