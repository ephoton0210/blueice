// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native boundaries for complete proposal-preserving decorator modules.
use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource, SourceMap,
};

#[allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[allow(dead_code)]
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[path = "oracle_support/ge5_preserved_decorator_roles.rs"]
mod preserved_roles;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

fn emitted(source: &str) -> (String, SourceMap) {
    emitted_module(source, ModuleKind::Esm)
}

fn emitted_module(source: &str, module_kind: ModuleKind) -> (String, SourceMap) {
    let id = "memory:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([ModuleSource::new(id, source)]),
        CompilerOptions {
            target: EcmaTarget::EsNext,
            module_kind,
            libraries: Some(vec![EcmaTarget::Es2022]),
            use_define_for_class_fields: Some(true),
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let artifact = result.output.unwrap().artifacts.remove(id).unwrap();
    (artifact.javascript, artifact.source_map.unwrap())
}

fn position(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

fn owner(javascript: &str, map: &SourceMap, offset: usize) -> Option<(usize, usize)> {
    let (line, column) = position(javascript, offset);
    map.original_position_at(javascript, line, column, 100_000, 32_000_000)
        .unwrap()
        .map(|owner| (owner.line, owner.column_utf16))
}

#[test]
fn arrow_blocks_and_empty_argument_objects_keep_native_opening_roles() {
    let source = "function take(value: any): any { return value; } export const read = (value: number) => { return take({}); };\n";
    let (javascript, map) = emitted(source);
    let arrow_body = javascript.find("=> {").unwrap() + "=> ".len();
    assert_eq!(
        owner(&javascript, &map, arrow_body),
        Some(position(source, source.find("=>").unwrap() + 2))
    );
    let object = javascript.find("{}").unwrap();
    assert_eq!(
        owner(&javascript, &map, object + 1),
        Some(position(source, source.find("{}").unwrap()))
    );
}

#[test]
fn unbraced_else_and_instanceof_keep_native_control_and_operation_roles() {
    let source = "class Box {} export function check(value: any): boolean { let result = false; if (value) result = true; else result = false; return value instanceof Box; }\n";
    let (javascript, map) = emitted(source);
    let otherwise = javascript.find("else").unwrap();
    for offset in otherwise..otherwise + "else".len() {
        assert_eq!(owner(&javascript, &map, offset), None);
    }
    let operation = javascript.find("instanceof").unwrap();
    let left_end = source.rfind("value instanceof").unwrap() + "value".len();
    for offset in operation..operation + "instanceof".len() {
        assert_eq!(
            owner(&javascript, &map, offset),
            Some(position(source, left_end))
        );
    }
}

#[test]
fn erased_receiver_signatures_preserve_anonymous_function_identity() {
    for source in [
        "export const read: any = function(this: any): number { return 1; };\n",
        "export const read: any = function(this: any, ...args: any[]): number { return args.length; };\n",
    ] {
        let (javascript, map) = emitted(source);
        let function = javascript.find("function").unwrap();
        let open = function + "function".len();
        assert_eq!(owner(&javascript, &map, open), Some(position(source, source.find("function").unwrap())));
        if let Some(rest) = javascript.find("...") {
            assert_eq!(owner(&javascript, &map, open + 1), Some(position(source, source.find("...").unwrap())));
            for offset in rest..rest + 3 {
                assert_eq!(owner(&javascript, &map, offset), Some(position(source, source.find("...").unwrap())));
            }
        } else {
            let body = javascript[open..].find('{').unwrap() + open;
            for offset in [open + 1, body] {
                assert_eq!(owner(&javascript, &map, offset), Some(position(source, source.find("function").unwrap())));
            }
        }
    }
}

#[test]
fn returned_anonymous_class_close_is_generated_only() {
    let source = "export function make(value: any): any { return class extends value { static ready = true; }; }\n";
    let (javascript, map) = emitted(source);
    let close = javascript.rfind("};").unwrap();
    assert_eq!(owner(&javascript, &map, close), None);
    let heritage = source.find("extends").unwrap();
    for gap in [
        javascript.find("class extends").unwrap() + "class".len(),
        javascript.find("extends").unwrap() + "extends".len(),
    ] {
        assert_eq!(
            owner(&javascript, &map, gap),
            Some(position(source, heritage))
        );
    }
}

#[test]
fn preserved_decorators_and_private_names_keep_complete_native_interiors() {
    let original = "function dec(value: any, context: any): any { return value; } export class C { @dec #run(value: number): number { return value; } run(): number { return this.#run(1); } }\n";
    for source in [
        original.to_string(),
        format!("/* 🧭 */ {}", original.replace('\n', "\r\n")),
    ] {
        let (javascript, map) = emitted(&source);
        let decorator = javascript.find("@dec").unwrap();
        assert_eq!(
            owner(&javascript, &map, decorator),
            Some(position(&source, source.find("@dec").unwrap()))
        );
        for (generated, original) in [
            (
                javascript.find("#run").unwrap(),
                source.find("#run").unwrap(),
            ),
            (
                javascript.rfind("#run").unwrap(),
                source.rfind("#run").unwrap(),
            ),
        ] {
            for offset in generated..generated + "#run".len() {
                assert_eq!(
                    owner(&javascript, &map, offset),
                    Some(position(&source, original))
                );
            }
        }
    }
}

#[test]
fn arrow_default_and_adjacent_generic_return_keep_native_parameter_and_arrow_roles() {
    let source =
        "export const calculate=async(value:number=40):Promise<number>=>{return value;};\n";
    let (javascript, map) = emitted(source);
    let default = javascript.find("=40").unwrap();
    assert_eq!(
        owner(&javascript, &map, default),
        Some(position(source, source.find("value").unwrap()))
    );
    let arrow = javascript.find("=>").unwrap();
    for offset in arrow..arrow + 2 {
        assert_eq!(
            owner(&javascript, &map, offset),
            Some(position(source, source.find("=>").unwrap()))
        );
    }
}

#[test]
fn original_comment_utf16_interiors_survive_a_generated_commonjs_prologue() {
    let source = "/* 🧭 */ export const value: number = 1;\n";
    let (javascript, map) = emitted_module(source, ModuleKind::CommonJs);
    let begin = javascript.find("/* 🧭 */").unwrap();
    let (line, column) = position(&javascript, begin);
    for unit in 0.."/* 🧭 */".encode_utf16().count() {
        let original = map
            .original_position_at(&javascript, line, column + unit, 100_000, 32_000_000)
            .unwrap()
            .unwrap();
        assert_eq!((original.line, original.column_utf16), (0, 0));
    }
    let end = begin + "/* 🧭 */".len();
    assert_eq!(
        owner(&javascript, &map, end),
        Some(position(source, "/* 🧭 */".len()))
    );
}

#[test]
fn preserved_private_accessor_modifiers_and_generator_stars_keep_native_roles() {
    let source = "function dec(value: any, context: any): any { return value; } class Base { get value(): number { return 1; } set value(n: number) {} method(): number { return 1; } } export class C extends Base { @dec get #value(): number { return super.value; } @dec set #value(n: number) { super.value = n; } @dec *#iterate(): Generator<number, void, unknown> { yield super.method(); } read(): number { this.#value = 2; return this.#value; } }\n";
    let (javascript, map) = emitted(source);
    let base_getter = javascript.find("get value").unwrap();
    assert_eq!(
        owner(&javascript, &map, base_getter),
        Some(position(source, source.find("get value").unwrap()))
    );
    for modifier in ["get", "set"] {
        let offset = javascript.find(&format!("{modifier} #value")).unwrap();
        for unit in 0..modifier.len() {
            assert_eq!(owner(&javascript, &map, offset + unit), None);
        }
        assert_eq!(owner(&javascript, &map, offset + modifier.len()), None);
    }
    let offset = javascript.find("*#iterate").unwrap();
    assert_eq!(
        owner(&javascript, &map, offset),
        Some(position(source, source.find("*#iterate").unwrap()))
    );
}
