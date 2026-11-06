// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Node bridge contract: every token character and every generated trivia gap.
use super::{cli, declarations, decoded, inputs, javascript_maps};
use base64::Engine;
use blueice_bluets::{lex, Token, TokenKind};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn actual_map_text(actual: &cli::Observation, name: &str, output: &str) -> String {
    if name.ends_with("#inline") {
        let encoded = actual.outputs[output]
            .lines()
            .find_map(|line| {
                line.strip_prefix("//# sourceMappingURL=data:application/json;base64,")
            })
            .expect("actual inline JavaScript map");
        String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
        )
        .unwrap()
    } else {
        actual.outputs[name].clone()
    }
}

#[test]
fn inline_map_evidence_keeps_exact_decoded_bytes() {
    let raw = "{\"version\":3,\"mappings\":\"AAAA\"}\r\n";
    let javascript = format!(
        "const x=1;\n//# sourceMappingURL=data:application/json;base64,{}\n",
        base64::engine::general_purpose::STANDARD.encode(raw)
    );
    let actual = cli::Observation {
        success: true,
        diagnostics: json!([]),
        outputs: BTreeMap::from([("main.js".to_string(), javascript.clone())]),
        selected_inputs: BTreeSet::new(),
    };
    let decoded = actual_map_text(&actual, "main.js#inline", "main.js");
    assert_eq!(decoded.as_bytes(), raw.as_bytes());
    assert_ne!(
        inputs::sha(decoded.as_bytes()),
        inputs::sha(javascript.as_bytes())
    );
}

fn owner(text: &str, map: &Value, offset: usize) -> Value {
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    owner_at(map, line, column)
}

fn owner_at(map: &Value, line: usize, column: usize) -> Value {
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

fn span_owners(text: &str, map: &Value, start: usize, end: usize) -> Vec<Value> {
    let prefix = &text[..start];
    let mut line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let mut column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    let mut values = Vec::new();
    for character in text[start..end].chars() {
        for _ in 0..character.len_utf16() {
            values.push(owner_at(map, line, column));
            column += 1;
        }
        if character == '\n' {
            line += 1;
            column = 0;
        }
    }
    values
}

fn tokens(text: &str) -> Vec<Token> {
    lex("<Node-emitted>", text)
        .unwrap()
        .into_iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .collect()
}

fn aliases(tokens: &[Token], sources: &[&str]) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let shape = [
        "import",
        "{",
        "createRequire",
        "as",
        "",
        "}",
        "from",
        "\"module\"",
        ";",
        "const",
        "",
        "=",
        "",
        "(",
        "import",
        ".",
        "meta",
        ".",
        "url",
        ")",
        ";",
    ];
    if tokens.len() < shape.len()
        || !tokens[..shape.len()]
            .iter()
            .zip(shape)
            .all(|(token, expected)| expected.is_empty() || token.text == expected)
    {
        return result;
    }
    assert_eq!(tokens[4].text, tokens[12].text);
    for (index, role) in [(4, "$node.createRequire"), (10, "$node.require")] {
        assert!(
            sources
                .iter()
                .flat_map(|source| tokens_for_source(source))
                .all(|token| token != tokens[index].text),
            "only fresh generated bindings may be renamed"
        );
        result.insert(tokens[index].text.clone(), role.into());
    }
    result
}

fn tokens_for_source(source: &str) -> Vec<String> {
    lex("<source>", source)
        .unwrap()
        .into_iter()
        .map(|token| token.text)
        .collect()
}

fn runs(values: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut result = Vec::new();
    for value in values {
        if result.last() != Some(&value) {
            result.push(value);
        }
    }
    result
}

