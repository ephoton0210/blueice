// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorated static super references retain their values and physical phases.
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and complete frozen source products"]
fn static_super_public_cli_reference_phases() {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools")
        .join("verify_ge5_static_super_public.cjs");
    let mut command = Command::new("node");
    command
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(compiler);
    if let Some(hash) = std::env::var_os("BLUEICE_GE5_EXPECTED_COMPILER_SHA256") {
        command.arg(hash);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn class_and_member_decorators_use_the_target_specific_static_receiver() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let source = "function dec(value:any,context:any):any{return function(this:any,initial:any){return initial+1;};}\nfunction replace(value:any):any{return class extends value{};}\n@replace export class C{@dec static \"static x\":number=1;@dec \"instance x\":number=2;static empty:number;}\n@replace export class Empty{static empty:any;}\nexport const result:any=[Object.hasOwn(C,'static x'),C['static x'],new C()['instance x'],Object.hasOwn(C,'empty'),Object.hasOwn(Object.getPrototypeOf(C),'empty'),Object.hasOwn(Empty,'empty'),Object.hasOwn(Object.getPrototypeOf(Empty),'empty')];\n";
    for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
        for define in [false, true] {
            for source_map in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, source)]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::CommonJs,
                        source_map,
                        use_define_for_class_fields: Some(define),
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let code = &result.output.unwrap().artifacts[id].javascript;
                let output = Command::new("node")
                    .arg("-e")
                    .arg(format!(
                        "var exports={{}};\n{code}\nconsole.log(JSON.stringify(exports.result));"
                    ))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let expected = if target < EcmaTarget::Es2022 {
                    "[true,2,3,true,false,false,false]"
                } else if define {
                    "[false,2,3,false,true,false,true]"
                } else {
                    "[false,2,3,false,true,false,false]"
                };
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout).trim(),
                    expected,
                    "{target:?}, define={define}, source_map={source_map}"
                );
            }
        }
    }
}

#[test]
fn nested_static_super_operands_keep_their_original_evaluation() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let source = "const events:any[]=[]; const Base:any=class{static get key():string{events.push('read');return 'amount';}static amount:number=7;}; function replace(value:any):any{return class extends value{};} @replace export class C extends Base{static value:any=super[super.key];static values:any=[(super.amount=super.amount+2),super.amount];}export const result:any=[C.value,C.values,C.amount,events];";
    for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
        for define in [false, true] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    use_define_for_class_fields: Some(define),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let code = &result.output.unwrap().artifacts[id].javascript;
            let output = Command::new("node")
                .arg("-e")
                .arg(format!(
                    "var exports={{}};\n{code}\nconsole.log(JSON.stringify(exports.result));"
                ))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                "[7,[9,7],9,[\"read\"]]",
                "{target:?}, define={define}"
            );
        }
    }
}

#[test]
fn retained_static_accessor_and_block_headers_keep_their_source_phase() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source = format!("{prefix}export class C {{ static get value():number {{ return 3; }} static {{ this.value; }} }}\n");
        let source_position = |offset: usize| {
            let before = &source[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        let modifier_end = source_position(source.find("static get").unwrap() + "static".len());
        let block_start = source_position(source.find("static {").unwrap());
        for target in [EcmaTarget::Es2022, EcmaTarget::EsNext] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.unwrap().artifacts[id];
            let code = &artifact.javascript;
            let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
            let getter = tokens.iter().find(|token| token.is("get")).unwrap();
            let block = tokens
                .windows(2)
                .find(|pair| pair[0].is("static") && pair[1].is("{"))
                .unwrap();
            let owner = |offset: usize| {
                let before = &code[..offset];
                let line = before.bytes().filter(|byte| *byte == b'\n').count();
                let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
                artifact
                    .source_map
                    .as_ref()
                    .unwrap()
                    .original_position_at(code, line, column, 100_000, 32_000_000)
                    .unwrap()
                    .map(|point| (point.line, point.column_utf16))
            };
            for offset in getter.start..=getter.end {
                assert_eq!(
                    owner(offset),
                    Some(modifier_end),
                    "{target:?}: accessor modifier"
                );
            }
            for offset in [block[0].end, block[1].start, block[1].end] {
                assert_eq!(
                    owner(offset),
                    Some(block_start),
                    "{target:?}: static block header"
                );
            }
        }
    }
}

