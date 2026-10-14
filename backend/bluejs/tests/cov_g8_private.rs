// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private names that no enclosing class declares, in every syntactic position
//! the early-error walk descends into: each is a syntax error, and each of the
//! walk's own error paths is reached through a different position.

use blueice_bluejs::parse;

/// The undeclared private reference each template puts at its `@`.
const REFERENCE: &str = "this.#x";

/// Statements, run inside a method body of a class that declares no `#x`.
const STATEMENTS: &[&str] = &[
    "@;",
    "if (@) ;",
    "if (1) @;",
    "if (0) ; else @;",
    "for (@;;) break;",
    "for (;@;) break;",
    "for (;;@) break;",
    "for (;;) { @; break; }",
    "for (var a = @;;) break;",
    "for (var [a = @] = [];;) break;",
    "for (var a in @) ;",
    "for (var a of @) ;",
    "for (var a in {}) @;",
    "for (var [a = @] in {}) ;",
    "for (var { a = @ } of []) ;",
    "for (var a = @ in {}) ;",
    "for (@ in {}) ;",
    "for ({ a: @ } of []) ;",
    "for ([@] of []) ;",
    "for ({ [@]: a } of []) ;",
    "for ([...@] of []) ;",
    "for ([a = @] of []) ;",
    "for ({ a = @ } of []) ;",
    "while (@) ;",
    "while (0) @;",
    "do ; while (@);",
    "do @; while (0);",
    "switch (@) { }",
    "switch (1) { case @: }",
    "switch (1) { case 1: @; }",
    "a: @;",
    "throw @;",
    "try { @ } catch (e) { }",
    "try { } catch ([e = @]) { }",
    "try { } catch (e) { @ }",
    "try { } finally { @ }",
    "with (@) ;",
    "with ({}) @;",
    "var a = @;",
    "var [a = @] = [];",
    "var { a = @ } = {};",
    "var { [@]: a } = {};",
    "let [[a] = @] = [[1]];",
    "var [[a = @]] = [[]];",
    "var [{ a = @ }] = [{}];",
    "var { k: { a = @ } } = {};",
    "var { k: [a = @] } = {};",
    "var { ...{ a = @ } } = {};",
    "({ ...{ a = @ } } = {});",
    "class D { @(this.#y) x = 1; #z; }",
    "class D { @(@) x = 1; }",
    "class D { @dec(@) x = 1; }",
    "class D { @dec x = @; }",
    "{ @ }",
    "function f() { @ }",
    "function f(a = @) { }",
    "function f([a = @]) { }",
    "function f({ [@]: a }) { }",
    "function f(...[a = @]) { }",
    "function* g() { yield @; }",
    "async function f() { await @; }",
    "async function f() { for await (var a of @) ; }",
    "using a = @;",
    "class D { [@]() { } }",
    "class D { [@] = 1; }",
    "class D { x = @; }",
    "class D { static { @; } }",
    "class D extends @ { }",
    "class D { @(@) m() { } }",
    "class D { @(@) x = 1; }",
    "class D { @(@) accessor y; }",
    "class D { @(@) static m() { } }",
    "class D { m(a = @) { } }",
    "class D { get [@]() { return 1; } }",
    "class D { static [@] = 1; }",
    "@(@) class D { }",
    "var d = @(@) class { };",
];

