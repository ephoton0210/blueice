// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{parse, parse_module, DeclKind, Expr, ForHead, ForInit, Stmt};

#[test]
fn reserved_labels_and_static_block_bindings_are_known_syntax_errors() {
    for source in [
        "async function f() { await: ; }",
        r"async function f() { a\u0077ait: ; }",
        "function* f() { yield: ; }",
        r"function* f() { y\u0069eld: ; }",
        "class C { static { await: ; } }",
        "class C { static { function await() {} } }",
        "class C { static { try {} catch (await) {} } }",
        "class C { static { try {} catch ({await}) {} } }",
    ] {
        let error = parse(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
    }
    for source in ["await: ;", r"a\u0077ait: ;", "function f() { await: ; }"] {
        let error = parse_module(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
    }
    for source in [
        "await: ; yield: ;",
        r"a\u0077ait: ; y\u0069eld: ;",
        "class C { static { function f() { await: ; try {} catch (await) {} } } }",
    ] {
        parse(source).expect(source);
    }
}

#[test]
fn declaration_for_heads_reject_invalid_bindings_and_initializers() {
    for (source, message) in [
        ("for (using enum = null;;) ;", "reserved word"),
        (
            "for (using first = null, [second] = null;;) ;",
            "a using declaration cannot use a destructuring pattern",
        ),
        (
            "for (using first = null, {second} = null;;) ;",
            "a using declaration cannot use a destructuring pattern",
        ),
        (
            "for (using first = null, ;;) ;",
            "expected a binding target",
        ),
        (
            "for (using first = null, second = ;;) ;",
            "expected an expression",
        ),
        (
            "for (let first = 1, second = ;;) ;",
            "expected an expression",
        ),
        (
            "for (let first = 1, let = 2;;) ;",
            "a lexical declaration cannot bind the name 'let'",
        ),
    ] {
        let error = parse(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
        assert!(error.message.contains(message), "{source}: {error:?}");
    }
}

#[test]
fn statement_terminators_reject_adjacent_tokens_without_a_line_break() {
    for source in [
        "throw value other;",
        "async function f() { await using value = null other; }",
    ] {
        let error = parse(source).expect_err(source);
        assert!(error.known_syntax, "{source}: {error:?}");
        assert!(
            error.message.contains("expected ';'"),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn automatic_semicolons_keep_let_expressions_and_empty_returns() {
    let program = parse("if (true) let\n; label: let\n; function f() { return\nvalue; }")
        .expect("line breaks terminate embedded let expressions and return");
    assert!(matches!(&program.body[0], Stmt::If { consequent, .. }
        if matches!(consequent.as_ref(), Stmt::Expr(Expr::Identifier(name)) if name == "let")));
    assert!(matches!(&program.body[1], Stmt::Labelled { item, .. }
        if matches!(item.as_ref(), Stmt::Expr(Expr::Identifier(name)) if name == "let")));
    assert!(matches!(&program.body[2], Stmt::FunctionDecl(function)
        if matches!(function.body.as_slice(), [Stmt::Return(None), Stmt::Expr(Expr::Identifier(name))] if name == "value")));
    for source in ["function f() { return; }", "function f() { return }"] {
        let program = parse(source).unwrap();
        assert!(matches!(&program.body[0], Stmt::FunctionDecl(function)
            if matches!(function.body.as_slice(), [Stmt::Return(None)])));
    }
}

#[test]
fn iteration_and_classic_heads_preserve_their_distinct_ast_forms() {
    let program = parse(
        "for ([value] in source) ; for ({value} of source) ; \
         for (value in source) ; for (value of source) ; \
         for ({value: 1};;) ; for (var value = 1 in source) ; \
         for (using first = null, second = null;;) ;",
    )
    .unwrap();
    assert!(matches!(
        &program.body[0],
        Stmt::ForIn {
            left: ForHead::Assignment(_),
            ..
        }
    ));
    assert!(matches!(
        &program.body[1],
        Stmt::ForOf {
            left: ForHead::Assignment(_),
            ..
        }
    ));
    assert!(matches!(
        &program.body[2],
        Stmt::ForIn {
            left: ForHead::Assignment(_),
            ..
        }
    ));
    assert!(matches!(
        &program.body[3],
        Stmt::ForOf {
            left: ForHead::Assignment(_),
            ..
        }
    ));
    assert!(matches!(
        &program.body[4],
        Stmt::For {
            init: Some(ForInit::Expr(Expr::Object(_))),
            ..
        }
    ));
    assert!(matches!(
        &program.body[5],
        Stmt::ForIn {
            left: ForHead::AnnexBVarInit(_, _),
            ..
        }
    ));
    assert!(
        matches!(&program.body[6], Stmt::For { init: Some(ForInit::VarDecl(DeclKind::Using, declarations)), .. }
        if declarations.len() == 2 && declarations.iter().all(|declaration| declaration.init.is_some()))
    );
}
