// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The text a `content` value generates for `::before` / `::after`.

use crate::tokenizer::Token;

/// The address inside a `url(...)` value (`url(x)`, `url("x")`), if `tokens`
/// are exactly that.
pub fn url_of(tokens: &[Token]) -> Option<String> {
    let meaningful: Vec<&Token> = tokens.iter().filter(|t| **t != Token::Whitespace).collect();
    match meaningful.as_slice() {
        [Token::Url(u)] => Some(u.clone()),
        [Token::Ident(name), Token::LeftParen, Token::QuotedString(u), Token::RightParen]
            if name.eq_ignore_ascii_case("url") =>
        {
            Some(u.clone())
        }
        _ => None,
    }
}

/// The generated text, or `None` when the value generates nothing it can
/// show (`counter()`, `url()` and unknown functions among them -- a value
/// made only of those makes no box).
pub fn generated_text(tokens: &[Token], attr: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    let mut produced = false;
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::QuotedString(s) => {
                out.push_str(s);
                produced = true;
            }
            Token::Ident(word) if matches!(tokens.get(i + 1), Some(Token::LeftParen)) => {
                let close = tokens[i..]
                    .iter()
                    .position(|t| *t == Token::RightParen)
                    .map_or(tokens.len(), |p| i + p);
                if word.eq_ignore_ascii_case("attr") {
                    let name = tokens[i + 2..close].iter().find_map(|t| match t {
                        Token::Ident(n) => Some(n.as_str()),
                        _ => None,
                    });
                    if let Some(value) = name.and_then(attr) {
                        out.push_str(&value);
                    }
                    produced = true;
                }
                i = close;
            }
            Token::Ident(word) => match word.to_ascii_lowercase().as_str() {
                "open-quote" => {
                    out.push('\u{201c}');
                    produced = true;
                }
                "close-quote" => {
                    out.push('\u{201d}');
                    produced = true;
                }
                "none" | "normal" => return None,
                _ => {}
            },
            _ => {}
        }
        i += 1;
    }
    produced.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn text(css: &str) -> Option<String> {
        generated_text(&tokenize(css), &|name| {
            (name == "title").then(|| "T".to_string())
        })
    }

    #[test]
    fn strings_join_and_empty_still_makes_a_box() {
        assert_eq!(text("\"a\" \"b\""), Some("ab".to_string()));
        assert_eq!(text("\"\""), Some(String::new()));
    }

    #[test]
    fn css_escapes_decode() {
        assert_eq!(text("\"\\201C\""), Some("\u{201c}".to_string()));
    }

    #[test]
    fn attr_and_quotes() {
        assert_eq!(
            text("attr(title) open-quote"),
            Some("T\u{201c}".to_string())
        );
        assert_eq!(text("attr(missing)"), Some(String::new()));
    }

    #[test]
    fn url_values_are_read_quoted_or_not() {
        assert_eq!(url_of(&tokenize("url(a/b.png?x=1)")), Some("a/b.png?x=1".into()));
        assert_eq!(url_of(&tokenize("url( 'x y.png' )")), Some("x y.png".into()));
        assert_eq!(url_of(&tokenize("url(\"https://h/i.svg\")")), Some("https://h/i.svg".into()));
        assert_eq!(url_of(&tokenize("linear-gradient(red, blue)")), None);
    }

    #[test]
    fn nothing_showable_generates_nothing() {
        assert_eq!(text("counter(x)"), None);
        assert_eq!(text("url(a.png)"), None);
        assert_eq!(text("none"), None);
    }
}
