// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn expression_origin_trees_keep_identical_children_in_their_original_order() {
    for source in [
        "new C(C)",
        "new N.C(N.C)",
        "f(x, x)",
        "x + x + x",
        "x && x && x",
        "x ?? x ?? x",
        "x ? x : x",
        "x = x = x",
        "x[x][x]",
        "{x, field:x, [x]:x, ...x}",
        "[x,,x]",
        "[x,x,...x]",
        "(x,x,x)",
        "!!x",
        "x!!.field",
        "(x).field",
        "x << x << x",
        "x ** x ** x",
    ] {
        let mut tokens = lex(ENTRY, source).unwrap();
        tokens.pop(); // The expression parser accepts a slice without EOF.
        let ordinary = ExpressionLowerer::new(ENTRY, &tokens).parse().unwrap();
        let (expression, origins) = ExpressionLowerer::new(ENTRY, &tokens)
            .parse_with_origins()
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(expression, ordinary, "{source}");
        let mut actual = Vec::new();
        origins
            .bind(&expression, &mut |node, offset| {
                if let bluejs::Expr::Identifier(name) = node {
                    actual.push((name.clone(), offset));
                }
            })
            .unwrap();
        let expected = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| {
                if token.kind != TokenKind::Identifier
                    || index > 0 && tokens[index - 1].text == "."
                    || tokens.get(index + 1).is_some_and(|next| next.text == ":")
                        && index > 0
                        && matches!(tokens[index - 1].text.as_str(), "{" | ",")
                {
                    None
                } else {
                    Some((token.text.clone(), token.start))
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{source}");

        // Location collection must not change executable bytes or constants.
        let program = |expression| {
            bluejs::BlueJsProgramV1::Script(bluejs::Program {
                body: vec![bluejs::Stmt::Expr(expression)],
            })
        };
        let replayed = program(expression).compile().unwrap();
        let ordinary = program(ordinary).compile().unwrap();
        assert_eq!(replayed.bytes(), ordinary.bytes(), "{source}");
        assert_eq!(replayed.constants(), ordinary.constants(), "{source}");
    }
}

#[test]
fn direct_expression_origin_capture_refuses_oversized_metadata() {
    let source = format!("[{}]", "x,".repeat(50_000));
    let mut tokens = lex(ENTRY, &source).unwrap();
    tokens.pop();
    assert!(ExpressionLowerer::new(ENTRY, &tokens).parse().is_ok());
    assert!(matches!(
        ExpressionLowerer::new(ENTRY, &tokens).parse_with_origins(),
        Err(BridgeError::ProvenanceAttachment(message)) if message.contains("byte budget")
    ));
}
