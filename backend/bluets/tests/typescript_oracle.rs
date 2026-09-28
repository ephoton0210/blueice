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
type NoEmitCase = (&'static str, &'static str, &'static [(usize, &'static str)]);
type NoEmitClassModuleCase = (
    &'static str,
    &'static str,
    &'static str,
    Option<(usize, &'static str)>,
);

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
        name: "optional-parameter-expression",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/optional-parameter-expression/main.ts"),
        )],
        expected_stdout: Some("guest:guest:Ada\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "default-parameter-expression",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/default-parameter-expression/main.ts"),
        )],
        expected_stdout: Some("42\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "optional-record",
        modules: &[(
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
        name: "array-hole-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/array-hole-expression/main.ts"),
        )],
        expected_stdout: Some("3:false:undefined\n"),
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
        name: "template-substitution-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/template-substitution-expression/main.ts"),
        )],
        expected_stdout: Some("BlueTSC: 42\n"),
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
        name: "function-expression-statement",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-expression-statement/main.ts"),
        )],
        expected_stdout: Some("5:5\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "function-throw-statement",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-throw-statement/main.ts"),
        )],
        expected_stdout: Some("ok\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "function-braced-if-statement",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-braced-if-statement/main.ts"),
        )],
        expected_stdout: Some("positive:other\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "function-braced-else-if-statement",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-braced-else-if-statement/main.ts"),
        )],
        expected_stdout: Some("many:one:none\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "function-braced-while-statement",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-braced-while-statement/main.ts"),
        )],
        expected_stdout: Some("0:6\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "function-braced-try-catch-finally-statement",
        modules: &[(
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/function-braced-try-catch-finally-statement/main.ts"
            ),
        )],
        expected_stdout: Some("7:5\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "function-typeof-local-guard",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-typeof-local-guard/main.ts"),
        )],
        expected_stdout: Some("1:42:1:42\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "callback-method-overload",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/callback-method-overload/main.ts"),
        )],
        expected_stdout: Some("3\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "optional-dot-property",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/optional-dot-property/main.ts"),
        )],
        expected_stdout: Some("41\n"),
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
        name: "computed-object-property-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/computed-object-property-expression/main.ts"),
        )],
        expected_stdout: Some("Ada:42\n"),
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
        name: "spread-argument-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/spread-argument-expression/main.ts"),
        )],
        expected_stdout: Some("42:object\n"),
        expected_diagnostics: &[],
    },
    OracleCase {
        name: "rest-parameter-expression",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/rest-parameter-expression/main.ts"),
        )],
        expected_stdout: Some("42\n"),
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
        name: "function-expression-statement-call-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/function-expression-statement-call-error/main.ts"
            ),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 8,
        }],
    },
    OracleCase {
        name: "function-throw-statement-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-throw-statement-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::ParseError,
            line: 6,
        }],
    },
    OracleCase {
        name: "function-braced-if-statement-call-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/function-braced-if-statement-call-error/main.ts"
            ),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 8,
        }],
    },
    OracleCase {
        name: "function-braced-else-if-statement-call-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/function-braced-else-if-statement-call-error/main.ts"
            ),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 10,
        }],
    },
    OracleCase {
        name: "function-return-fallthrough-error",
        modules: &[ (
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/function-return-fallthrough-error/main.ts"
            ),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::ReturnTypeMismatch,
            line: 5,
        }],
    },
    OracleCase {
        name: "function-braced-while-statement-call-error",
        modules: &[(
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/function-braced-while-statement-call-error/main.ts"
            ),
        )],
        expected_stdout: None,
        expected_diagnostics: &[
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 8,
            },
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 9,
            },
        ],
    },
    OracleCase {
        name: "function-braced-while-statement-return-error",
        modules: &[(
            "memory:///main.ts",
            include_str!(
                "fixtures/typescript_oracle/function-braced-while-statement-return-error/main.ts"
            ),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::ReturnTypeMismatch,
            line: 5,
        }],
    },
    OracleCase {
        name: "function-braced-try-catch-call-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-braced-try-catch-call-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[ExpectedDiagnostic {
            code: DiagnosticCode::TypeMismatch,
            line: 9,
        }],
    },
    OracleCase {
        name: "function-typeof-local-guard-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/function-typeof-local-guard-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 11,
            },
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 13,
            },
        ],
    },
    OracleCase {
        name: "callback-method-overload-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/callback-method-overload-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 15,
            },
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 16,
            },
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 17,
            },
        ],
    },
    OracleCase {
        name: "optional-dot-property-error",
        modules: &[(
            "memory:///main.ts",
            include_str!("fixtures/typescript_oracle/optional-dot-property-error/main.ts"),
        )],
        expected_stdout: None,
        expected_diagnostics: &[
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 10,
            },
            ExpectedDiagnostic {
                code: DiagnosticCode::TypeMismatch,
                line: 11,
            },
        ],
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

