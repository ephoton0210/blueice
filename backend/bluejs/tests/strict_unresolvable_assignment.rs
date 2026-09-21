// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! §13.15.2: the left-hand side of `=` is evaluated to a Reference before the
//! right-hand side runs, so a strict assignment to a name that was
//! unresolvable at that moment is a ReferenceError even when the right-hand
//! side goes on to create the global property.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn run(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute_script(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn the_reference_is_resolved_before_the_right_hand_side_creates_the_binding() {
    let result = run("'use strict'; undeclared = (this.undeclared = 5);");
    assert!(
        matches!(result, Err(RuntimeError::ReferenceError(_))),
        "{result:?}"
    );
    // ... and the RHS still ran.
    assert_eq!(
        run("'use strict'; try { undeclared = (this.undeclared = 5); } catch (e) {} this.undeclared"),
        Ok(Value::Number(5.0))
    );
}

#[test]
fn a_name_that_resolves_before_the_right_hand_side_is_assigned_normally() {
    assert_eq!(
        run("'use strict'; this.declared = 1; declared = (this.other = 7, 2); this.declared + this.other"),
        Ok(Value::Number(9.0)),
    );
    // A binding that resolved but is gone when the value is stored: the
    // object Environment Record's SetMutableBinding (strict) throws.
    let result = run("'use strict'; this.declared = 1; declared = (delete this.declared, 2);");
    assert!(
        matches!(result, Err(RuntimeError::ReferenceError(_))),
        "{result:?}"
    );
    assert_eq!(
        run("'use strict'; var v = 1; v = (v = 3, 4); v"),
        Ok(Value::Number(4.0))
    );
    assert!(
        run("'use strict'; NaN = 1;").is_err(),
        "a read-only global still rejects the write"
    );
}

#[test]
fn a_sloppy_assignment_to_an_unresolvable_name_creates_the_global() {
    assert_eq!(
        run("created = (this.created = 5, 6); created + this.created"),
        Ok(Value::Number(12.0))
    );
}