#[test]
fn class_only_static_field_value_ends_at_its_assignment_phase() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source = format!("{prefix}const Base:any=class{{}};function replace(value:any):any{{return class extends value{{}};}} @replace export class C extends Base{{static value:number=5;static result:any=[super['amount']];}}\n");
        let end = source.find("value:number").unwrap() + "value".len();
        let before = &source[..end];
        let expected = (
            before.bytes().filter(|byte| *byte == b'\n').count(),
            before.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        for target in [EcmaTarget::Es2022, EcmaTarget::EsNext] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    use_define_for_class_fields: Some(false),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.unwrap().artifacts[id];
            let code = &artifact.javascript;
            let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
            let value = tokens.iter().find(|token| token.text == "5").unwrap();
            let before = &code[..value.end];
            let point = artifact
                .source_map
                .as_ref()
                .unwrap()
                .original_position_at(
                    code,
                    before.bytes().filter(|byte| *byte == b'\n').count(),
                    before.rsplit('\n').next().unwrap().encode_utf16().count(),
                    100_000,
                    32_000_000,
                )
                .unwrap();
            assert_eq!(
                point.map(|point| (point.line, point.column_utf16)),
                Some(expected),
                "{target:?}: complete static assignment value.end"
            );
        }
    }
}

#[test]
fn generated_class_metadata_does_not_inherit_the_source_heritage() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let source = "const Base:any=class{};function replace(value:any):any{return class extends value{};} @replace export class C extends Base{static value:number=5;}";
    for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                target,
                module_kind: ModuleKind::CommonJs,
                source_map: true,
                use_define_for_class_fields: Some(false),
                libraries: Some(vec![EcmaTarget::Es2022]),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
        let pair = tokens
            .windows(2)
            .find(|pair| {
                matches!(pair[0].text.as_str(), "const" | "var") && pair[1].text == "_metadata"
            })
            .unwrap();
        for offset in pair[0].start..pair[1].end {
            let before = &code[..offset];
            let point = artifact
                .source_map
                .as_ref()
                .unwrap()
                .original_position_at(
                    code,
                    before.bytes().filter(|byte| *byte == b'\n').count(),
                    before.rsplit('\n').next().unwrap().encode_utf16().count(),
                    100_000,
                    32_000_000,
                )
                .unwrap();
            assert!(
                point.is_none(),
                "{target:?}: synthetic metadata header acquired source authority"
            );
        }
    }
}

#[test]
fn static_super_read_array_separator_keeps_the_logical_get_end() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source = format!("{prefix}const Base:any=class{{}};function replace(value:any):any{{return class extends value{{}};}} @replace export class C extends Base{{static result:any=[super['amount'],super['method'](2)];}}\n");
        let end = source.find("super['amount']").unwrap() + "super".len();
        let before = &source[..end];
        let expected = (
            before.bytes().filter(|byte| *byte == b'\n').count(),
            before.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
            for define in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::CommonJs,
                        source_map: true,
                        use_define_for_class_fields: Some(define),
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.unwrap().artifacts[id];
                let code = &artifact.javascript;
                let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
                let comma = tokens
                    .windows(3)
                    .find(|items| items[0].is(")") && items[1].is(",") && items[2].is("("))
                    .unwrap();
                for offset in comma[1].start..comma[2].start {
                    let before = &code[..offset];
                    let point = artifact
                        .source_map
                        .as_ref()
                        .unwrap()
                        .original_position_at(
                            code,
                            before.bytes().filter(|byte| *byte == b'\n').count(),
                            before.rsplit('\n').next().unwrap().encode_utf16().count(),
                            100_000,
                            32_000_000,
                        )
                        .unwrap();
                    assert_eq!(
                        point.map(|p| (p.line, p.column_utf16)),
                        Some(expected),
                        "{target:?}/{define}: array separator logical-get phase"
                    );
                }
            }
        }
    }
}

#[test]
fn retained_super_field_semicolon_keeps_the_get_phase_before_statement_end() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for prefix in ["", "/* 🧭 */\r\n"] {
        for separator in [";", " \r\n ;"] {
            let source = format!("{prefix}const Base:any=class{{}};function replace(value:any):any{{return class extends value{{}};}} @replace export class C extends Base{{static result:any=super.amount{separator}}}\n");
            let begin = source.find("super.amount").unwrap();
            let point = |offset| {
                let before = &source[..offset];
                (
                    before.bytes().filter(|byte| *byte == b'\n').count(),
                    before.rsplit('\n').next().unwrap().encode_utf16().count(),
                )
            };
            let get_end = point(begin + "super".len());
            let statement_end = point(source[begin..].find(';').unwrap() + begin + 1);
            for target in [EcmaTarget::Es2022] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target,
                        module_kind: ModuleKind::CommonJs,
                        source_map: true,
                        use_define_for_class_fields: Some(true),
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let artifact = &result.output.unwrap().artifacts[id];
                let code = &artifact.javascript;
                let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
                let field = tokens
                    .windows(2)
                    .position(|items| items[0].is("static") && items[1].text == "result")
                    .unwrap();
                let semicolon = tokens[field..].iter().find(|token| token.is(";")).unwrap();
                for (offset, expected) in
                    [(semicolon.start, get_end), (semicolon.end, statement_end)]
                {
                    let before = &code[..offset];
                    let actual = artifact
                        .source_map
                        .as_ref()
                        .unwrap()
                        .original_position_at(
                            code,
                            before.bytes().filter(|byte| *byte == b'\n').count(),
                            before.rsplit('\n').next().unwrap().encode_utf16().count(),
                            100_000,
                            32_000_000,
                        )
                        .unwrap();
                    assert_eq!(actual.map(|p| (p.line, p.column_utf16)), Some(expected));
                }
            }
        }
    }
}

