// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array.prototype.sort writes the sorted values back with Set(..., true):
//! a failed write throws even in sloppy code and even when the order did not
//! change.
use blueice_bluejs::{compile, parse, Value, Vm};

fn check(sources: &[&str]) {
    for source in sources {
        let value = Vm::default()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(value, Value::Bool(true), "{source}");
    }
}

#[test]
fn sorting_a_frozen_array_throws_a_type_error_in_sloppy_code() {
    check(&[
        "var a = Object.freeze([4, 5, 1]); \
         try { a.sort(function () {}); false } catch (e) { e instanceof TypeError }",
        "var a = Object.freeze([1, 2, 3]); \
         try { a.sort(); false } catch (e) { e instanceof TypeError }",
        "var a = Object.freeze([1, undefined, 3]); \
         try { a.sort(); false } catch (e) { e instanceof TypeError }",
        "var a = Object.freeze([]); a.sort() === a",
        "var a = Object.freeze([2, 1]); a.length === 2 && a[0] === 2 \
           && (function () { try { a.sort(); } catch (e) {} return a[0] === 2 && a[1] === 1; })()",
    ]);
}

#[test]
fn sorting_still_rewrites_holes_and_undefined_in_order() {
    check(&[
        "var a = [3, , 1, undefined, 2]; a.sort(); a.length === 5 && a[0] === 1 && a[1] === 2 \
           && a[2] === 3 && a[3] === undefined && !(4 in a)",
    ]);
}