/// Class fixtures compare TypeScript syntax and overload rules.
/// BlueTS must still reject every class before output until J.3.1.3–J.3.1.6.
#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_method_boundary_matches_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 7] = [
        (
            "overloads",
            include_str!("fixtures/typescript_oracle/class-method-overloads/main.ts"),
            &[],
        ),
        (
            "record-return",
            include_str!("fixtures/typescript_oracle/class-method-record-return/main.ts"),
            &[],
        ),
        (
            "deferred-private",
            include_str!("fixtures/typescript_oracle/class-method-deferred-private/main.ts"),
            &[],
        ),
        (
            "orphan-signature",
            include_str!("fixtures/typescript_oracle/class-method-orphan-signature/main.ts"),
            &[(6, "TS2391")],
        ),
        (
            "interrupted-signature",
            include_str!("fixtures/typescript_oracle/class-method-interrupted-signature/main.ts"),
            &[(6, "TS2391")],
        ),
        (
            "incompatible-overload",
            include_str!("fixtures/typescript_oracle/class-method-incompatible-overload/main.ts"),
            &[(6, "TS2394")],
        ),
        (
            "duplicate-implementations",
            include_str!(
                "fixtures/typescript_oracle/class-method-duplicate-implementations/main.ts"
            ),
            &[(6, "TS2393"), (7, "TS2393")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_dual_binding_matches_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 7] = [
        (
            "dual-binding",
            include_str!("fixtures/typescript_oracle/class-dual-binding/main.ts"),
            &[],
        ),
        (
            "wrong-side",
            include_str!("fixtures/typescript_oracle/class-dual-binding-wrong-side/main.ts"),
            &[(8, "TS2741")],
        ),
        (
            "duplicate-name",
            include_str!("fixtures/typescript_oracle/class-duplicate-name/main.ts"),
            &[(5, "TS2300"), (6, "TS2300")],
        ),
        (
            "type-alias-collision",
            include_str!("fixtures/typescript_oracle/class-type-alias-collision/main.ts"),
            &[(5, "TS2300"), (6, "TS2300")],
        ),
        (
            "value-collision",
            include_str!("fixtures/typescript_oracle/class-value-collision/main.ts"),
            &[(5, "TS2451"), (6, "TS2451")],
        ),
        (
            "interface-then-class",
            "interface Reader {} class Reader {}",
            &[],
        ),
        (
            "class-then-interface",
            "class Reader {} interface Reader {}",
            &[],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_construction_matches_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 8] = [
        (
            "construction",
            include_str!("fixtures/typescript_oracle/class-construction/main.ts"),
            &[],
        ),
        (
            "constructor-overloads",
            include_str!("fixtures/typescript_oracle/class-construction-overloads/main.ts"),
            &[],
        ),
        (
            "inherited-deferred",
            include_str!(
                "fixtures/typescript_oracle/class-construction-inherited-deferred/main.ts"
            ),
            &[],
        ),
        (
            "argument-error",
            include_str!("fixtures/typescript_oracle/class-construction-argument-error/main.ts"),
            &[(6, "TS2345")],
        ),
        (
            "arity-error",
            include_str!("fixtures/typescript_oracle/class-construction-arity-error/main.ts"),
            &[(6, "TS2554")],
        ),
        (
            "default-arity-error",
            include_str!(
                "fixtures/typescript_oracle/class-construction-default-arity-error/main.ts"
            ),
            &[(6, "TS2554")],
        ),
        (
            "inferred-shape-error",
            include_str!(
                "fixtures/typescript_oracle/class-construction-inferred-shape-error/main.ts"
            ),
            &[(7, "TS2741")],
        ),
        (
            "nested-argument-error",
            include_str!(
                "fixtures/typescript_oracle/class-construction-nested-argument-error/main.ts"
            ),
            &[(7, "TS2345")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_constructor_validation_matches_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 9] = [
        (
            "valid-overloads",
            include_str!("fixtures/typescript_oracle/class-construction-overloads/main.ts"),
            &[],
        ),
        (
            "valid-default",
            include_str!("fixtures/typescript_oracle/class-constructor-valid-default/main.ts"),
            &[],
        ),
        (
            "missing-implementation",
            include_str!(
                "fixtures/typescript_oracle/class-constructor-missing-implementation/main.ts"
            ),
            &[(6, "TS2390")],
        ),
        (
            "interrupted-overload",
            include_str!(
                "fixtures/typescript_oracle/class-constructor-interrupted-overload/main.ts"
            ),
            &[(6, "TS2390")],
        ),
        (
            "duplicate-implementation",
            include_str!(
                "fixtures/typescript_oracle/class-constructor-duplicate-implementation/main.ts"
            ),
            &[(6, "TS2392"), (7, "TS2392")],
        ),
        (
            "incompatible-overload",
            include_str!(
                "fixtures/typescript_oracle/class-constructor-incompatible-overload/main.ts"
            ),
            &[(6, "TS2394")],
        ),
        (
            "unknown-type",
            include_str!("fixtures/typescript_oracle/class-constructor-unknown-type/main.ts"),
            &[(6, "TS2304")],
        ),
        (
            "invalid-default",
            include_str!("fixtures/typescript_oracle/class-constructor-invalid-default/main.ts"),
            &[(6, "TS2322")],
        ),
        (
            "overload-default",
            include_str!("fixtures/typescript_oracle/class-constructor-overload-default/main.ts"),
            &[(6, "TS2371")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_constructor_bodies_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 7] = [
        (
            "typed-locals",
            include_str!("fixtures/typescript_oracle/class-constructor-body-valid/main.ts"),
            &[],
        ),
        (
            "primitive-return",
            include_str!("fixtures/typescript_oracle/class-constructor-body-primitive-return/main.ts"),
            &[],
        ),
        (
            "aliased-primitive-return",
            include_str!("fixtures/typescript_oracle/class-constructor-body-aliased-primitive-return/main.ts"),
            &[],
        ),
        (
            "invalid-local",
            include_str!("fixtures/typescript_oracle/class-constructor-body-invalid-local/main.ts"),
            &[(7, "TS2322")],
        ),
        (
            "invalid-call",
            include_str!("fixtures/typescript_oracle/class-constructor-body-invalid-call/main.ts"),
            &[(8, "TS2345")],
        ),
        (
            "invalid-return",
            include_str!("fixtures/typescript_oracle/class-constructor-body-invalid-return/main.ts"),
            &[(9, "TS2741"), (9, "TS2409")],
        ),
        (
            "nested-return",
            include_str!("fixtures/typescript_oracle/class-constructor-body-nested-return/main.ts"),
            &[(10, "TS2741"), (10, "TS2409")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_method_body_scopes_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 7] = [
        (
            "typed-scopes",
            include_str!("fixtures/typescript_oracle/class-method-body-scopes/main.ts"),
            &[],
        ),
        (
            "instance-local",
            include_str!(
                "fixtures/typescript_oracle/class-method-body-invalid-instance-local/main.ts"
            ),
            &[(7, "TS2322")],
        ),
        (
            "static-local",
            include_str!(
                "fixtures/typescript_oracle/class-method-body-invalid-static-local/main.ts"
            ),
            &[(7, "TS2322")],
        ),
        (
            "invalid-call",
            include_str!("fixtures/typescript_oracle/class-method-body-invalid-call/main.ts"),
            &[(7, "TS2345")],
        ),
        (
            "unknown-parameter",
            include_str!("fixtures/typescript_oracle/class-method-body-unknown-parameter/main.ts"),
            &[(6, "TS2304")],
        ),
        (
            "invalid-default",
            include_str!("fixtures/typescript_oracle/class-method-body-invalid-default/main.ts"),
            &[(6, "TS2322")],
        ),
        (
            "overload-default",
            include_str!("fixtures/typescript_oracle/class-method-body-overload-default/main.ts"),
            &[(6, "TS2371")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_method_returns_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 8] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-method-returns-valid/main.ts"),
            &[],
        ),
        (
            "invalid-instance",
            include_str!(
                "fixtures/typescript_oracle/class-method-returns-invalid-instance/main.ts"
            ),
            &[(6, "TS2322")],
        ),
        (
            "invalid-static",
            include_str!("fixtures/typescript_oracle/class-method-returns-invalid-static/main.ts"),
            &[(6, "TS2322")],
        ),
        (
            "invalid-void",
            include_str!("fixtures/typescript_oracle/class-method-returns-invalid-void/main.ts"),
            &[(6, "TS2322")],
        ),
        (
            "bare-return",
            include_str!("fixtures/typescript_oracle/class-method-returns-bare/main.ts"),
            &[(6, "TS2322")],
        ),
        (
            "fallthrough",
            include_str!("fixtures/typescript_oracle/class-method-returns-fallthrough/main.ts"),
            &[(6, "TS2366")],
        ),
        (
            "unknown-type",
            include_str!("fixtures/typescript_oracle/class-method-returns-unknown-type/main.ts"),
            &[(6, "TS2304")],
        ),
        (
            "overload-body",
            include_str!("fixtures/typescript_oracle/class-method-returns-overload-body/main.ts"),
            &[(7, "TS2322")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_instance_this_class_bodies_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 6] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-instance-this-valid/main.ts"),
            &[],
        ),
        (
            "argument-error",
            include_str!("fixtures/typescript_oracle/class-instance-this-argument-error/main.ts"),
            &[(6, "TS2345")],
        ),
        (
            "wrong-side-call",
            include_str!("fixtures/typescript_oracle/class-instance-this-wrong-side-call/main.ts"),
            &[(7, "TS2576")],
        ),
        (
            "wrong-side-read",
            include_str!("fixtures/typescript_oracle/class-instance-this-wrong-side-read/main.ts"),
            &[(7, "TS2576")],
        ),
        (
            "inferred-result",
            include_str!(
                "fixtures/typescript_oracle/class-instance-this-inferred-return-error/main.ts"
            ),
            &[(6, "TS2322")],
        ),
        (
            "self-return",
            include_str!(
                "fixtures/typescript_oracle/class-instance-this-self-return-error/main.ts"
            ),
            &[(6, "TS2322")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_static_this_class_bodies_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 6] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-static-this-valid/main.ts"),
            &[],
        ),
        (
            "argument-error",
            include_str!("fixtures/typescript_oracle/class-static-this-argument-error/main.ts"),
            &[(7, "TS2345")],
        ),
        (
            "wrong-side-call",
            include_str!("fixtures/typescript_oracle/class-static-this-wrong-side-call/main.ts"),
            &[(7, "TS2339")],
        ),
        (
            "wrong-side-read",
            include_str!("fixtures/typescript_oracle/class-static-this-wrong-side-read/main.ts"),
            &[(7, "TS2339")],
        ),
        (
            "inferred-result",
            include_str!(
                "fixtures/typescript_oracle/class-static-this-inferred-return-error/main.ts"
            ),
            &[(7, "TS2322")],
        ),
        (
            "self-return",
            include_str!("fixtures/typescript_oracle/class-static-this-self-return-error/main.ts"),
            &[(7, "TS2741")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_instance_method_overloads_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 5] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-instance-overload-valid/main.ts"),
            &[],
        ),
        (
            "argument-error",
            include_str!(
                "fixtures/typescript_oracle/class-instance-overload-argument-error/main.ts"
            ),
            &[(11, "TS2769")],
        ),
        (
            "inferred-result",
            include_str!(
                "fixtures/typescript_oracle/class-instance-overload-inferred-error/main.ts"
            ),
            &[(11, "TS2322")],
        ),
        (
            "this-return",
            include_str!(
                "fixtures/typescript_oracle/class-instance-overload-this-return-error/main.ts"
            ),
            &[(9, "TS2322")],
        ),
        (
            "this-argument",
            include_str!(
                "fixtures/typescript_oracle/class-instance-overload-this-argument-error/main.ts"
            ),
            &[(9, "TS2769")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_static_method_overloads_on_values_and_this_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 5] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-static-overload-this-valid/main.ts"),
            &[],
        ),
        (
            "value-argument",
            include_str!("fixtures/typescript_oracle/class-static-overload-error/main.ts"),
            &[(10, "TS2769")],
        ),
        (
            "value-result",
            include_str!(
                "fixtures/typescript_oracle/class-static-overload-value-inferred-error/main.ts"
            ),
            &[(10, "TS2322")],
        ),
        (
            "this-argument",
            include_str!(
                "fixtures/typescript_oracle/class-static-overload-this-argument-error/main.ts"
            ),
            &[(9, "TS2769")],
        ),
        (
            "this-return",
            include_str!(
                "fixtures/typescript_oracle/class-static-overload-this-return-error/main.ts"
            ),
            &[(9, "TS2322")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_named_class_heritage_validation_matches_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 4] = [
        (
            "local-valid",
            include_str!("fixtures/typescript_oracle/class-heritage-local-valid/main.ts"),
            &[],
        ),
        (
            "unknown-base",
            include_str!("fixtures/typescript_oracle/class-heritage-unknown-base/main.ts"),
            &[(5, "TS2304")],
        ),
        (
            "nonconstructor-base",
            include_str!("fixtures/typescript_oracle/class-heritage-nonconstructor-base/main.ts"),
            &[(6, "TS2507")],
        ),
        (
            "forward-base",
            include_str!("fixtures/typescript_oracle/class-heritage-forward-base/main.ts"),
            &[(5, "TS2449")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
    assert_pinned_class_module_cases(
        &tsc,
        &[(
            "imported-valid",
            include_str!("fixtures/typescript_oracle/class-heritage-imported-valid/main.ts"),
            include_str!("fixtures/typescript_oracle/class-heritage-imported-valid/box.ts"),
            None,
        )],
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_local_class_heritage_cycles_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 2] = [
        (
            "self-cycle",
            include_str!("fixtures/typescript_oracle/class-heritage-self-cycle/main.ts"),
            &[(5, "TS2506")],
        ),
        (
            "mutual-cycle",
            include_str!("fixtures/typescript_oracle/class-heritage-mutual-cycle/main.ts"),
            &[(5, "TS2506"), (5, "TS2449"), (6, "TS2506")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_inherited_instance_methods_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 3] = [
        (
            "local-valid",
            include_str!("fixtures/typescript_oracle/class-inherited-instance-valid/main.ts"),
            &[],
        ),
        (
            "wrong-argument",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-instance-argument-error/main.ts"
            ),
            &[(8, "TS2345")],
        ),
        (
            "wrong-result",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-instance-result-error/main.ts"
            ),
            &[(8, "TS2322")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
    assert_pinned_class_module_cases(
        &tsc,
        &[(
            "imported-valid",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-instance-valid/main.ts"
            ),
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-instance-valid/box.ts"
            ),
            None,
        )],
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_inherited_static_overloads_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 3] = [
        (
            "local-valid",
            include_str!("fixtures/typescript_oracle/class-inherited-static-valid/main.ts"),
            &[],
        ),
        (
            "wrong-argument",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-static-argument-error/main.ts"
            ),
            &[(11, "TS2769")],
        ),
        (
            "wrong-result",
            include_str!("fixtures/typescript_oracle/class-inherited-static-result-error/main.ts"),
            &[(11, "TS2322")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
    assert_pinned_class_module_cases(
        &tsc,
        &[(
            "imported-valid",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-static-valid/main.ts"
            ),
            include_str!("fixtures/typescript_oracle/class-inherited-imported-static-valid/box.ts"),
            None,
        )],
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_inherited_constructor_signatures_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 5] = [
        (
            "local-valid",
            include_str!("fixtures/typescript_oracle/class-inherited-constructor-valid/main.ts"),
            &[],
        ),
        (
            "wrong-argument",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-constructor-argument-error/main.ts"
            ),
            &[(11, "TS2769")],
        ),
        (
            "wrong-arity",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-constructor-arity-error/main.ts"
            ),
            &[(7, "TS2554")],
        ),
        (
            "wrong-result",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-constructor-result-error/main.ts"
            ),
            &[(7, "TS2741")],
        ),
        (
            "own-constructor",
            include_str!(
                "fixtures/typescript_oracle/class-derived-own-constructor-argument-error/main.ts"
            ),
            &[(7, "TS2345")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
    assert_pinned_class_module_cases(
        &tsc,
        &[(
            "imported-valid",
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-constructor-valid/main.ts"
            ),
            include_str!(
                "fixtures/typescript_oracle/class-inherited-imported-constructor-valid/box.ts"
            ),
            None,
        )],
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_exported_local_derived_class_surfaces_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let box_module =
        include_str!("fixtures/typescript_oracle/class-export-inherited-direct/box.ts");
    let cases: [NoEmitClassModuleCase; 6] = [
        (
            "direct-valid",
            include_str!("fixtures/typescript_oracle/class-export-inherited-direct/main.ts"),
            box_module,
            None,
        ),
        (
            "alias-valid",
            include_str!("fixtures/typescript_oracle/class-export-inherited-alias/main.ts"),
            include_str!("fixtures/typescript_oracle/class-export-inherited-alias/box.ts"),
            None,
        ),
        (
            "constructor-error",
            include_str!(
                "fixtures/typescript_oracle/class-export-inherited-constructor-error/main.ts"
            ),
            box_module,
            Some((6, "TS2345")),
        ),
        (
            "instance-error",
            include_str!(
                "fixtures/typescript_oracle/class-export-inherited-instance-error/main.ts"
            ),
            box_module,
            Some((7, "TS2345")),
        ),
        (
            "static-error",
            include_str!("fixtures/typescript_oracle/class-export-inherited-static-error/main.ts"),
            box_module,
            Some((6, "TS2345")),
        ),
        (
            "result-error",
            include_str!("fixtures/typescript_oracle/class-export-inherited-result-error/main.ts"),
            box_module,
            Some((6, "TS2322")),
        ),
    ];
    assert_pinned_class_module_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_imported_base_derived_class_surfaces_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let base = include_str!("fixtures/typescript_oracle/class-imported-base-derived-valid/base.ts");
    let direct_box =
        include_str!("fixtures/typescript_oracle/class-imported-base-derived-valid/box.ts");
    for (name, main, box_module, expected_error) in [
        (
            "direct-valid",
            include_str!("fixtures/typescript_oracle/class-imported-base-derived-valid/main.ts"),
            direct_box,
            None,
        ),
        (
            "alias-valid",
            include_str!("fixtures/typescript_oracle/class-imported-base-derived-alias/main.ts"),
            include_str!("fixtures/typescript_oracle/class-imported-base-derived-alias/box.ts"),
            None,
        ),
        (
            "constructor-error",
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-derived-constructor-error/main.ts"
            ),
            direct_box,
            Some((6, "TS2345")),
        ),
        (
            "instance-error",
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-derived-instance-error/main.ts"
            ),
            direct_box,
            Some((7, "TS2345")),
        ),
        (
            "static-error",
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-derived-static-error/main.ts"
            ),
            direct_box,
            Some((6, "TS2345")),
        ),
        (
            "result-error",
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-derived-result-error/main.ts"
            ),
            direct_box,
            Some((6, "TS2322")),
        ),
    ] {
        let temporary = TestDirectory::new();
        let input = temporary.path().join("main.ts");
        fs::write(&input, main).unwrap();
        fs::write(temporary.path().join("box.ts"), box_module).unwrap();
        fs::write(temporary.path().join("base.ts"), base).unwrap();
        let output = Command::new(&tsc)
            .args([
                "--target",
                "ES2022",
                "--module",
                "ES2022",
                "--strict",
                "--pretty",
                "false",
                "--allowImportingTsExtensions",
                "--noEmit",
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            expected_error.is_none(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            typescript_diagnostic_lines(&output),
            expected_error.map_or_else(Vec::new, |(line, _)| vec![line]),
            "{name}"
        );
        if let Some((line, code)) = expected_error {
            assert!(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|text| text.contains(&format!("({line},")) && text.contains(code)),
                "{name}: missing {code} at line {line}"
            );
        }
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 3);
    }
    let temporary = TestDirectory::new();
    let input = temporary.path().join("main.ts");
    fs::write(
        &input,
        include_str!("fixtures/typescript_oracle/class-imported-base-derived-chain/main.ts"),
    )
    .unwrap();
    fs::write(
        temporary.path().join("box.ts"),
        include_str!("fixtures/typescript_oracle/class-imported-base-derived-chain/box.ts"),
    )
    .unwrap();
    fs::write(
        temporary.path().join("middle.ts"),
        include_str!("fixtures/typescript_oracle/class-imported-base-derived-chain/middle.ts"),
    )
    .unwrap();
    fs::write(temporary.path().join("base.ts"), base).unwrap();
    let output = Command::new(&tsc)
        .args([
            "--target",
            "ES2022",
            "--module",
            "ES2022",
            "--strict",
            "--pretty",
            "false",
            "--allowImportingTsExtensions",
            "--noEmit",
        ])
        .arg(&input)
        .output()
        .unwrap();
    assert_success(
        &output,
        "four-module inherited class chain should type-check",
    );
    assert!(typescript_diagnostic_lines(&output).is_empty());
    assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 4);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_imported_base_type_reexport_chain_matches_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    for (name, main, expected_error) in [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-imported-base-type-reexport/valid.ts"),
            None,
        ),
        (
            "argument-error",
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-type-reexport/argument-error.ts"
            ),
            Some((7, "TS2345")),
        ),
        (
            "result-error",
            include_str!(
                "fixtures/typescript_oracle/class-imported-base-type-reexport/result-error.ts"
            ),
            Some((7, "TS2322")),
        ),
    ] {
        let temporary = TestDirectory::new();
        let input = temporary.path().join("main.ts");
        fs::write(&input, main).unwrap();
        for (filename, source) in [
            (
                "second.ts",
                include_str!(
                    "fixtures/typescript_oracle/class-imported-base-type-reexport/second.ts"
                ),
            ),
            (
                "first.ts",
                include_str!(
                    "fixtures/typescript_oracle/class-imported-base-type-reexport/first.ts"
                ),
            ),
            (
                "box.ts",
                include_str!("fixtures/typescript_oracle/class-imported-base-type-reexport/box.ts"),
            ),
            (
                "base.ts",
                include_str!(
                    "fixtures/typescript_oracle/class-imported-base-type-reexport/base.ts"
                ),
            ),
        ] {
            fs::write(temporary.path().join(filename), source).unwrap();
        }
        let output = Command::new(&tsc)
            .args([
                "--target",
                "ES2022",
                "--module",
                "ES2022",
                "--strict",
                "--pretty",
                "false",
                "--allowImportingTsExtensions",
                "--noEmit",
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            expected_error.is_none(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            typescript_diagnostic_lines(&output),
            expected_error.map_or_else(Vec::new, |(line, _)| vec![line]),
            "{name}"
        );
        if let Some((line, code)) = expected_error {
            assert!(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|text| text.contains(&format!("({line},")) && text.contains(code)),
                "{name}: missing {code} at line {line}"
            );
        }
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 5);
    }
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_local_method_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 5] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-local-valid/main.ts"),
            &[],
        ),
        (
            "instance-parameter-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-local-instance-parameter-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "instance-result-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-local-instance-result-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "static-parameter-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-local-static-parameter-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
        (
            "static-result-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-local-static-result-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_ancestor_method_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 6] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-ancestor-valid/main.ts"),
            &[],
        ),
        (
            "nearest-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-nearest-error/main.ts"
            ),
            &[(8, "TS2416")],
        ),
        (
            "instance-parameter-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-instance-parameter-error/main.ts"
            ),
            &[(8, "TS2416")],
        ),
        (
            "instance-result-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-instance-result-error/main.ts"
            ),
            &[(8, "TS2416")],
        ),
        (
            "static-parameter-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-static-parameter-error/main.ts"
            ),
            &[(7, "TS2417")],
        ),
        (
            "static-result-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-ancestor-static-result-error/main.ts"
            ),
            &[(7, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_imported_method_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    for (name, main, expected_error) in [
        (
            "direct-valid",
            include_str!("fixtures/typescript_oracle/class-override-imported/direct-valid.ts"),
            None,
        ),
        (
            "transitive-valid",
            include_str!("fixtures/typescript_oracle/class-override-imported/transitive-valid.ts"),
            None,
        ),
        (
            "alias-static-error",
            include_str!("fixtures/typescript_oracle/class-override-imported/alias-static-error.ts"),
            Some((6, "TS2417")),
        ),
        (
            "required-extra-error",
            include_str!("fixtures/typescript_oracle/class-override-imported/required-extra-error.ts"),
            Some((7, "TS2416")),
        ),
        (
            "direct-instance-error",
            include_str!("fixtures/typescript_oracle/class-override-imported/direct-instance-error.ts"),
            Some((7, "TS2416")),
        ),
        (
            "direct-static-error",
            include_str!("fixtures/typescript_oracle/class-override-imported/direct-static-error.ts"),
            Some((6, "TS2417")),
        ),
        (
            "transitive-instance-error",
            include_str!("fixtures/typescript_oracle/class-override-imported/transitive-instance-error.ts"),
            Some((7, "TS2416")),
        ),
        (
            "transitive-static-error",
            include_str!("fixtures/typescript_oracle/class-override-imported/transitive-static-error.ts"),
            Some((6, "TS2417")),
        ),
        (
            "transitive-instance-result-error",
            include_str!("fixtures/typescript_oracle/class-override-imported/transitive-instance-result-error.ts"),
            Some((7, "TS2416")),
        ),
        (
            "direct-static-result-error",
            include_str!("fixtures/typescript_oracle/class-override-imported/direct-static-result-error.ts"),
            Some((6, "TS2417")),
        ),
    ] {
        let temporary = TestDirectory::new();
        let input = temporary.path().join("main.ts");
        fs::write(&input, main).unwrap();
        fs::write(temporary.path().join("base.ts"), include_str!("fixtures/typescript_oracle/class-override-imported/base.ts")).unwrap();
        fs::write(temporary.path().join("middle.ts"), include_str!("fixtures/typescript_oracle/class-override-imported/middle.ts")).unwrap();
        let output = Command::new(&tsc)
            .args([
                "--target", "ES2022", "--module", "ES2022", "--strict", "--pretty", "false",
                "--allowImportingTsExtensions", "--noEmit",
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            expected_error.is_none(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            typescript_diagnostic_lines(&output),
            expected_error.map_or_else(Vec::new, |(line, _)| vec![line]),
            "{name}"
        );
        if let Some((line, code)) = expected_error {
            assert!(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|text| text.contains(&format!("({line},")) && text.contains(code)),
                "{name}: missing {code} at line {line}"
            );
        }
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 3);
    }
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_method_override_arities_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 7] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-arity-valid/main.ts"),
            &[],
        ),
        (
            "instance-optional-required",
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-instance-optional-required/main.ts"
            ),
            &[],
        ),
        (
            "static-optional-required",
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-static-optional-required/main.ts"
            ),
            &[],
        ),
        (
            "instance-extra-required",
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-instance-extra-required/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "instance-optional-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-instance-optional-type-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "static-extra-required",
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-static-extra-required/main.ts"
            ),
            &[(6, "TS2417")],
        ),
        (
            "static-optional-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-arity-static-optional-type-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_array_rest_method_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 6] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-array-rest-valid/main.ts"),
            &[],
        ),
        (
            "instance-element-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-instance-element-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "instance-result-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-instance-result-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "static-element-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-static-element-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
        (
            "static-result-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-array-rest-static-result-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
        (
            "prefix-error",
            include_str!("fixtures/typescript_oracle/class-override-array-rest-prefix-error/main.ts"),
            &[(7, "TS2416")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_derived_array_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 4] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-derived-rest-valid/main.ts"),
            &[],
        ),
        (
            "instance-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-rest-instance-type-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "static-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-rest-static-type-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
        (
            "later-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-rest-later-type-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_fixed_derived_base_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 6] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-base-rest-valid/main.ts"),
            &[],
        ),
        (
            "required-at-rest",
            include_str!("fixtures/typescript_oracle/class-override-base-rest-required/main.ts"),
            &[],
        ),
        (
            "extra-required",
            include_str!(
                "fixtures/typescript_oracle/class-override-base-rest-extra-required/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "instance-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-base-rest-instance-type-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "later-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-base-rest-later-type-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "static-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-base-rest-static-type-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_shifted_array_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 5] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-shifted-rest-valid/main.ts"),
            &[],
        ),
        (
            "prefix-error",
            include_str!("fixtures/typescript_oracle/class-override-shifted-rest-prefix-error/main.ts"),
            &[(7, "TS2416")],
        ),
        (
            "tail-error",
            include_str!("fixtures/typescript_oracle/class-override-shifted-rest-tail-error/main.ts"),
            &[(7, "TS2416")],
        ),
        (
            "static-prefix-error",
            include_str!("fixtures/typescript_oracle/class-override-shifted-rest-static-prefix-error/main.ts"),
            &[(6, "TS2417")],
        ),
        (
            "extra-required",
            include_str!("fixtures/typescript_oracle/class-override-shifted-rest-extra-required/main.ts"),
            &[],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_tuple_rest_annotations_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 3] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-tuple-rest-valid/main.ts"),
            &[],
        ),
        (
            "primitive-error",
            include_str!("fixtures/typescript_oracle/class-tuple-rest-primitive-error/main.ts"),
            &[(6, "TS2370")],
        ),
        (
            "unknown-element",
            include_str!("fixtures/typescript_oracle/class-tuple-rest-unknown-element/main.ts"),
            &[(6, "TS2304")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_optional_tuple_elements_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 5] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/tuple-optional-valid/main.ts"),
            &[],
        ),
        (
            "wrong-type",
            include_str!("fixtures/typescript_oracle/tuple-optional-wrong-type/main.ts"),
            &[(5, "TS2322")],
        ),
        (
            "required-after",
            include_str!("fixtures/typescript_oracle/tuple-optional-required-after/main.ts"),
            &[(5, "TS1257")],
        ),
        (
            "index-valid",
            include_str!("fixtures/typescript_oracle/tuple-optional-index-valid/main.ts"),
            &[],
        ),
        (
            "index-error",
            include_str!("fixtures/typescript_oracle/tuple-optional-index-error/main.ts"),
            &[(6, "TS2322")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_labeled_tuple_elements_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 3] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/tuple-labeled-valid/main.ts"),
            &[],
        ),
        (
            "mixed-valid",
            include_str!("fixtures/typescript_oracle/tuple-labeled-mixed-valid/main.ts"),
            &[],
        ),
        (
            "type-error",
            include_str!("fixtures/typescript_oracle/tuple-labeled-type-error/main.ts"),
            &[(5, "TS2322")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_trailing_tuple_rest_elements_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 5] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/tuple-rest-trailing-valid/main.ts"),
            &[],
        ),
        (
            "type-error",
            include_str!("fixtures/typescript_oracle/tuple-rest-trailing-type-error/main.ts"),
            &[(5, "TS2322")],
        ),
        (
            "arity-error",
            include_str!("fixtures/typescript_oracle/tuple-rest-trailing-arity-error/main.ts"),
            &[(5, "TS2322")],
        ),
        (
            "index-error",
            include_str!("fixtures/typescript_oracle/tuple-rest-trailing-index-error/main.ts"),
            &[(6, "TS2322")],
        ),
        (
            "optional-after-error",
            include_str!(
                "fixtures/typescript_oracle/tuple-rest-trailing-optional-after-error/main.ts"
            ),
            &[(5, "TS1266")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_derived_tuple_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 4] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-derived-tuple-valid/main.ts"),
            &[],
        ),
        (
            "fixed-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tuple-fixed-type-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "fixed-arity-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tuple-fixed-arity-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "array-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tuple-array-type-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_inherited_tuple_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 4] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-base-tuple-valid/main.ts"),
            &[],
        ),
        (
            "fixed-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-base-tuple-fixed-type-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "fixed-arity-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-base-tuple-fixed-arity-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "array-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-base-tuple-array-type-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_both_tuple_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 4] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-both-tuple-valid/main.ts"),
            &[],
        ),
        (
            "type-error",
            include_str!("fixtures/typescript_oracle/class-override-both-tuple-type-error/main.ts"),
            &[(7, "TS2416")],
        ),
        (
            "arity-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-both-tuple-arity-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "static-type-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-both-tuple-static-type-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_optional_tuple_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 4] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-optional-tuple-valid/main.ts"),
            &[],
        ),
        (
            "element-error",
            include_str!("fixtures/typescript_oracle/class-override-optional-tuple-element-error/main.ts"),
            &[(7, "TS2416")],
        ),
        (
            "arity-error",
            include_str!("fixtures/typescript_oracle/class-override-optional-tuple-arity-error/main.ts"),
            &[(7, "TS2416")],
        ),
        (
            "static-element-error",
            include_str!("fixtures/typescript_oracle/class-override-optional-tuple-static-element-error/main.ts"),
            &[(6, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_labeled_tuple_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 4] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-labeled-tuple-valid/main.ts"),
            &[],
        ),
        (
            "instance-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-labeled-tuple-instance-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "static-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-labeled-tuple-static-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
        (
            "arity-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-labeled-tuple-arity-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_derived_trailing_tuple_rest_overrides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 4] = [
        (
            "valid",
            include_str!("fixtures/typescript_oracle/class-override-derived-tail-valid/main.ts"),
            &[],
        ),
        (
            "fixed-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tail-fixed-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "arity-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tail-arity-error/main.ts"
            ),
            &[(7, "TS2416")],
        ),
        (
            "array-error",
            include_str!(
                "fixtures/typescript_oracle/class-override-derived-tail-array-error/main.ts"
            ),
            &[(6, "TS2417")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_local_class_method_sides_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 6] = [
        (
            "instance-method",
            include_str!("fixtures/typescript_oracle/class-local-instance-method/main.ts"),
            &[],
        ),
        (
            "wrong-side-call",
            include_str!("fixtures/typescript_oracle/class-local-instance-wrong-side-call/main.ts"),
            &[(6, "TS2339")],
        ),
        (
            "wrong-side-read",
            include_str!("fixtures/typescript_oracle/class-local-instance-wrong-side-read/main.ts"),
            &[(6, "TS2339")],
        ),
        (
            "called-without-new",
            include_str!("fixtures/typescript_oracle/class-local-called-without-new/main.ts"),
            &[(6, "TS2348")],
        ),
        (
            "nested-call-without-new",
            include_str!("fixtures/typescript_oracle/class-local-nested-call-without-new/main.ts"),
            &[(7, "TS2348")],
        ),
        (
            "shadowed-call",
            include_str!("fixtures/typescript_oracle/class-local-shadowed-call/main.ts"),
            &[],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_static_class_method_shell_matches_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    assert_pinned_no_emit_cases(
        &tsc,
        &[(
            "static-method-shell",
            include_str!("fixtures/typescript_oracle/class-static-method-shell/main.ts"),
            &[],
        )],
    );
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_static_class_binding_matches_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitCase; 6] = [
        (
            "static-binding",
            include_str!("fixtures/typescript_oracle/class-static-binding/main.ts"),
            &[],
        ),
        (
            "static-overload-binding",
            include_str!("fixtures/typescript_oracle/class-static-overload-binding/main.ts"),
            &[],
        ),
        (
            "static-wrong-instance-call",
            include_str!("fixtures/typescript_oracle/class-static-wrong-instance-call/main.ts"),
            &[(7, "TS2576")],
        ),
        (
            "static-wrong-instance-read",
            include_str!("fixtures/typescript_oracle/class-static-wrong-instance-read/main.ts"),
            &[(7, "TS2576")],
        ),
        (
            "static-argument-error",
            include_str!("fixtures/typescript_oracle/class-static-argument-error/main.ts"),
            &[(6, "TS2345")],
        ),
        (
            "static-overload-error",
            include_str!("fixtures/typescript_oracle/class-static-overload-error/main.ts"),
            &[(10, "TS2769")],
        ),
    ];
    assert_pinned_no_emit_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_type_imports_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitClassModuleCase; 6] = [
        (
            "direct",
            include_str!("fixtures/typescript_oracle/class-type-import/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import/box.ts"),
            None,
        ),
        (
            "local-export-alias",
            include_str!("fixtures/typescript_oracle/class-type-import-alias/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-alias/box.ts"),
            None,
        ),
        (
            "type-export-alias",
            include_str!("fixtures/typescript_oracle/class-type-import-type-export/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-type-export/box.ts"),
            None,
        ),
        (
            "self-reference",
            include_str!("fixtures/typescript_oracle/class-type-import-self-reference/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-self-reference/box.ts"),
            None,
        ),
        (
            "wrong-side",
            include_str!("fixtures/typescript_oracle/class-type-import-wrong-side/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-wrong-side/box.ts"),
            Some((7, "TS2576")),
        ),
        (
            "private-class",
            include_str!("fixtures/typescript_oracle/class-type-import-private/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-private/box.ts"),
            Some((5, "TS2459")),
        ),
    ];
    assert_pinned_class_module_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_value_import_bindings_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let cases: [NoEmitClassModuleCase; 3] = [
        (
            "direct-value-import",
            include_str!("fixtures/typescript_oracle/class-value-import-binding/main.ts"),
            include_str!("fixtures/typescript_oracle/class-value-import-binding/box.ts"),
            None,
        ),
        (
            "aliased-value-import",
            include_str!("fixtures/typescript_oracle/class-value-import-alias-binding/main.ts"),
            include_str!("fixtures/typescript_oracle/class-value-import-alias-binding/box.ts"),
            None,
        ),
        (
            "wrong-side-value",
            include_str!(
                "fixtures/typescript_oracle/class-value-import-wrong-side-binding/main.ts"
            ),
            include_str!("fixtures/typescript_oracle/class-value-import-wrong-side-binding/box.ts"),
            Some((6, "TS2741")),
        ),
    ];
    assert_pinned_class_module_cases(&tsc, &cases);
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn pinned_class_value_import_calls_match_typescript_without_emit() {
    let tsc = pinned_bluetsc_oracle();
    assert_pinned_version(&tsc);
    let box_module = include_str!("fixtures/typescript_oracle/class-value-import-calls/box.ts");
    let cases: [NoEmitClassModuleCase; 11] = [
        (
            "direct-calls",
            include_str!("fixtures/typescript_oracle/class-value-import-calls/main.ts"),
            box_module,
            None,
        ),
        (
            "aliased-calls",
            include_str!("fixtures/typescript_oracle/class-value-import-calls-alias/main.ts"),
            include_str!("fixtures/typescript_oracle/class-value-import-calls-alias/box.ts"),
            None,
        ),
        (
            "type-only-safe",
            include_str!("fixtures/typescript_oracle/class-type-import-value-safe/main.ts"),
            box_module,
            None,
        ),
        (
            "constructor-error",
            include_str!("fixtures/typescript_oracle/class-value-import-constructor-error/main.ts"),
            box_module,
            Some((6, "TS2345")),
        ),
        (
            "static-error",
            include_str!("fixtures/typescript_oracle/class-value-import-static-error/main.ts"),
            box_module,
            Some((6, "TS2345")),
        ),
        (
            "wrong-constructor-side",
            include_str!(
                "fixtures/typescript_oracle/class-value-import-wrong-constructor-side/main.ts"
            ),
            box_module,
            Some((6, "TS2339")),
        ),
        (
            "wrong-instance-side",
            include_str!(
                "fixtures/typescript_oracle/class-value-import-wrong-instance-side/main.ts"
            ),
            box_module,
            Some((7, "TS2576")),
        ),
        (
            "bare-call",
            include_str!("fixtures/typescript_oracle/class-value-import-bare-call/main.ts"),
            box_module,
            Some((6, "TS2348")),
        ),
        (
            "type-only-value-use",
            include_str!("fixtures/typescript_oracle/class-type-import-value-use/main.ts"),
            box_module,
            Some((6, "TS1361")),
        ),
        (
            "type-only-bare-value",
            include_str!("fixtures/typescript_oracle/class-type-import-bare-value/main.ts"),
            box_module,
            Some((6, "TS1361")),
        ),
        (
            "type-export-value-use",
            include_str!("fixtures/typescript_oracle/class-type-export-value-use/main.ts"),
            include_str!("fixtures/typescript_oracle/class-type-import-type-export/box.ts"),
            Some((6, "TS1362")),
        ),
    ];
    assert_pinned_class_module_cases(&tsc, &cases);
}

fn assert_pinned_class_module_cases(tsc: &Path, cases: &[NoEmitClassModuleCase]) {
    for &(name, main, box_module, expected_error) in cases {
        let temporary = TestDirectory::new();
        let input = temporary.path().join("main.ts");
        fs::write(&input, main).unwrap();
        fs::write(temporary.path().join("box.ts"), box_module).unwrap();
        let output = Command::new(tsc)
            .args([
                "--target",
                "ES2022",
                "--module",
                "ES2022",
                "--strict",
                "--pretty",
                "false",
                "--allowImportingTsExtensions",
                "--noEmit",
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            expected_error.is_none(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            typescript_diagnostic_lines(&output),
            expected_error.map_or_else(Vec::new, |(line, _)| vec![line]),
            "{name}"
        );
        if let Some((line, code)) = expected_error {
            assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("({line},")));
            assert!(String::from_utf8_lossy(&output.stdout).contains(code));
        }
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 2);
    }
}

fn assert_pinned_no_emit_cases(tsc: &Path, cases: &[NoEmitCase]) {
    for &(name, source, expected_errors) in cases {
        let temporary = TestDirectory::new();
        let input = temporary.path().join("main.ts");
        fs::write(&input, source).unwrap();
        let output = run_tsc(tsc, &input, temporary.path(), true, false);
        assert_eq!(
            output.status.success(),
            expected_errors.is_empty(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let expected_lines = expected_errors
            .iter()
            .map(|(line, _)| *line)
            .collect::<Vec<_>>();
        assert_eq!(
            typescript_diagnostic_lines(&output),
            expected_lines,
            "{name}"
        );
        for (line, code) in expected_errors {
            assert!(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|text| text.contains(&format!("({line},")) && text.contains(code)),
                "{name}: missing {code} at line {line}"
            );
        }
        assert_eq!(
            fs::read_dir(temporary.path()).unwrap().count(),
            1,
            "{name} emitted an artifact"
        );
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
            let (prefix, _) = line.split_once("): error TS")?;
            let (_, location) = prefix.rsplit_once('(')?;
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
