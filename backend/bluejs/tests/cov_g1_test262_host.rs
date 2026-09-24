// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The Test262 host adapters (`$262`, `setTimeout`, `buildString`, the
//! property-escape and exhaustive-range batch functions, `assert.deepEqual`
//! and the NumberFormat precision matrix) with their abrupt and failing
//! paths: each script's outcome, either its `String` conversion, the thrown
//! error, or the Test262 failure the host reported.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn observe(expr: &str) -> String {
    let source = format!(
        "(function(){{ try {{ return String({expr}) }} catch (e) {{ return 'throws ' + e.constructor.name + ': ' + e.message }} }})()"
    );
    let mut vm = Vm::new(VmConfig::default()).unwrap();
    vm.install_test262_harness().unwrap();
    vm.install_test262_is_html_dda().unwrap();
    match vm.execute(&compile(&parse(&source).unwrap()).unwrap()) {
        Ok(Value::String(text)) => {
            let text = text.to_utf8().unwrap();
            // Object handles carry a heap serial number that varies per run.
            text.split(" ObjectId {").next().unwrap().to_string()
        }
        other => format!("{other:?}"),
    }
}

include!("cov_g1_tables/test262_host.in");

#[test]
fn test262_host_adapters_report_the_reference_outcomes() {
    let mut mismatches = Vec::new();
    for (expr, expected) in CASES {
        let actual = observe(expr);
        if actual != *expected {
            mismatches.push(format!(
                "{expr}\n    expected {expected}\n    actual   {actual}"
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

fn run_with(config: VmConfig, source: &str) -> Result<Value, blueice_bluejs::RuntimeError> {
    let mut vm = Vm::new(config).unwrap();
    vm.install_test262_harness().unwrap();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn installing_the_host_again_keeps_the_existing_objects() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.install_test262_harness().unwrap();
    assert_eq!(
        vm.execute(&compile(&parse("typeof $262.AbstractModuleSource").unwrap()).unwrap()),
        Ok(Value::String("function".into()))
    );
}

/// Runs `source` under every instruction budget from 1 upward until it
/// completes; every shorter budget must stop with the instruction limit (from
/// wherever it happens to run out, native adapter loops included).
fn every_budget_completes_or_hits_the_limit(source: &str) {
    let mut budget = 1;
    loop {
        let config = VmConfig {
            instruction_budget: budget,
            ..VmConfig::default()
        };
        match run_with(config, source) {
            Ok(_) => break,
            Err(error) => assert_eq!(error, blueice_bluejs::RuntimeError::InstructionLimit),
        }
        budget += 1;
    }
    assert!(budget > 3, "{source}");
}

#[test]
fn the_batch_adapters_stop_cleanly_at_every_instruction_budget() {
    for source in [
        "assert.deepEqual([{ a: [1, 2] }, 3, 4], [{ a: [1, 2] }, 3, 4])",
        "__bluejsTest262NumberFormatPrecisionMatrix(['en', 'de'], ['latn', 'thai'], {}, { '1': '1', '1.500': '1.5', '1.625': '1.625', '1.750': '1.75', '1.875': '1.875', '2.000': '2' })",
        "__bluejsTest262EncodeUriExhaustive(encodeURIComponent, 0x800, 0x803)",
        "buildString({ loneCodePoints: [0x41, 0x42], ranges: [[0x30, 0x39]] })",
        "testPropertyOfStrings({ regExp: /^(a|b)$/, matchStrings: ['a', 'b'], nonMatchStrings: ['c', 'd'] })",
        "__bluejsTest262RegExpClassEscape([/a/, /a/], 'a', true)",
        "__bluejsTest262TypedArrayOverlappingSet(new Uint8Array(4), new Uint8Array(2))",
    ] {
        every_budget_completes_or_hits_the_limit(source);
    }
}

#[test]
fn joined_match_strings_are_bounded_by_the_string_limit() {
    let config = VmConfig {
        max_string_bytes: 30,
        ..VmConfig::default()
    };
    // Each string is 20 bytes; their join is not.
    assert_eq!(
        run_with(
            config,
            "testPropertyOfStrings({ regExp: /a/, matchStrings: ['aaaaaaaaaa', 'bbbbbbbbbb'] })"
        ),
        Err(blueice_bluejs::RuntimeError::StringLimit { limit: 30 })
    );
}