#[test]
fn plain_static_base_factory_close_has_no_source_role() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let source = "class Base{static value:number=3;static get amount():number{return this.value;}} export const result=Base.amount;";
    for define in [false, true] {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                module_kind: ModuleKind::CommonJs,
                source_map: true,
                use_define_for_class_fields: Some(define),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
        let close = &tokens
            .windows(4)
            .find(|items| {
                items[0].is("return")
                    && items[1].text == "Base"
                    && items[2].is(";")
                    && items[3].is("}")
            })
            .unwrap()[3];
        let before = &code[..close.start];
        let owner = artifact
            .source_map
            .as_ref()
            .unwrap()
            .original_position_at(
                code,
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
                100_000,
                32_000_000,
            )
            .unwrap();
        assert!(
            owner.is_none(),
            "plain static factory close acquired original class-body authority"
        );
    }
}

#[test]
fn super_field_assignment_block_preserves_its_original_static_header() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source=format!("{prefix}const Base:any=class{{}};function replace(value:any):any{{return class extends value{{}};}} @replace export class C extends Base{{static value:number=5;static result:any=[super['amount']];}}\n");
        let offset = source.find("static result").unwrap();
        let before = &source[..offset];
        let expected = (
            before.bytes().filter(|byte| *byte == b'\n').count(),
            before.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, &source)]),
            CompilerOptions {
                target: EcmaTarget::Es2022,
                module_kind: ModuleKind::CommonJs,
                source_map: true,
                use_define_for_class_fields: Some(false),
                libraries: Some(vec![EcmaTarget::Es2022]),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
        let header = tokens
            .windows(5)
            .find(|items| {
                items[0].is("static")
                    && items[1].is("{")
                    && items[2].is("this")
                    && items[3].is(".")
                    && items[4].text == "result"
            })
            .unwrap();
        for offset in header[0].start..header[1].end {
            let before = &code[..offset];
            let point = artifact
                .source_map
                .as_ref()
                .unwrap()
                .original_position_at(
                    code,
                    before.bytes().filter(|byte| *byte == b'\n').count(),
                    before.rsplit('\n').next().unwrap().encode_utf16().count(),
                    100_000,
                    32_000_000,
                )
                .unwrap();
            assert_eq!(point.map(|p| (p.line, p.column_utf16)), Some(expected));
        }
    }
}

#[test]
fn static_write_capture_restarts_at_its_own_reference_phase() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let source="const Base:any=class{};function replace(value:any):any{return class extends value{};}function rhs():number{return 2;} @replace export class C extends Base{static result:any=[(super.amount=rhs()),super.amount];}";
    let expected = source.find("super.amount").unwrap();
    for target in [EcmaTarget::Es5, EcmaTarget::Es2015, EcmaTarget::Es2022] {
        for define in [false, true] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    use_define_for_class_fields: Some(define),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.unwrap().artifacts[id];
            let code = &artifact.javascript;
            let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
            let key = tokens
                .iter()
                .find(|token| token.text == "\"amount\"")
                .unwrap();
            for offset in key.start..key.end {
                let before = &code[..offset];
                let point = artifact
                    .source_map
                    .as_ref()
                    .unwrap()
                    .original_position_at(
                        code,
                        before.bytes().filter(|byte| *byte == b'\n').count(),
                        before.rsplit('\n').next().unwrap().encode_utf16().count(),
                        100_000,
                        32_000_000,
                    )
                    .unwrap();
                assert_eq!(point.map(|p|(p.line,p.column_utf16)),Some((0,expected)),"{target:?}/{define}: the generated raw-key capture cannot inherit the earlier helper-body completion");
            }
        }
    }
}