/// Expressions, each a whole expression statement.
const EXPRESSIONS: &[&str] = &[
    "@ + 1",
    "1 + @",
    "@ && 1",
    "1 || @",
    "@ ?? 1",
    "-@",
    "typeof @",
    "void @",
    "@++",
    "--@",
    "(@)",
    "(@, 1)",
    "(1, @)",
    "a = @",
    "@ = 1",
    "@ += 1",
    "[@] = [1]",
    "[a = @] = []",
    "[...@] = []",
    "[[@]] = [[1]]",
    "({ a: @ } = {})",
    "({ a = @ } = {})",
    "({ [@]: a } = {})",
    "({ ...@ } = {})",
    "({ a: { b: @ } } = {})",
    "1 ? @ : 2",
    "1 ? 2 : @",
    "@ ? 1 : 2",
    "@()",
    "f(@)",
    "f(...@)",
    "new @()",
    "new f(@)",
    "@?.()",
    "f?.(@)",
    "@?.a",
    "a?.[@]",
    "a[@]",
    "@.a",
    "@[1]",
    "@`t`",
    "f`${@}`",
    "`${@}`",
    "[@]",
    "[...@]",
    "({ k: @ })",
    "({ [@]: 1 })",
    "({ ...@ })",
    "({ m() { @ } })",
    "({ get g() { return @; } })",
    "({ set s(v) { @; } })",
    "({ m(a = @) { } })",
    "({ [@]() { } })",
    "({ get [@]() { return 1; } })",
    "(function () { @ })",
    "(function (a = @) { })",
    "(function ([a = @]) { })",
    "(function* () { yield @; })",
    "(async function () { await @; })",
    "(class { m() { @ } })",
    "(class extends @ { })",
    "(a = @) => a",
    "(a) => @",
    "(a) => { @ }",
    "([a = @]) => a",
    "({ a = @ }) => a",
    "(...[a = @]) => a",
    "import(@)",
    "import('a', @)",
    "@ in {}",
    "#x in this",
    "#x in @",
    "1 in @",
];

fn error_of(source: &str) -> Option<String> {
    parse(source).err().map(|error| error.message)
}

const UNDECLARED: &str = "private name is not declared in an enclosing class";

/// How many of `templates`, put in the positions `wrap` builds, are rejected
/// because of the undeclared private name (the others are invalid in that
/// setting for some other reason, which must then not be about private names).
fn rejected_for_the_private_name(
    templates: &[&str],
    reference: &str,
    wrap: impl Fn(&str) -> String,
) -> usize {
    let mut rejected = 0;
    for template in templates {
        let source = wrap(&template.replace('@', reference));
        match error_of(&source) {
            Some(error) if error.starts_with(UNDECLARED) => rejected += 1,
            Some(error) => assert!(!error.contains("private name"), "{source}: {error}"),
            None => panic!("{source} was accepted"),
        }
    }
    rejected
}

/// A class expression whose method reads a private name nothing declares.
const CLASS_WITH_REFERENCE: &str = "(class { m() { this.#x; } })";

#[test]
fn an_undeclared_private_name_is_reported_in_every_position_of_class_code() {
    let statements = rejected_for_the_private_name(STATEMENTS, REFERENCE, |statement| {
        format!("class C {{ m() {{ {statement} }} }}")
    });
    let expressions = rejected_for_the_private_name(EXPRESSIONS, REFERENCE, |expression| {
        format!("class C {{ async *m() {{ {expression}; }} }}")
    });
    assert!(statements > 55, "{statements}");
    assert!(expressions > 60, "{expressions}");
}

#[test]
fn an_undeclared_private_name_is_reported_in_every_position_of_sloppy_code() {
    // A class in each position, so the walk reaches it through every kind of
    // enclosing statement or expression, sloppy-only ones (`with`, the Annex B
    // `for-in` initializer) included.
    let statements = rejected_for_the_private_name(STATEMENTS, CLASS_WITH_REFERENCE, |statement| {
        statement.to_string()
    });
    let expressions =
        rejected_for_the_private_name(EXPRESSIONS, CLASS_WITH_REFERENCE, |expression| {
            format!("async function* g() {{ {expression}; }}")
        });
    assert!(statements > 40, "{statements}");
    assert!(expressions > 45, "{expressions}");
}

#[test]
fn the_same_positions_accept_a_declared_private_name() {
    for template in STATEMENTS {
        let statement = template.replace('@', REFERENCE);
        let source = format!("class C {{ #x; m() {{ {statement} }} }}");
        if let Some(error) = error_of(&source) {
            assert!(!error.contains("private name"), "{source}: {error}");
        }
    }
}

#[test]
fn super_cannot_reach_a_private_element() {
    for source in [
        "class C { #x; m() { super.#x; } }",
        "class C { #x; m() { super?.#x; } }",
    ] {
        let error = error_of(source).unwrap_or_else(|| panic!("{source} was accepted"));
        assert!(
            error.starts_with("super cannot access a private element"),
            "{source}: {error}"
        );
    }
}
