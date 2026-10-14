// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Statement blocks and object methods have different parameter grammar.

use blueice_bluets::{parse_module, Declaration};

#[test]
fn iterator_control_conditions_are_not_parsed_as_method_parameters() {
    for source in [
        include_str!("fixtures/typescript_oracle/emittarget-es2022-iteration-close/main.ts"),
        include_str!("fixtures/typescript_oracle/emittarget-es2022-iterator-abrupt-close/main.ts"),
    ] {
        parse_module("memory:///main.ts", source).unwrap_or_else(|errors| panic!("{errors:#?}"));
    }
}

#[test]
fn declarations_after_braced_loops_remain_module_declarations() {
    let module = parse_module(
        "memory:///main.ts",
        include_str!("fixtures/target_protocols/input.ts"),
    )
    .unwrap();
    for name in [
        "beforeStartReturning",
        "beforeStartThrowing",
        "reentrantNext",
        "completedControls",
    ] {
        assert!(
            module.declarations.iter().any(|declaration| {
                matches!(declaration, Declaration::Function(function) if function.name == name)
            }),
            "missing module function {name}"
        );
    }
}

#[test]
fn control_keywords_remain_valid_object_method_names() {
    for name in ["if", "for", "while", "switch", "catch", "return"] {
        let source = format!("const receiver = {{ {name}(value: number): number {{ return value; }} }}; receiver.{name}(42);");
        parse_module("memory:///main.ts", source).unwrap();
    }
}
