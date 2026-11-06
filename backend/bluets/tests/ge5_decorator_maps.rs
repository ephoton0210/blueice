// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-boundary regressions for relocated decorator and metadata origins.
use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource, SourceMap,
};

fn emitted(source: &str, module_kind: ModuleKind) -> (String, SourceMap) {
    emitted_target(source, module_kind, EcmaTarget::Es2022)
}

fn emitted_target(
    source: &str,
    module_kind: ModuleKind,
    target: EcmaTarget,
) -> (String, SourceMap) {
    let id = "memory:///main.ts";
    let result = compile(
        id,
        &MapLoader::from([
            ModuleSource::new(id, source),
            ModuleSource::new("memory:///types.js", "export class Model {}\n"),
        ]),
        CompilerOptions {
            target,
            module_kind,
            libraries: Some(vec![EcmaTarget::Es2022]),
            experimental_decorators: true,
            emit_decorator_metadata: true,
            use_define_for_class_fields: Some(false),
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

fn assert_owner(javascript: &str, map: &SourceMap, offset: usize, original: (usize, usize)) {
    let (line, column) = position(javascript, offset);
    let owner = map
        .original_position_at(javascript, line, column, 100_000, 32_000_000)
        .unwrap()
        .expect("retained source role");
    assert_eq!((owner.line, owner.column_utf16), original);
}

#[test]
fn decorated_constructor_open_inherits_the_constructor_keyword() {
    let original = "function dec(...args: any[]): void {}\n@dec export class Box { constructor(@dec public value: number) {} }\n";
    for source in [
        original.to_string(),
        format!("/* 🧭 */ {}", original.replace('\n', "\r\n")),
    ] {
        for module in [ModuleKind::CommonJs, ModuleKind::Esm] {
            let (javascript, map) = emitted(&source, module);
            let offset = javascript.find("constructor(").unwrap() + "constructor".len();
            assert_owner(
                &javascript,
                &map,
                offset,
                position(&source, source.find("constructor").unwrap()),
            );
        }
    }
}

#[test]
fn decorated_class_assignment_gaps_retain_the_class_name_end() {
    let source = "function dec(...args: any[]): void {}\n@dec export class C {}\n";
    let (javascript, map) = emitted(source, ModuleKind::Esm);
    let name_end = source.find("class C").unwrap() + "class C".len();
    for marker in ["let C ", "C = __bluetsDecorate("] {
        let start = javascript.rfind(marker).unwrap();
        let gap = start + if marker.starts_with("let") { 5 } else { 1 };
        assert_owner(&javascript, &map, gap, position(source, name_end));
    }
}

#[test]
fn runtime_class_metadata_retains_annotation_interiors_and_endpoints() {
    let original = "import { Model } from './types.js';\nfunction dec(...args: any[]): void {}\n@dec export class C { @dec property!: Model; @dec method(value: Model): Model { return value; } constructor(@dec value: Model) {} }\n";
    for source in [
        original.to_string(),
        format!("/* 🧭 */ {}", original.replace('\n', "\r\n")),
    ] {
        for module in [ModuleKind::CommonJs, ModuleKind::Esm] {
            let (javascript, map) = emitted(&source, module);
            let generated = javascript
                .match_indices("__bluetsMetadata(")
                .map(|(start, _)| {
                    let end = start + javascript[start..].find(')').unwrap();
                    (start, end)
                })
                .filter(|(start, end)| javascript[*start..*end].contains("Model"))
                .collect::<Vec<_>>();
            let annotations = source.match_indices(": Model").collect::<Vec<_>>();
            assert_eq!(generated.len(), 4);
            assert_eq!(annotations.len(), generated.len());
            for ((start, end), (annotation, _)) in generated.into_iter().zip(annotations) {
                let begin = start + javascript[start..end].rfind("Model").unwrap();
                for offset in begin..begin + "Model".len() {
                    assert_owner(&javascript, &map, offset, position(&source, annotation + 2));
                }
                assert_owner(
                    &javascript,
                    &map,
                    begin + "Model".len(),
                    position(&source, annotation + 2 + "Model".len()),
                );
            }
        }
    }
}

#[test]
fn metadata_array_transport_retains_the_preceding_native_endpoint() {
    let source = "class Model {}\nfunction dec(...args: any[]): void {}\n@dec export class C { constructor(@dec model: Model, count: number) {} }\n";
    let (javascript, map) = emitted(source, ModuleKind::Esm);
    let generated = javascript.rfind(", Number]").unwrap() + ", ".len();
    let annotation_end = source.find(": Model,").unwrap() + ": Model".len();
    for offset in generated..generated + "Number".len() {
        assert_owner(&javascript, &map, offset, position(source, annotation_end));
    }
}

#[test]
fn rest_parameter_prologue_keeps_native_signature_and_parameter_roles() {
    let original = "export function record(...args: any[]): number { return args.length; }\n";
    for source in [
        original.to_string(),
        format!("/* 🧭 */ {}", original.replace('\n', "\r\n")),
    ] {
        let (javascript, map) = emitted_target(&source, ModuleKind::Esm, EcmaTarget::Es5);
        let open = source.find('(').unwrap();
        let signature = javascript.find("record()").unwrap() + "record".len();
        for offset in signature..signature + "(){".len() {
            assert_owner(&javascript, &map, offset, position(&source, open));
        }
        let parameter = source.find("...").unwrap();
        let declaration = javascript.find("var args").unwrap();
        let end = declaration + javascript[declaration..].find(';').unwrap() + 1;
        for offset in declaration..end {
            assert_owner(&javascript, &map, offset, position(&source, parameter));
        }
        let loop_start = javascript.find("for (").unwrap();
        let (line, column) = position(&javascript, loop_start);
        assert_eq!(
            map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
                .unwrap(),
            None,
            "the generated for keyword cannot acquire the rest parameter's source"
        );
    }
}

#[test]
fn retained_legacy_class_publishes_initial_binding_before_decorators() {
    let source = "function dec(...args: any[]): void {}\n@dec export class C {}\n";
    let (javascript, map) = emitted(source, ModuleKind::CommonJs);
    let publications = javascript
        .match_indices("exports.C = C;")
        .collect::<Vec<_>>();
    assert_eq!(
        publications.len(),
        2,
        "initial publication and final update"
    );
    let decorator = javascript.rfind("C = __bluetsDecorate(").unwrap();
    assert!(publications[0].0 < decorator && decorator < publications[1].0);
    let name = source.find("class C").unwrap() + "class ".len();
    for offset in publications[0].0..publications[0].0 + "exports.C = C;".len() {
        assert_owner(&javascript, &map, offset, position(source, name));
    }
    for offset in publications[1].0..publications[1].0 + "exports.C = C;".len() {
        let (line, column) = position(&javascript, offset);
        assert_eq!(
            map.original_position_at(&javascript, line, column, 100_000, 32_000_000)
                .unwrap(),
            None,
            "the final publication transport is generated-only"
        );
    }
}