#[test]
fn decorated_original_static_block_header_keeps_its_own_start() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source=format!("{prefix}const Base:any=class{{}};function replace(value:any):any{{return class extends value{{}};}} @replace class C extends Base{{static result:any;static {{try{{super['amount']=2;}}catch(e){{this.result=e instanceof TypeError;}}}}}}");
        let offset = source.find("static {try").unwrap();
        let before = &source[..offset];
        let expected = (
            before.bytes().filter(|byte| *byte == b'\n').count(),
            before.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        for define in [false, true] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target: EcmaTarget::Es2022,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    use_define_for_class_fields: Some(define),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.unwrap().artifacts[id];
            let code = &artifact.javascript;
            let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
            let header = tokens
                .windows(3)
                .find(|tokens| tokens[0].is("static") && tokens[1].is("{") && tokens[2].is("try"))
                .unwrap();
            for offset in header[0].start..=header[1].end {
                let before = &code[..offset];
                let point = artifact
                    .source_map
                    .as_ref()
                    .unwrap()
                    .original_position_at(
                        code,
                        before.bytes().filter(|byte| *byte == b'\n').count(),
                        before.rsplit('\n').next().unwrap().encode_utf16().count(),
                        100_000,
                        32_000_000,
                    )
                    .unwrap();
                assert_eq!(point.map(|p| (p.line, p.column_utf16)), Some(expected));
            }
        }
    }
}

#[test]
fn captured_static_receiver_dot_keeps_the_original_this_start() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for prefix in ["", "/* 🧭 */\r\n"] {
        let source=format!("{prefix}const Base:any=class{{}};function replace(value:any):any{{return class extends value{{}};}} @replace export class C extends Base{{static result:number=1;static {{this.result=2;}}}}");
        let start = source.find("this.result").unwrap();
        let before = &source[..start];
        let expected = (
            before.bytes().filter(|byte| *byte == b'\n').count(),
            before.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        for target in [EcmaTarget::Es5, EcmaTarget::Es2015] {
            let id = "memory:///main.ts";
            let result = compile(
                id,
                &MapLoader::from([ModuleSource::new(id, &source)]),
                CompilerOptions {
                    target,
                    module_kind: ModuleKind::CommonJs,
                    source_map: true,
                    use_define_for_class_fields: Some(false),
                    libraries: Some(vec![EcmaTarget::Es2022]),
                    ..CompilerOptions::default()
                },
            );
            assert!(!result.has_errors(), "{:?}", result.diagnostics);
            let artifact = &result.output.unwrap().artifacts[id];
            let code = &artifact.javascript;
            let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
            let access = tokens
                .windows(3)
                .rfind(|items| {
                    items[0].text.starts_with("_classThis")
                        && items[1].is(".")
                        && items[2].text == "result"
                })
                .unwrap();
            for offset in access[0].end..access[1].end {
                let before = &code[..offset];
                let point = artifact
                    .source_map
                    .as_ref()
                    .unwrap()
                    .original_position_at(
                        code,
                        before.bytes().filter(|byte| *byte == b'\n').count(),
                        before.rsplit('\n').next().unwrap().encode_utf16().count(),
                        100_000,
                        32_000_000,
                    )
                    .unwrap();
                assert_eq!(point.map(|p| (p.line, p.column_utf16)), Some(expected));
            }
        }
    }
}

#[test]
fn nested_static_declaration_retains_its_constructor_and_method_phases() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let source="export const result=(()=>{class Nested{static value:number=7;static run():number{return this.value;}}return Nested.run();})();";
    let method = source.find("run()").unwrap() + "run".len();
    for target in [EcmaTarget::Es5, EcmaTarget::Es2015] {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                target,
                module_kind: ModuleKind::CommonJs,
                source_map: true,
                use_define_for_class_fields: Some(false),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
        let (offset, expected) = if target == EcmaTarget::Es5 {
            let items = tokens
                .windows(4)
                .find(|items| {
                    items[0].is("return")
                        && items[1].text == "Nested"
                        && items[2].is(";")
                        && items[3].is("}")
                })
                .unwrap();
            (items[3].start, None)
        } else {
            let items = tokens
                .windows(4)
                .find(|items| {
                    items[0].text == "run"
                        && items[1].is("(")
                        && items[2].is(")")
                        && items[3].is("{")
                })
                .unwrap();
            (items[3].start, Some((0, method)))
        };
        let before = &code[..offset];
        let point = artifact
            .source_map
            .as_ref()
            .unwrap()
            .original_position_at(
                code,
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
                100_000,
                32_000_000,
            )
            .unwrap();
        assert_eq!(
            point.map(|p| (p.line, p.column_utf16)),
            expected,
            "{target:?}: the nested declaration retains its own source phase"
        );
    }
}

