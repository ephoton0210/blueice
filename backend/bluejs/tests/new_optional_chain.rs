// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `new` takes a MemberExpression: an optional chain is never its target.
use blueice_bluejs::parse;

#[test]
fn new_cannot_take_an_optional_chain_as_its_target() {
    for source in [
        "const o = { C: class {} }; new o?.C();",
        "const o = { C: class {} }; new o?.['C']();",
        "class C {} new C?.();",
        "new C?.D",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    // A parenthesized chain is an ordinary operand.
    assert!(parse("const o = { C: class {} }; new (o?.C)();").is_ok());
}
