// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The decorators grammar (`@` before classes and class elements): what is
//! accepted, what shape the parser gives it, and what is a SyntaxError.

use blueice_bluejs::{parse, ClassElement, Expr, Stmt};

fn class_of(source: &str) -> blueice_bluejs::Class {
    let program = parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    match program.body.into_iter().next() {
        Some(Stmt::ClassDecl(class)) => class,
        Some(Stmt::Expr(Expr::Class(class))) => class,
        other => panic!("{source}: expected a class, found {other:?}"),
    }
}

fn accepts(source: &str) {
    parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
}

fn rejects(source: &str) {
    assert!(parse(source).is_err(), "{source} should be a SyntaxError");
}

#[test]
fn a_class_declaration_keeps_its_decorators_in_source_order() {
    let class = class_of("@a @b.c @d(1, 2) @(e + f) class C {}");
    assert_eq!(class.decorators.len(), 4);
    assert_eq!(class.decorators[0], Expr::Identifier("a".into()));
    assert!(matches!(&class.decorators[1], Expr::Member { .. }));
    assert!(matches!(&class.decorators[2], Expr::Call { args, .. } if args.len() == 2));
    assert!(matches!(&class.decorators[3], Expr::Parenthesized(_)));
}

#[test]
fn a_class_expression_can_be_decorated() {
    accepts("var C = @dec class {};");
    accepts("var C = @dec class Named {};");
    accepts("(@dec class {});");
    accepts("var f = () => @dec class {};");
    accepts("var C = @dec1 @dec2 class extends Base {};");
}

#[test]
fn class_elements_keep_their_decorators() {
    let class = class_of(
        "class C { @a m() {} @b static get g() {} @c set s(v) {} @d f = 1; @e static #p; \
         @f accessor x; @g static accessor #y = 1; @h @i [k]() {} }",
    );
    let counts: Vec<usize> = class
        .elements
        .iter()
        .map(|element| match element {
            ClassElement::Method { decorators, .. }
            | ClassElement::Accessor { decorators, .. }
            | ClassElement::Field { decorators, .. } => decorators.len(),
            ClassElement::StaticBlock(_) => 0,
        })
        .collect();
    assert_eq!(counts, [1, 1, 1, 1, 1, 1, 1, 2]);
}

#[test]
fn decorators_may_share_a_line_with_or_precede_the_element_on_the_next() {
    accepts("class C { @a @b m() {} }");
    accepts("class C {\n  @a\n  @b\n  m() {}\n}");
    accepts("class C { @a\n static\n m() {} }");
}

#[test]
fn a_decorator_expression_is_a_member_chain_a_call_or_a_parenthesized_expression() {
    accepts("class C { @a.b.c.d m() {} }");
    // Only one argument list: a second `(...)` cannot continue the decorator.
    rejects("class C { @a.b.c(1)(2) m() {} }");
}

#[test]
fn a_decorator_cannot_be_an_arbitrary_expression() {
    // `@a[b]` is not a member decorator: `[b]` is a computed method key.
    let class = class_of("class C { @a [b]() {} }");
    assert!(matches!(
        &class.elements[0],
        ClassElement::Method { decorators, .. } if decorators.len() == 1
    ));
    rejects("class C { @a?.b m() {} }");
    rejects("@a`template` class C {}");
    rejects("@a + b class C {}");
    rejects("@new A class C {}");
    rejects("@this.a class C {}");
    rejects("@1 class C {}");
    rejects("@ class C {}");
    rejects("class C { @ m() {} }");
}

#[test]
fn a_private_name_is_allowed_after_a_dot_in_a_decorator() {
    accepts("class C { static #d() {} static { @C.#d class D {} } }");
    accepts("class C { static #d() {} @C.#d m() {} }");
}

#[test]
fn decorators_must_precede_something_decoratable() {
    rejects("@dec");
    rejects("@dec;");
    rejects("@dec function f() {}");
    rejects("@dec var x;");
    rejects("@dec {}");
    rejects("class C { @dec }");
    rejects("class C { @dec; m() {} }");
    rejects("class C { @dec static {} }");
    rejects("class C { @dec constructor() {} }");
    rejects("class C { @dec 'constructor'() {} }");
    // A decorated anonymous class is not a declaration.
    rejects("@dec class {}");
}

#[test]
fn the_constructor_can_be_decorated_only_through_the_class() {
    accepts("@dec class C { constructor() {} }");
    // A computed key named constructor is an ordinary method.
    accepts("class C { @dec ['constructor']() {} }");
}

#[test]
fn accessor_and_other_contextual_words_are_valid_decorator_names_and_element_names() {
    accepts("class C { @accessor @get @set @async @await m() {} }");
    accepts("var yield = () => {}; @yield class C {}");
    accepts("class C { @a accessor accessor; @a static accessor static; }");
}

#[test]
fn yield_and_await_follow_the_surrounding_context_in_a_decorator() {
    // `yield` is an ordinary identifier in sloppy code but an operator's
    // keyword in a generator, where `@yield` is not an IdentifierReference.
    accepts("var yield = () => {}; @yield() class C {}");
    rejects("function* g() { @yield class C {} }");
    accepts("function* g() { @(yield) class C {} }");
    accepts("async function f() { @(await 1) class C {} }");
}

#[test]
fn a_class_decorator_list_is_outside_the_strict_class_body() {
    // The decorator list is outside the class body, so it is sloppy code in a
    // sloppy script (`yield` and `let` are identifiers there).
    accepts("@yield class C {}");
    rejects("'use strict'; @yield class C {}");
}

#[test]
fn a_decorated_class_declaration_needs_a_class_after_the_decorators() {
    rejects("@dec export");
    rejects("@dec let x");
}
