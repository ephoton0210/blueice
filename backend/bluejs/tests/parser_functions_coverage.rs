// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::parse;

#[test]
fn malformed_function_names_class_heritage_and_accessors_are_syntax_errors() {
    let private_marker = parse("function #").expect_err("a private marker cannot name a function");
    assert!(private_marker.known_syntax);
    assert!(
        private_marker
            .message
            .contains("a function cannot have a private name"),
        "{private_marker:?}"
    );
    for source in [
        "function #private() {}",
        "(async function a\\u0077ait() {})",
        "class Child extends parent => {}",
        "class Child extends (parent {}",
        "class Example { accessor value() {} }",
    ] {
        let error = parse(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
    }
}

#[test]
fn sloppy_decorator_let_and_async_arrow_await_follow_their_grammar_contexts() {
    parse("var let = () => {}; @let class Example {}")
        .expect("sloppy let is a valid decorator reference");
    for source in ["async await => 1", "async a\\u0077ait => 1"] {
        let error = parse(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
    }
}

#[test]
fn malformed_decorator_operands_and_arrow_bodies_fail_at_their_grammar_boundary() {
    for source in [
        "function missing_parameters {}",
        "function missing_rest_name(...) {}",
        "function missing_close(...rest",
        "({ get value: 1 })",
        "@() class Example {}",
        "@decorator.() class Example {}",
        "@decorator( class Example {}",
        "@decorator class Example {",
        "a\\u0073ync value => value",
        "value =>",
    ] {
        assert!(parse(source).is_err(), "{source} should fail to parse");
    }
}
