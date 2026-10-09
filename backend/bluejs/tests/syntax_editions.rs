// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned independent grammar controls for nested and lexical contexts.

use blueice_bluejs::{parse_module_with_edition, parse_with_edition, SyntaxEdition};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn edition(name: &str) -> SyntaxEdition {
    use SyntaxEdition::*;
    match name {
        "ES5" => Es5,
        "ES2015" => Es2015,
        "ES2016" => Es2016,
        "ES2017" => Es2017,
        "ES2018" => Es2018,
        "ES2019" => Es2019,
        "ES2020" => Es2020,
        "ES2021" => Es2021,
        "ES2022" => Es2022,
        "ES2023" => Es2023,
        "ESNext" => EsNext,
        _ => panic!("unknown recorded edition {name}"),
    }
}

#[test]
fn selected_editions_preserve_lexical_context_and_nested_template_grammar() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/syntax_editions.json")).unwrap();
    assert_eq!(reference["version"], "8.15.0");
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 31);
    let mut failures = Vec::new();
    let (mut accept, mut reject) = (0, 0);
    for case in cases {
        let source = case["source"].as_str().unwrap();
        let module = case["mode"] == "module";
        let verdicts = case["verdicts"].as_array().unwrap();
        assert_eq!(verdicts.len(), 11);
        for verdict in verdicts {
            let target = verdict[0].as_str().unwrap();
            let expected = verdict[1].as_bool().unwrap();
            let parsed = if module {
                parse_module_with_edition(source, edition(target)).map(|_| ())
            } else {
                parse_with_edition(source, edition(target)).map(|_| ())
            };
            if parsed.is_ok() != expected {
                failures.push(format!(
                    "{}/{target}: expected {expected}, received {parsed:?}",
                    case["name"]
                ));
            }
            if expected {
                accept += 1;
            } else {
                reject += 1;
                if let Err(error) = parsed {
                    assert!(
                        error.known_syntax && error.resource.is_none(),
                        "{}/{target}: {error:?}",
                        case["name"]
                    );
                }
            }
        }
    }
    assert_eq!((accept, reject), (264, 77));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires pinned Acorn 8.15.0 and Node"]
fn context_controls_match_the_actual_pinned_independent_parser() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = r#"
const fs = require('node:fs');
const acorn = require(process.argv[2])();
const reference = JSON.parse(fs.readFileSync(process.argv[1], 'utf8'));
if (reference.version !== acorn.version) throw Error('oracle version differs');
let observed = 0;
for (const item of reference.cases) {
    for (const [target, expected] of item.verdicts) {
        const ecmaVersion = target === 'ESNext' ? 'latest' : Number(target.slice(2));
        let actual = true;
        try { acorn.parse(item.source, {
            ecmaVersion, sourceType: item.mode,
            allowImportExportEverywhere: item.mode === 'module' && ecmaVersion === 5
        }); } catch { actual = false; }
        if (actual !== expected) throw Error(`${item.name}/${target}: oracle verdict changed`);
        observed++;
    }
}
if (observed !== 341) throw Error('incomplete context record');
"#;
    let output = Command::new("node")
        .args(["-e", script])
        .arg(root.join("tests/fixtures/syntax_editions.json"))
        .arg(root.join("../bluets/tests/fixtures/oracle_support/load_acorn.cjs"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