pub(super) fn compare(
    native: &str,
    native_map: &Value,
    actual: &str,
    actual_map: &Value,
    sources: &[&str],
) -> Result<(), String> {
    if native_map["metadata"] != actual_map["metadata"] {
        return Err("complete metadata differs".into());
    }
    if native == actual && native_map != actual_map {
        return Err("identical JavaScript requires every decoded entry".into());
    }
    let wanted = tokens(native);
    let observed = tokens(actual);
    let native_aliases = aliases(&wanted, sources);
    let blue_aliases = aliases(&observed, sources);
    if wanted.len() != observed.len() {
        return Err(format!(
            "token inventory {} / {}",
            observed.len(),
            wanted.len()
        ));
    }
    let mut native_end = 0;
    let mut blue_end = 0;
    for (index, (a, b)) in wanted.iter().zip(&observed).enumerate() {
        let role = |token: &Token, aliases: &BTreeMap<String, String>| {
            aliases.get(&token.text).unwrap_or(&token.text).clone()
        };
        if role(a, &native_aliases) != role(b, &blue_aliases) {
            return Err(format!("token {index}: {:?} / {:?}", b.text, a.text));
        }
        let interior =
            |text: &str, token: &Token, map: &Value| span_owners(text, map, token.start, token.end);
        let expected = interior(native, a, native_map);
        let observed = interior(actual, b, actual_map);
        let (expected, observed) = if a.text != b.text {
            if runs(expected.clone()).len() != 1 || runs(observed.clone()).len() != 1 {
                return Err(format!(
                    "generated alias {index} must have one uniform owner"
                ));
            }
            (runs(expected), runs(observed))
        } else {
            (expected, observed)
        };
        if expected != observed {
            return Err(format!(
                "token {index} {:?}: complete interior owner transitions {:?} / {:?}",
                a.text,
                interior(actual, b, actual_map),
                interior(native, a, native_map)
            ));
        }
        // Formatting may remove a gap or add a physical line break. Every
        // remaining gap character must follow Native's ordered owners; newline
        // resets and generated comments may only add unmapped runs. This does
        // not discard any token, source tuple or in-token transition.
        let allowed = runs(
            std::iter::once(owner(native, native_map, native_end))
                .chain(
                    native[native_end..a.start]
                        .char_indices()
                        .map(|(offset, _)| owner(native, native_map, native_end + offset)),
                )
                .chain(std::iter::once(owner(native, native_map, a.start))),
        );
        let mut cursor = 0;
        for value in span_owners(actual, actual_map, blue_end, b.start) {
            if value.is_null() && allowed.contains(&Value::Null) {
                continue;
            }
            if value.is_null() {
                // A physical newline resets a map even when Native formats the
                // same gap on one line. It cannot justify an inline source gap.
                if native_end == a.start
                    || actual[blue_end..b.start].contains('\n')
                    || native[native_end..a.start].contains('\n')
                {
                    continue;
                }
            }
            let Some(next) = allowed[cursor..]
                .iter()
                .position(|candidate| candidate == &value)
            else {
                return Err(format!("gap before token {index} {:?}: owner {value} outside ordered Native gap {allowed:?}", a.text));
            };
            cursor += next;
        }
        native_end = a.end;
        blue_end = b.end;
    }
    if actual[blue_end..].char_indices().any(|(offset, _)| {
        !owner(actual, actual_map, blue_end + offset).is_null()
            && owner(actual, actual_map, blue_end + offset) != owner(native, native_map, native_end)
    }) {
        return Err("trailing generated trivia claims another source".into());
    }
    Ok(())
}

#[test]
fn node_bridge_contract_rejects_token_omission_interior_retargeting_and_helper_gap_grants() {
    let metadata = json!({"version":3,"sources":["main.ts"],"sourcesContent":["abc"],"names":[]});
    let native = json!({"metadata":metadata,"segments":[[0,0,0,0,0],[0,3,0,0,3]]});
    assert!(compare("abc;", &native, "abc;", &native, &["abc"]).is_ok());
    let changed = json!({"metadata":metadata,"segments":[[0,0,0,0,0],[0,1,0,0,2],[0,3,0,0,3]]});
    assert!(compare("abc;", &native, "abc;", &changed, &["abc"]).is_err());
    assert!(compare("abc;", &native, "abc ;", &changed, &["abc"])
        .unwrap_err()
        .contains("interior"));
    assert!(compare("abc;", &native, "abc", &native, &["abc"])
        .unwrap_err()
        .contains("inventory"));
    let injected =
        json!({"metadata":metadata,"segments":[[0,0,0,0,0],[0,3,0,0,3],[0,4,0,0,1],[1,0,0,0,3]]});
    assert!(compare(
        "abc;",
        &native,
        "abc; /* generated */\n",
        &injected,
        &["abc"]
    )
    .is_err());
}

