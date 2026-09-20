// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Opt-in BlueTSC compatibility checks against a pinned external TypeScript
//! compiler.
//!
//! The fixtures are deliberately narrow: each one is already part of BlueTS's
//! documented language matrix. The oracle never makes a new syntax supported.

use blueice_bluets::{compile, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource, Severity};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const PINNED_TYPESCRIPT_VERSION: &str = "5.9.3";
static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

struct OracleCase {
    name: &'static str,
    modules: &'static [(&'static str, &'static str)],
    expected_stdout: Option<&'static str>,
    expected_diagnostics: &'static [ExpectedDiagnostic],
}

struct ExpectedDiagnostic {
    code: DiagnosticCode,
    line: usize,
}

const CASES: &[OracleCase] = &[
    OracleCase {
        name: "generic-property",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/generic-property/main.ts"),
        )],
        expected_stdout: Some("Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "optional-default",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/optional-default/main.ts"),
        )],
        expected_stdout: Some("2\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "optional-record",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/optional-record/main.ts"),
        )],
        expected_stdout: Some("missing\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "generic-declaration-module",
        modules: &[
            (
                "memory:///main.ts",
                include_str!("fixtures/typescript_oracle/generic-declaration-module/main.ts"),
            ),
            (
                "memory:///types/envelope.d.ts",
                include_str!(
                    "fixtures/typescript_oracle/generic-declaration-module/types/envelope.d.ts"
                ),
            ),
        ],
        expected_stdout: Some("Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "generic-constraint-default-declaration-module",
        modules: &[
            (
                "memory:///main.ts",
                include_str!(
                    "fixtures/typescript_oracle/generic-constraint-default-declaration-module/main.ts"
                ),
            ),
            (
                "memory:///types/envelope.d.ts",
                include_str!(
                    "fixtures/typescript_oracle/generic-constraint-default-declaration-module/types/envelope.d.ts"
                ),
            ),
        ],
        expected_stdout: Some("Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "explicit-generic-call",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/explicit-generic-call/main.ts"),
        )],
        expected_stdout: Some("Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "generic-interface-heritage",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/generic-interface-heritage/main.ts"),
        )],
        expected_stdout: Some("Ada:user:account\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "generic-interface-heritage-declaration-module",
        modules: &[
            (
                "memory:///main.ts",
                include_str!(
                    "fixtures/typescript_oracle/generic-interface-heritage-declaration-module/main.ts"
                ),
            ),
            (
                "memory:///types/model.d.ts",
                include_str!(
                    "fixtures/typescript_oracle/generic-interface-heritage-declaration-module/types/model.d.ts"
                ),
            ),
        ],
        expected_stdout: Some("Ada:user:account\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "function-overload",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-overload/main.ts"),
        )],
        expected_stdout: Some("Ada:2\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "generic-function-overload",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/generic-function-overload/main.ts"),
        )],
        expected_stdout: Some("Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "default-function-export",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/default-function-export/main.ts"),
        )],
        expected_stdout: Some("Hello, Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "default-value-export",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/default-value-export/main.ts"),
        )],
        expected_stdout: Some("Hello, Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "named-value-export",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/named-value-export/main.ts"),
        )],
        expected_stdout: Some("Hello, Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "boolean-conditional-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/boolean-conditional-expression/main.ts"),
        )],
        expected_stdout: Some("true:true:42:false:-41:-42:41:41\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "typeof-void-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/typeof-void-expression/main.ts"),
        )],
        expected_stdout: Some("number:undefined\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "nullish-coalescing-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/nullish-coalescing-expression/main.ts"),
        )],
        expected_stdout: Some("guest:42\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "bitwise-shift-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/bitwise-shift-expression/main.ts"),
        )],
        expected_stdout: Some("34:10:10\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "exponentiation-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/exponentiation-expression/main.ts"),
        )],
        expected_stdout: Some("512:0.125:4\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "compound-assignment-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/compound-assignment-expression/main.ts"),
        )],
        expected_stdout: Some("1:42:5:2\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "update-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/update-expression/main.ts"),
        )],
        expected_stdout: Some("1:3:2:2:1\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "comma-sequence-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/comma-sequence-expression/main.ts"),
        )],
        expected_stdout: Some("3\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "array-literal-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/array-literal-expression/main.ts"),
        )],
        expected_stdout: Some("object:3:2\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "array-spread-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/array-spread-expression/main.ts"),
        )],
        expected_stdout: Some("4:3\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "object-literal-property-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/object-literal-property-expression/main.ts"),
        )],
        expected_stdout: Some("Ada:42\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "object-spread-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/object-spread-expression/main.ts"),
        )],
        expected_stdout: Some("Grace:Countess\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "template-literal-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/template-literal-expression/main.ts"),
        )],
        expected_stdout: Some("BlueTS!\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "property-assignment-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/property-assignment-expression/main.ts"),
        )],
        expected_stdout: Some("Grace\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "object-shorthand-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/object-shorthand-expression/main.ts"),
        )],
        expected_stdout: Some("Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "string-escape-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/string-escape-expression/main.ts"),
        )],
        expected_stdout: Some("Ada\\Grace\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "template-escape-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/template-escape-expression/main.ts"),
        )],
        expected_stdout: Some("Ada\\Grace\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "template-identifier-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/template-identifier-expression/main.ts"),
        )],
        expected_stdout: Some("Hello, Ada!\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "object-literal-key-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/object-literal-key-expression/main.ts"),
        )],
        expected_stdout: Some("Ada:answer\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "member-call-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/member-call-expression/main.ts"),
        )],
        expected_stdout: Some("ADA\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "constructor-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/constructor-expression/main.ts"),
        )],
        expected_stdout: Some("object\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "property-delete-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/property-delete-expression/main.ts"),
        )],
        expected_stdout: Some("true\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "property-update-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/property-update-expression/main.ts"),
        )],
        expected_stdout: Some("1:2:3:3\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "relational-membership-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/relational-membership-expression/main.ts"),
        )],
        expected_stdout: Some("true\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "generic-arithmetic-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/generic-arithmetic-expression/main.ts"),
        )],
        expected_stdout: Some("42:40:84:20.5:1:Ada Lovelace\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "arithmetic-operand-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/arithmetic-operand-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 5,
        }],
    },
    OracleCase {
        name: "bitwise-operand-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/bitwise-operand-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 5,
        }],
    },
    OracleCase {
        name: "exponentiation-operand-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/exponentiation-operand-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 5,
        }],
    },
    OracleCase {
        name: "unary-exponentiation-base-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/unary-exponentiation-base-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::ParseError,
            line: 5,
        }],
    },
    OracleCase {
        name: "strict-equality-disjoint-primitive-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/strict-equality-disjoint-primitive-error/main.ts"
            ),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 5,
        }],
    },
    OracleCase {
        name: "typeof-assignment-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/typeof-assignment-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 5,
        }],
    },
    OracleCase {
        name: "nullish-coalescing-assignment-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/nullish-coalescing-assignment-error/main.ts"
            ),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 6,
        }],
    },
    OracleCase {
        name: "nullish-logical-mixing-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/nullish-logical-mixing-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::ParseError,
            line: 5,
        }],
    },
    OracleCase {
        name: "assignment-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/assignment-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 5,
        }],
    },
    OracleCase {
        name: "call-argument-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/call-argument-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 6,
        }],
    },
    OracleCase {
        name: "optional-record-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/optional-record-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 8,
        }],
    },
    OracleCase {
        name: "generic-constraint-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/generic-constraint-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 6,
        }],
    },
    OracleCase {
        name: "explicit-generic-constraint-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/explicit-generic-constraint-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 6,
        }],
    },
    OracleCase {
        name: "generic-interface-heritage-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/generic-interface-heritage-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 8,
        }],
    },
    OracleCase {
        name: "interface-heritage-override-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/interface-heritage-override-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 6,
        }],
    },
    OracleCase {
        name: "function-overload-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-overload-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 8,
            },
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 10,
            },
        ],
    },
];

