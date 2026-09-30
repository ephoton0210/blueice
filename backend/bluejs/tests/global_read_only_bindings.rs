// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assigning to a global `var` that is backed by a non-writable global
//! property (`NaN`, `Infinity`, `undefined`): SetMutableBinding of the global
//! Environment Record silently ignores the write in sloppy code and throws a
//! TypeError in strict code (§9.1.1.2.5).

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn run_script(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute_script(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn sloppy_assignment_to_a_read_only_global_var_is_ignored() {
    assert_eq!(
        run_script("var NaN = 1.0; NaN = 'asdf'; NaN = true; NaN !== NaN"),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        run_script("var Infinity = 1; Infinity = 'x'; Infinity === 1 / 0"),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        run_script("var undefined = 1; undefined = 2; undefined === void 0"),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        run_script("NaN = 5; typeof NaN === 'number' && NaN !== NaN"),
        Ok(Value::Bool(true))
    );
}

#[test]
fn strict_assignment_to_a_read_only_global_throws_a_type_error() {
    assert!(matches!(
        run_script("'use strict'; NaN = 1;"),
        Err(RuntimeError::TypeError(_))
    ));
    assert!(matches!(
        run_script("'use strict'; undefined = 1;"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn a_writable_global_var_is_unaffected() {
    assert_eq!(
        run_script("var g = 1; g = 2; this.g === 2 && g === 2"),
        Ok(Value::Bool(true))
    );
}