#[test]
fn empty_source_arrow_headers_end_before_their_generated_bodies() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    for gap in ["", " "] {
        let source=format!("const Base:any=class{{}};function replace(value:any):any{{return class extends value{{}};}} @replace class C extends Base{{static result:any=(()=>{{{gap}const read=()=>super.amount;return read();}})();}}");
        let outer = source.find("(()=>").unwrap() + 1;
        let inner = source.find("()=>super").unwrap();
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, &source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                module_kind: ModuleKind::CommonJs,
                source_map: true,
                use_define_for_class_fields: Some(false),
                libraries: Some(vec![EcmaTarget::Es2022]),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
        let body = tokens
            .windows(6)
            .find(|items| {
                items[0].is("function")
                    && items[1].is("(")
                    && items[2].is(")")
                    && items[3].is("{")
                    && items[4].is("var")
                    && items[5].text == "read"
            })
            .unwrap();
        let read = tokens
            .windows(7)
            .find(|items| {
                items[0].is("var")
                    && items[1].text == "read"
                    && items[2].is("=")
                    && items[3].is("function")
                    && items[4].is("(")
                    && items[5].is(")")
                    && items[6].is("{")
            })
            .unwrap();
        for (open, expected) in [(&body[3], outer), (&read[6], inner)] {
            for offset in open.start..=open.end {
                let before = &code[..offset];
                let point = artifact
                    .source_map
                    .as_ref()
                    .unwrap()
                    .original_position_at(
                        code,
                        before.bytes().filter(|byte| *byte == b'\n').count(),
                        before.rsplit('\n').next().unwrap().encode_utf16().count(),
                        100_000,
                        32_000_000,
                    )
                    .unwrap();
                assert_eq!(
                    point.map(|p| (p.line, p.column_utf16)),
                    Some((0, expected)),
                    "source arrow body header"
                );
            }
        }
    }
}

#[test]
fn legacy_scalar_super_key_completion_uses_the_assignment_phase() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let source="const Base:any=class{};function legacy(value:any):any{return value;} @legacy export class C extends Base{static x:any=super.amount;}";
    let expected = source.find("static x").unwrap() + "static x".len();
    let id = "memory:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(id, source)]),
        CompilerOptions {
            target: EcmaTarget::Es2022,
            module_kind: ModuleKind::CommonJs,
            source_map: true,
            experimental_decorators: true,
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let artifact = &result.output.unwrap().artifacts[id];
    let code = &artifact.javascript;
    let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
    let key = tokens
        .iter()
        .find(|token| token.text == "\"amount\"")
        .unwrap();
    let before = &code[..key.end];
    let owner = artifact
        .source_map
        .as_ref()
        .unwrap()
        .original_position_at(
            code,
            before.bytes().filter(|byte| *byte == b'\n').count(),
            before.rsplit('\n').next().unwrap().encode_utf16().count(),
            100_000,
            32_000_000,
        )
        .unwrap();
    assert_eq!(owner.map(|p| (p.line, p.column_utf16)), Some((0, expected)));
}

#[test]
fn legacy_class_static_assignments_separate_value_and_statement_completion() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let source = "function legacy(value:any):any{return value;} @legacy export class C{static scalar:number=7;}";
    let name_end = source.find("scalar").unwrap() + "scalar".len();
    let field_end = source.find("7;").unwrap() + 2;
    for target in [EcmaTarget::Es2022, EcmaTarget::EsNext] {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                target,
                module_kind: ModuleKind::CommonJs,
                source_map: true,
                experimental_decorators: true,
                use_define_for_class_fields: Some(false),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let tokens = blueice_bluets::lex("emitted.js", code).unwrap();
        let field = tokens
            .windows(4)
            .find(|items| {
                items[0].is("scalar") && items[1].is("=") && items[2].is("7") && items[3].is(";")
            })
            .unwrap();
        for (offset, expected) in [
            (field[2].end, name_end),
            (field[3].start, name_end),
            (field[3].end, field_end),
        ] {
            let before = &code[..offset];
            let owner = artifact
                .source_map
                .as_ref()
                .unwrap()
                .original_position_at(
                    code,
                    before.bytes().filter(|byte| *byte == b'\n').count(),
                    before.rsplit('\n').next().unwrap().encode_utf16().count(),
                    100_000,
                    32_000_000,
                )
                .unwrap();
            assert_eq!(
                owner.map(|point| (point.line, point.column_utf16)),
                Some((0, expected)),
                "{target:?}: exact legacy field completion"
            );
        }
    }
}