/// This test is ignored in ordinary Rust builds because the reference compiler
/// is an explicitly provisioned test tool, not a BlueTSC or BlueTS dependency.
#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_bluetsc_oracle_matches_the_supported_fixture_matrix() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let node = env::var_os("BLUEICE_NODE").unwrap_or_else(|| "node".into());
    for case in CASES {
        run_case(case, &tsc, &node);
    }
}

fn run_case(case: &OracleCase, tsc: &Path, node: &std::ffi::OsStr) {
    let temporary = TestDirectory::new();
    write_esm_package(temporary.path());
    let sources = case
        .modules
        .iter()
        .map(|(id, text)| ModuleSource::new(*id, *text))
        .collect::<Vec<_>>();
    for (id, text) in case.modules {
        let path = temporary.path().join(disk_path(id));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let expected_declaration = expected_declaration(case);
    let options = CompilerOptions {
        source_map: true,
        declaration: expected_declaration.is_some(),
        ..CompilerOptions::default()
    };
    let compilation = compile("memory:///main.ts", &MapLoader::from(sources), options);
    let typescript_output = temporary.path().join("typescript");
    fs::create_dir_all(&typescript_output).unwrap();
    write_esm_package(&typescript_output);
    let input = temporary.path().join("main.ts");
    let tsc_output = run_tsc(
        tsc,
        &input,
        &typescript_output,
        case.expected_stdout.is_none(),
        expected_declaration.is_some(),
    );

    match case.expected_stdout {
        Some(expected_stdout) => {
            assert!(
                case.expected_diagnostics.is_empty(),
                "accepted fixture {} must not define expected diagnostics",
                case.name
            );
            assert!(
                !compilation.has_errors(),
                "BlueTS rejected accepted fixture {}: {:#?}",
                case.name,
                compilation.diagnostics
            );
            assert_success(
                &tsc_output,
                &format!(
                    "the pinned TypeScript compiler rejected fixture {}",
                    case.name
                ),
            );
            let artifact = &compilation.output.as_ref().unwrap().artifacts["memory:///main.ts"];
            let blueice_output = temporary.path().join("blueice.js");
            fs::write(&blueice_output, &artifact.javascript).unwrap();
            assert_source_map(&artifact.source_map.as_ref().unwrap().to_json(), "BlueTSC");
            assert_source_map(
                &fs::read_to_string(typescript_output.join("main.js.map")).unwrap(),
                "TypeScript",
            );
            if let Some(expected_declaration) = expected_declaration {
                assert_eq!(
                    artifact.declaration.as_deref(),
                    Some(expected_declaration),
                    "{} BlueTSC declaration",
                    case.name
                );
                assert_eq!(
                    fs::read_to_string(typescript_output.join("main.d.ts")).unwrap(),
                    expected_declaration,
                    "{} TypeScript declaration",
                    case.name
                );
            }
            let blueice_result = run_node(node, &blueice_output);
            let typescript_result = run_node(node, &typescript_output.join("main.js"));
            assert_success(&blueice_result, "Node could not execute BlueTSC output");
            assert_success(
                &typescript_result,
                "Node could not execute TypeScript output",
            );
            assert_eq!(
                blueice_result.stdout,
                expected_stdout.as_bytes(),
                "{} BlueTSC stdout",
                case.name
            );
            assert_eq!(
                typescript_result.stdout,
                expected_stdout.as_bytes(),
                "{} TypeScript stdout",
                case.name
            );
        }
        None => {
            assert!(
                compilation.has_errors(),
                "BlueTS accepted rejected fixture {}",
                case.name
            );
            assert_expected_diagnostics(case, &compilation.diagnostics);
            assert!(
                !tsc_output.status.success(),
                "the pinned TypeScript compiler accepted rejected fixture {}",
                case.name
            );
            assert_eq!(
                typescript_diagnostic_lines(&tsc_output),
                case.expected_diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.line)
                    .collect::<Vec<_>>(),
                "{} TypeScript diagnostic count or source lines",
                case.name
            );
        }
    }
}

