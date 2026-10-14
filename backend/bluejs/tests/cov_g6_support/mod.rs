// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `cov_g6_*` test targets.
#![allow(dead_code)]

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

pub fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

/// Evaluates `expression` in a fresh VM (with the Test262 harness installed
/// when `harness`) and renders what it produced: `ok:<String(value)>`, or
/// `throw:<name>:<message>` for anything thrown (a thrown primitive has no
/// name, so it renders as `throw:undefined:<String(value)>`).
pub fn outcome_with(harness: bool, expression: &str) -> String {
    let mut vm = Vm::default();
    if harness {
        vm.install_test262_harness().unwrap();
    }
    let source = format!(
        "(function () {{ try {{ var r = ({expression}); return 'ok:' + String(r); }}
           catch (e) {{ return 'throw:' + (e && e.name) + ':' + (e && e.message !== undefined ? e.message : String(e)); }} }})()"
    );
    match run(&mut vm, &source) {
        Ok(Value::String(text)) => text.to_utf8().unwrap_or_else(|_| "<lone surrogate>".into()),
        other => panic!("{expression}: {other:?}"),
    }
}

pub fn outcome(expression: &str) -> String {
    outcome_with(false, expression)
}

/// Asserts the rendered outcome of every `(expression, expected)` pair.
pub fn check(cases: &[(&str, &str)]) {
    for (expression, expected) in cases {
        assert_eq!(&outcome(expression), expected, "{expression}");
    }
}
