// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `get`, `set`, `static` and `async` introduce a class element only when
//! spelled without a Unicode escape; an escaped spelling is an ordinary
//! property name, so `get foo() {}` is not a getter.
use blueice_bluejs::{compile, parse};

fn is_rejected(source: &str) -> bool {
    match parse(source) {
        Err(error) => {
            assert!(error.known_syntax, "{source:?}: {error:?}");
            true
        }
        Ok(program) => compile(&program).is_err(),
    }
}

#[test]
fn an_escaped_accessor_keyword_is_not_an_accessor_in_a_class() {
    for source in [
        "(class { \\u0067et foo() { return 0; } });",
        "(class { g\\u0065t foo() { return 0; } });",
        "(class { g\\u0065t ['hi']() { return 0; } });",
        "(class { g\\u0065t 'hi'() { return 0; } });",
        "(class { g\\u0065t 42() { return 0; } });",
        "(class { s\\u0065t foo(v) {} });",
        "(class { st\\u0061tic get foo() { return 0; } });",
        "(class { st\\u0061tic m() {} });",
        "(class { \\u0061sync m() {} });",
    ] {
        assert!(is_rejected(source), "{source}");
    }
}

#[test]
fn an_escaped_accessor_word_is_still_a_valid_member_name() {
    for source in [
        "(class { g\\u0065t() { return 1; } });",
        "(class { g\\u0065t = 1; });",
        "(class { g\\u0065t\n foo() {} });",
    ] {
        assert!(!is_rejected(source), "{source}");
    }
}