fn expected_declaration(case: &OracleCase) -> Option<&'static str> {
    match case.name {
        "default-function-export" => {
            Some("export default function greeting(name: string): string;\n")
        }
        "default-value-export" => {
            Some("declare const greeting: string;\nexport default greeting;\n")
        }
        "named-value-export" => {
            Some("declare const label: string;\nexport { label as greeting };\n")
        }
        _ => None,
    }
}

fn assert_expected_diagnostics(case: &OracleCase, diagnostics: &[blueice_bluets::Diagnostic]) {
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect::<Vec<_>>();
    assert_eq!(
        errors.len(),
        case.expected_diagnostics.len(),
        "{} BlueTS diagnostic count: {diagnostics:#?}",
        case.name
    );
    for (diagnostic, expected) in errors.iter().zip(case.expected_diagnostics) {
        assert_eq!(
            diagnostic.code, expected.code,
            "{} BlueTS diagnostic code at {}:{}",
            case.name, diagnostic.span.module, diagnostic.span.start
        );
        let source = case
            .modules
            .iter()
            .find_map(|(module, source)| (*module == diagnostic.span.module).then_some(*source))
            .unwrap_or_else(|| {
                panic!(
                    "{} BlueTS reported an unexpected module {}",
                    case.name, diagnostic.span.module
                )
            });
        assert_eq!(
            source_line(source, diagnostic.span.start),
            expected.line,
            "{} BlueTS diagnostic location",
            case.name
        );
    }
}

