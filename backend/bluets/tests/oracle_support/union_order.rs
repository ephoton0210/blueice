// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A deliberately narrow, bounded proof of one literal return-union permutation.

use blueice_bluets::{lex, TokenKind};

pub fn only_literal_return_union_order(actual: &str, native: &str) -> bool {
    if actual == native {
        return false;
    }
    let (Some(left), Some(right)) = (parts(actual), parts(native)) else {
        return false;
    };
    left.0 == right.0
        && left.3 == right.3
        && left.1 != left.2
        && left.1 == right.2
        && left.2 == right.1
}

fn parts(source: &str) -> Option<(&str, &str, &str, &str)> {
    if source.len() > 4096 {
        return None;
    }
    let tokens = lex("declaration.d.ts", source).ok()?;
    let [export, declare, function, name, open, close, colon, first, pipe, second, semicolon, eof] =
        tokens.as_slice()
    else {
        return None;
    };
    if !export.is("export")
        || !declare.is("declare")
        || !function.is("function")
        || name.kind != TokenKind::Identifier
        || !open.is("(")
        || !close.is(")")
        || !colon.is(":")
        || !pipe.is("|")
        || !semicolon.is(";")
        || eof.kind != TokenKind::Eof
        || !matches!(first.kind, TokenKind::String | TokenKind::Number)
        || !matches!(second.kind, TokenKind::String | TokenKind::Number)
        || &source[first.end..second.start] != " | "
    {
        return None;
    }
    Some((
        &source[..first.start],
        &source[first.start..first.end],
        &source[second.start..second.end],
        &source[second.end..],
    ))
}

#[test]
fn union_order_guard_accepts_only_the_same_literal_members() {
    let native = "export declare function read(): 1 | \"discarded\";\n";
    let actual = "export declare function read(): \"discarded\" | 1;\n";
    assert!(only_literal_return_union_order(actual, native));
    // A quoted pipe is part of a member, not a union delimiter.
    assert!(only_literal_return_union_order(
        "export declare function read(): \"a|b\" | 1;\n",
        "export declare function read(): 1 | \"a|b\";\n"
    ));
}

#[test]
fn union_order_guard_rejects_every_non_permutation_change() {
    let native = "export declare function read(): 1 | \"discarded\";\n";
    for actual in [
        "export declare function read(): 1;\n",
        "export declare function read(): 1 | \"discarded\" | 2;\n",
        "export declare function read(): number | \"discarded\";\n",
        "export declare function read(): 1 | \"changed\";\n",
        "export declare function read(): 1 | 1;\n",
        "export declare function read(): (\"discarded\" | 1);\n",
        "export declare function read(): \"discarded\" & 1;\n",
        "export declare function read(): [\"discarded\", 1];\n",
        "export declare function read(value: number): \"discarded\" | 1;\n",
        "export declare function renamed(): \"discarded\" | 1;\n",
        "export declare function read(): \"discarded\"|1;\n",
        "export declare function read(): \"discarded\" | 1;\r\n",
        "export declare function read(): \"discarded\" | 1;\n// changed\n",
        "export declare function read(): \"discarded\" | 1;\nexport type Extra = 1;\n",
        "export declare function read(): Alias;\n",
    ] {
        assert!(!only_literal_return_union_order(actual, native), "{actual}");
    }
    assert!(!only_literal_return_union_order(&" ".repeat(4097), native));
    assert!(!only_literal_return_union_order(native, native));
}
