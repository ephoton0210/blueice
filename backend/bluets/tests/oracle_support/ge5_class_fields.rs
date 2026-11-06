// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete Native roles for moved class initializer subtrees.
use super::{cli, declarations, decoded, inputs};
use serde_json::{json, Value};

fn owner(code: &str, map: &Value, offset: usize) -> Value {
    let prefix = &code[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    map["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|segment| {
            segment[0].as_u64().unwrap() as usize == line
                && segment[1].as_u64().unwrap() as usize <= column
        })
        .filter(|segment| segment.as_array().unwrap().len() > 2)
        .map_or(Value::Null, |segment| {
            json!(&segment.as_array().unwrap()[2..])
        })
}

#[test]
fn moved_field_initializers_retain_complete_native_subtree_roles() {
    use blueice_bluets::{lex, parse_module, Declaration, TokenKind};
    inputs::verify_authorities();
    assert_eq!(
        inputs::sha(include_bytes!("../fixtures/ge5_source_maps/cases.json")),
        "5b736263464566250127ae12beddc7dae3c488f48fcec018261c198ccc59e12e"
    );
    assert_eq!(
        inputs::sha(
            &std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/ge5_source_maps/reference.json")
            )
            .unwrap()
        ),
        "166af4ea3dd8b1af12aeb0c76c9f3f4a7f8b47aab0971bb2e93e72d01ded32f3"
    );
    let temporary = cli::Temporary::new("class-field-subtree");
    for index in [732, 733, 804, 805] {
        let specification = &inputs::input()["cases"][index];
        let row = inputs::resolve(specification);
        let expected = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(declarations::compare(specification, &row, expected, &actual).is_empty());
        let native = inputs::blob(expected["outputs"]["out/main.js"].as_str().unwrap());
        let blue = &actual.outputs["out/main.js"];
        let native_map = decoded::decoded(inputs::blob(
            expected["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let blue_map = decoded::decoded(&actual.outputs["out/main.js.map"]);
        let constructor = |source: &str| {
            let module = parse_module("emitted.js", source).unwrap();
            module
                .declarations
                .iter()
                .find_map(|declaration| {
                    let Declaration::Class(class) = declaration else {
                        return None;
                    };
                    (class.name == "Box").then(|| {
                        class
                            .members
                            .iter()
                            .find_map(|member| {
                                member
                                    .constructor
                                    .as_ref()
                                    .map(|constructor| constructor.span.clone())
                            })
                            .unwrap()
                    })
                })
                .unwrap()
        };
        let n = constructor(native);
        let a = constructor(blue);
        assert_eq!(
            owner(blue, &blue_map, a.end),
            owner(native, &native_map, n.end),
            "{} complete implicit constructor endpoint",
            specification["id"]
        );
        let tokens = |source: &str, span: &blueice_bluets::SourceSpan| {
            lex("emitted.js", source)
                .unwrap()
                .into_iter()
                .filter(|token| {
                    token.kind != TokenKind::Eof
                        && span.start <= token.start
                        && token.end <= span.end
                })
                .collect::<Vec<_>>()
        };
        let wanted = tokens(native, &n);
        let observed = tokens(blue, &a);
        assert_eq!(
            observed.iter().map(|token| &token.text).collect::<Vec<_>>(),
            wanted.iter().map(|token| &token.text).collect::<Vec<_>>(),
            "complete moved constructor/initializer token inventory"
        );
        for (index, (n, a)) in wanted.iter().zip(&observed).enumerate() {
            for (offset, character) in a.text.char_indices() {
                assert_eq!(
                    owner(blue, &blue_map, a.start + offset),
                    owner(native, &native_map, n.start + offset),
                    "{} complete moved initializer token {index} interior {offset}",
                    specification["id"]
                );
                assert_eq!(
                    character.len_utf16(),
                    1,
                    "the retained initializer fixture has ASCII token interiors"
                );
            }
            if index == 0 {
                continue;
            }
            let previous_n = &wanted[index - 1];
            let previous_a = &observed[index - 1];
            let mut allowed = Vec::new();
            for offset in previous_n.end..=n.start {
                let value = owner(native, &native_map, offset);
                if allowed.last() != Some(&value) {
                    allowed.push(value);
                }
            }
            let mut cursor = 0;
            for offset in previous_a.end..a.start {
                let value = owner(blue, &blue_map, offset);
                if value.is_null()
                    && (previous_n.end == n.start
                        || native[previous_n.end..n.start].contains('\n')
                        || blue[previous_a.end..a.start].contains('\n'))
                {
                    continue;
                }
                let found = allowed
                    .iter()
                    .enumerate()
                    .find(|(index, candidate)| *index >= cursor && **candidate == value);
                assert!(
                    found.is_some(),
                    "{} complete moved initializer ordered gap before token {index}: {value}",
                    specification["id"]
                );
                cursor = found.unwrap().0;
            }
        }
    }
}
