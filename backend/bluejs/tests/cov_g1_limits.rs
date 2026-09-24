// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Resource limits reached from built-ins that loop or build strings: the
//! instruction budget (charged by native loops as well as bytecode) and the
//! string byte limit.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn run(config: VmConfig, source: &str) -> Result<Value, RuntimeError> {
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
}

/// Runs `source` under every instruction budget from 1 upward until it
/// completes; every shorter budget must stop with the instruction limit, from
/// wherever it happens to run out (native loops included).
fn every_budget_completes_or_hits_the_limit(source: &str) -> Value {
    let mut budget = 1;
    loop {
        let config = VmConfig {
            instruction_budget: budget,
            ..VmConfig::default()
        };
        match run(config, source) {
            Ok(value) => {
                assert!(budget > 3, "{source}");
                return value;
            }
            Err(error) => assert_eq!(error, RuntimeError::InstructionLimit, "{source}"),
        }
        budget += 1;
    }
}

#[test]
fn built_in_loops_stop_at_every_instruction_budget() {
    for (source, expected) in [
        ("Array.from({ length: 3, 0: 1, 1: 2, 2: 3 }).length", 3.0),
        ("Array.from([1, 2, 3]).length", 3.0),
        (
            "Object.keys(Object.groupBy([1, 2, 3], x => x % 2)).length",
            2.0,
        ),
        (
            "function F() {} var o = Object.create(Object.create(Object.create(F.prototype))); o instanceof F ? 1 : 0",
            1.0,
        ),
        (
            "Intl.getCanonicalLocales({ length: 3, 0: 'en', 1: 'de', 2: 'fr' }).length",
            3.0,
        ),
        (
            "Intl.getCanonicalLocales(['en', 'de', 'fr']).length",
            3.0,
        ),
    ] {
        assert_eq!(
            every_budget_completes_or_hits_the_limit(source),
            Value::Number(expected),
            "{source}"
        );
    }
}

#[test]
fn a_bound_function_name_is_bounded_by_the_string_limit() {
    let tiny = || VmConfig {
        max_string_bytes: 8,
        ..VmConfig::default()
    };
    // A string name is appended to "bound ", which is already too long.
    assert_eq!(
        run(tiny(), "(function () {}).bind()"),
        Err(RuntimeError::StringLimit { limit: 8 })
    );
    // A name that is not a string leaves "bound " alone, still too long.
    assert_eq!(
        run(
            tiny(),
            "(function () { class C { static name() {} } return C.bind() })()"
        ),
        Err(RuntimeError::StringLimit { limit: 8 })
    );
}