#[test]
fn all_node_module_products_preserve_complete_token_owners_and_generated_gaps() {
    let temporary = cli::Temporary::new("node-role-contract");
    let mut configurations = 0;
    let mut maps = 0;
    let mut forms = BTreeSet::new();
    let mut failures = Vec::new();
    let mut observations = Vec::new();
    let mut verified = Vec::new();
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if !specification["form"]
            .as_str()
            .unwrap()
            .starts_with("ge2_module_helpers/node-")
        {
            continue;
        }
        configurations += 1;
        forms.insert(specification["form"].as_str().unwrap());
        let row = inputs::resolve(specification);
        let native = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(actual.success, "{}", specification["id"]);
        assert_eq!(actual.diagnostics, native["diagnostics"]);
        failures.extend(declarations::compare(specification, &row, native, &actual));
        observations.push(json!({"id":specification["id"], "index":index,
            "form":specification["form"], "repeatBuilds":2,
            "inputSha256":inputs::sha(serde_json::to_string(&row).unwrap().as_bytes()),
            "outputs":actual.outputs.iter().map(|(name, text)| (name, inputs::sha(text.as_bytes()))).collect::<BTreeMap<_,_>>() }));
        let sources = row["files"]
            .as_object()
            .unwrap()
            .values()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        for (name, expected) in native["maps"].as_object().unwrap() {
            if expected["kind"] != "javascript" {
                continue;
            }
            maps += 1;
            let output = expected["output"].as_str().unwrap();
            let native_js = inputs::blob(native["outputs"][output].as_str().unwrap());
            let native_raw = javascript_maps::native_map(native, name);
            let native_map = decoded::decoded(native_raw);
            let raw = actual_map_text(&actual, name, output);
            if let Err(error) = compare(
                native_js,
                &native_map,
                &actual.outputs[output],
                &decoded::decoded(&raw),
                &sources,
            ) {
                failures.push(format!("{}: {name}: {error}", specification["id"]));
            } else {
                verified.push(json!({"index":index,"id":specification["id"],"map":name,
                    "allCharactersClassified":true,
                    "nativeJavaScriptSha256":native["outputs"][output],
                    "nativeMapSha256":inputs::sha(native_raw.as_bytes()),
                    "actualJavaScriptSha256":inputs::sha(actual.outputs[output].as_bytes()),
                    "actualMapSha256":inputs::sha(raw.as_bytes())}));
            }
        }
    }
    assert_eq!(configurations, 146);
    assert_eq!(forms.len(), 8);
    assert_eq!(maps, 214);
    eprintln!("G-E5 Node bridge full role/gap contract: {configurations} configurations / {maps} JavaScript maps / two builds");
    assert!(
        failures.is_empty(),
        "{} Node role/gap failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    if let Some(path) = std::env::var_os("BLUEICE_GE5_NODE_ROLE_REPORT") {
        let binary = env!("CARGO_BIN_EXE_bluetsc");
        let report = json!({"schema":1, "contract":"complete token UTF-16 owners and ordered generated gaps; fresh Node bridge aliases only",
            "configurations":configurations, "javascriptMaps":maps,"forms":forms,"repeatBuilds":2,
            "inputSha256":inputs::sha(include_bytes!("../fixtures/ge5_source_maps/cases.json")),
            "compilerSha256":inputs::sha(&std::fs::read(binary).unwrap()),"completeFamily":true,
            "failures":failures,"observations":observations,"verified":verified});
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
