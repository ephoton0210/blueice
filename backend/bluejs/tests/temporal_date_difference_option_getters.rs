// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, Value, Vm};

#[test]
fn date_difference_propagates_each_option_getter_error() {
    let source = r#"
        const date = new Temporal.PlainDate(2020, 5, 23);
        for (const name of ["largestUnit", "roundingIncrement", "roundingMode", "smallestUnit"]) {
            const expected = {};
            const options = {};
            Object.defineProperty(options, name, { get() { throw expected; } });
            let caught = false;
            try {
                date.until(date, options);
            } catch (error) {
                caught = error === expected;
            }
            if (!caught) throw new Error(name);
        }
        true;
    "#;
    let program = compile(&parse(source).unwrap()).unwrap();
    assert_eq!(Vm::default().execute(&program).unwrap(), Value::Bool(true));
}