fn source_line(source: &str, byte_offset: usize) -> usize {
    source[..byte_offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn typescript_diagnostic_lines(output: &Output) -> Vec<usize> {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines()
        .filter_map(|line| {
            let (_, location) = line.rsplit_once('(')?;
            let (location, _) = location.split_once("): error TS")?;
            location.split_once(',')?.0.parse().ok()
        })
        .collect()
}

fn disk_path(module_id: &str) -> &str {
    module_id
        .strip_prefix("memory:///")
        .expect("oracle fixture module IDs are memory-rooted")
}

fn assert_source_map(source_map: &str, producer: &str) {
    let value: Value = serde_json::from_str(source_map)
        .unwrap_or_else(|error| panic!("{producer} emitted invalid source-map JSON: {error}"));
    assert_eq!(value["version"], 3, "{producer} source map version");
    assert!(value["mappings"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(value["sources"]
        .as_array()
        .is_some_and(|value| !value.is_empty()));
}

fn pinned_bluetsc_oracle() -> PathBuf {
    env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable")
}

fn assert_pinned_version(tsc: &Path) {
    let output = Command::new(tsc).arg("--version").output().unwrap();
    assert_success(&output, "could not query the TypeScript compiler version");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("Version {PINNED_TYPESCRIPT_VERSION}"),
        "BLUEICE_BLUETSC_ORACLE must be the pinned BlueTSC oracle compiler"
    );
}

fn run_tsc(tsc: &Path, input: &Path, output: &Path, no_emit: bool, declaration: bool) -> Output {
    let mut command = Command::new(tsc);
    command.args([
        "--target",
        "ES2022",
        "--module",
        "ES2022",
        "--strict",
        "--pretty",
        "false",
        "--sourceMap",
    ]);
    if no_emit {
        command.arg("--noEmit");
    } else {
        command.arg("--outDir").arg(output);
    }
    if declaration {
        command.arg("--declaration");
    }
    command.arg(input).output().unwrap()
}

fn write_esm_package(directory: &Path) {
    fs::write(directory.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
}

fn run_node(node: &std::ffi::OsStr, input: &Path) -> Output {
    Command::new(node).arg(input).output().unwrap()
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "blueice-bluets-oracle-{}-{}-{sequence}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
