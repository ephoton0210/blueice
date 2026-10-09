// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned independent grammar controls for nested and lexical contexts.

use blueice_bluejs::{parse_module_with_edition, parse_with_edition, SyntaxEdition};
use serde_json::Value;

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
    assert_eq!(cases.len(), 33);
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
    assert_eq!((accept, reject), (279, 84));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
