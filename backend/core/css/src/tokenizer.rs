// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CSS tokenizer: a `&str` -> [`Token`] stream, per the MVP subset of
//! CSS Syntax Level 3 tokenization needed by `../parser.rs` and
//! `../selector.rs`.
//!
//! Scope, per `phase-2-mvp-scope/PLAN.md`'s "MVP CSS scope": written
//! from scratch rather than vendoring the standalone `cssparser` crate
//! or Stylo's `style`/`selectors` crates -- consistent with the
//! project's "written from scratch" identity and the same choice
//! already made for the HTML tokenizer (`research/css-cascade.md` §4).
//! Comments (`/* ... */`) are skipped entirely, folded into surrounding
//! whitespace, matching the CSS Syntax spec treating them as
//! insignificant separators. CDO/CDC (`<!--`/`-->`, a legacy
//! HTML-comment-hiding mechanism for inline `<style>` blocks) are not
//! recognized -- vanishingly rare in modern authored CSS, and a
//! deliberate cut consistent with the HTML tokenizer's own precedent of
//! naming what's cut rather than silently ignoring it.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    AtKeyword(String),
    /// `#foo` -- the raw text after `#`, with whether every character
    /// was valid in an identifier (an "id-like" hash, e.g. `#foo`) as
    /// opposed to an arbitrary hex run (e.g. `#1a2b3c`, still a valid
    /// hash token per the CSS Syntax spec, just not `ident`-shaped).
    /// Selector parsing wants the former (ID selectors); color parsing
    /// accepts either.
    Hash(String),
    QuotedString(String),
    Number(f64),
    Percentage(f64),
    Dimension(f64, String),
    Whitespace,
    Colon,
    Semicolon,
    Comma,
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    /// Any other single-character punctuation (`.`, `>`, `*`, `=`, `+`,
    /// `~`, `!`, ...) not otherwise classified above.
    Delim(char),
    Eof,
}

pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '-' || !c.is_ascii()
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || !c.is_ascii()
}

impl Tokenizer {
    pub fn new(input: &str) -> Self {
        Tokenizer {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.input.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    pub fn next_token(&mut self) -> Token {
        // Comments are consumed as nothing (per CSS Syntax): they're a
        // separator between tokens, not whitespace content in their own
        // right, so pure-comment input (or a comment right at EOF)
        // shouldn't manufacture a spurious Whitespace token. Only an
        // actual whitespace *character* seen along the way does that --
        // whitespace/comments freely interleave and still collapse to
        // one token, matching consume_whitespace's own loop.
        let mut saw_whitespace_char = false;
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.advance();
                    saw_whitespace_char = true;
                }
                Some('/') if self.peek_at(1) == Some('*') => self.skip_comment(),
                _ => break,
            }
        }
        if saw_whitespace_char {
            return Token::Whitespace;
        }

        match self.peek() {
            None => Token::Eof,
            Some('"') | Some('\'') => self.consume_string(),
            Some('#') => self.consume_hash(),
            Some('@') => self.consume_at_keyword(),
            Some(c) if c.is_ascii_digit() => self.consume_numeric(),
            Some('.') if self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) => self.consume_numeric(),
            Some('+') if self.peek_at(1).is_some_and(|c| c.is_ascii_digit() || c == '.') => self.consume_numeric(),
            Some('-') if self.peek_at(1).is_some_and(|c| c.is_ascii_digit() || c == '.') => self.consume_numeric(),
            Some('-') if self.starts_ident_after_minus() => self.consume_ident_like(),
            Some(c) if c != '-' && is_ident_start(c) => self.consume_ident_like(),
            Some(':') => {
                self.advance();
                Token::Colon
            }
            Some(';') => {
                self.advance();
                Token::Semicolon
            }
            Some(',') => {
                self.advance();
                Token::Comma
            }
            Some('{') => {
                self.advance();
                Token::LeftBrace
            }
            Some('}') => {
                self.advance();
                Token::RightBrace
            }
            Some('(') => {
                self.advance();
                Token::LeftParen
            }
            Some(')') => {
                self.advance();
                Token::RightParen
            }
            Some('[') => {
                self.advance();
                Token::LeftBracket
            }
            Some(']') => {
                self.advance();
                Token::RightBracket
            }
            Some(c) => {
                self.advance();
                Token::Delim(c)
            }
        }
    }

    fn starts_ident_after_minus(&self) -> bool {
        match self.peek_at(1) {
            Some(c) if is_ident_start(c) => true,
            Some('-') => true,
            _ => false,
        }
    }

    fn skip_comment(&mut self) {
        self.pos += 2; // consume "/*"
        while self.peek().is_some() && !(self.peek() == Some('*') && self.peek_at(1) == Some('/')) {
            self.advance();
        }
        if self.peek().is_some() {
            self.pos += 2; // consume "*/"
        }
    }

    fn consume_string(&mut self) -> Token {
        let quote = self.advance().unwrap();
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c == quote {
                self.advance();
                break;
            }
            if c == '\\' {
                self.advance();
                if let Some(escaped) = self.advance() {
                    s.push(escaped);
                }
                continue;
            }
            s.push(c);
            self.advance();
        }
        Token::QuotedString(s)
    }

    fn consume_hash(&mut self) -> Token {
        self.advance(); // '#'
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        Token::Hash(s)
    }

    fn consume_at_keyword(&mut self) -> Token {
        self.advance(); // '@'
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        Token::AtKeyword(s)
    }

    fn consume_ident_like(&mut self) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        Token::Ident(s)
    }

    fn consume_numeric(&mut self) -> Token {
        let mut s = String::new();
        if matches!(self.peek(), Some('+') | Some('-')) {
            s.push(self.advance().unwrap());
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            s.push(self.advance().unwrap());
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    s.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
        }
        let value: f64 = s.parse().unwrap_or(0.0);

        if self.peek() == Some('%') {
            self.advance();
            return Token::Percentage(value);
        }
        if self.peek().is_some_and(is_ident_start) {
            let mut unit = String::new();
            while let Some(c) = self.peek() {
                if is_ident_continue(c) {
                    unit.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            return Token::Dimension(value, unit.to_ascii_lowercase());
        }
        Token::Number(value)
    }
}

/// Tokenizes `input` fully, dropping [`Token::Whitespace`] runs down to
/// a single marker but never dropping them entirely -- callers that
/// don't care (declaration/value parsing) filter them out; the selector
/// parser needs them to distinguish the descendant combinator (a bare
/// whitespace token) from directly-adjacent compound selectors.
pub fn tokenize(input: &str) -> Vec<Token> {
    let mut tokenizer = Tokenizer::new(input);
    let mut tokens = Vec::new();
    loop {
        let tok = tokenizer.next_token();
        if tok == Token::Eof {
            break;
        }
        tokens.push(tok);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idents_and_keywords() {
        assert_eq!(tokenize("div"), vec![Token::Ident("div".to_string())]);
        assert_eq!(tokenize("-webkit-foo"), vec![Token::Ident("-webkit-foo".to_string())]);
        assert_eq!(tokenize("--custom"), vec![Token::Ident("--custom".to_string())]);
    }

    #[test]
    fn at_keyword() {
        assert_eq!(tokenize("@media"), vec![Token::AtKeyword("media".to_string())]);
    }

    #[test]
    fn hash_token_for_id_selector_or_hex_color() {
        assert_eq!(tokenize("#foo"), vec![Token::Hash("foo".to_string())]);
        assert_eq!(tokenize("#1a2b3c"), vec![Token::Hash("1a2b3c".to_string())]);
    }

    #[test]
    fn numbers_dimensions_and_percentages() {
        assert_eq!(tokenize("12"), vec![Token::Number(12.0)]);
        assert_eq!(tokenize("1.5"), vec![Token::Number(1.5)]);
        assert_eq!(tokenize("-3px"), vec![Token::Dimension(-3.0, "px".to_string())]);
        assert_eq!(tokenize("50%"), vec![Token::Percentage(50.0)]);
        assert_eq!(tokenize("0"), vec![Token::Number(0.0)]);
        assert_eq!(tokenize("1.5em"), vec![Token::Dimension(1.5, "em".to_string())]);
    }

    #[test]
    fn strings_single_and_double_quoted() {
        assert_eq!(tokenize(r#""hello""#), vec![Token::QuotedString("hello".to_string())]);
        assert_eq!(tokenize("'hello'"), vec![Token::QuotedString("hello".to_string())]);
    }

    #[test]
    fn string_with_escaped_quote() {
        assert_eq!(tokenize(r#""a\"b""#), vec![Token::QuotedString("a\"b".to_string())]);
    }

    #[test]
    fn structural_punctuation() {
        assert_eq!(
            tokenize(":;,{}()[]"),
            vec![
                Token::Colon,
                Token::Semicolon,
                Token::Comma,
                Token::LeftBrace,
                Token::RightBrace,
                Token::LeftParen,
                Token::RightParen,
                Token::LeftBracket,
                Token::RightBracket,
            ]
        );
    }

    #[test]
    fn generic_delimiters() {
        assert_eq!(
            tokenize(".>*=+~!"),
            vec![
                Token::Delim('.'),
                Token::Delim('>'),
                Token::Delim('*'),
                Token::Delim('='),
                Token::Delim('+'),
                Token::Delim('~'),
                Token::Delim('!'),
            ]
        );
    }

    #[test]
    fn whitespace_is_a_single_token_regardless_of_run_length() {
        assert_eq!(tokenize("a   b"), vec![Token::Ident("a".to_string()), Token::Whitespace, Token::Ident("b".to_string())]);
        assert_eq!(tokenize("a\n\t b"), vec![Token::Ident("a".to_string()), Token::Whitespace, Token::Ident("b".to_string())]);
    }

    #[test]
    fn comment_with_no_adjacent_real_whitespace_produces_no_whitespace_token() {
        // matches the real CSS Syntax spec: a comment is consumed as
        // nothing, not as whitespace -- `a/**/b` is two tokens with no
        // separator between them, not the same as `a b`. This is why
        // the selector parser must not treat "no Whitespace token
        // between two idents" as ambiguous with descendant combinator.
        assert_eq!(tokenize("a/* comment */b"), vec![Token::Ident("a".to_string()), Token::Ident("b".to_string())]);
    }

    #[test]
    fn comment_next_to_real_whitespace_still_collapses_to_one_whitespace_token() {
        assert_eq!(
            tokenize("a /* comment */ b"),
            vec![Token::Ident("a".to_string()), Token::Whitespace, Token::Ident("b".to_string())]
        );
    }

    #[test]
    fn comment_only_input_produces_no_tokens() {
        assert_eq!(tokenize("/* just a comment */"), vec![]);
    }

    #[test]
    fn unterminated_comment_consumes_to_eof_without_hanging() {
        assert_eq!(tokenize("a/* unterminated"), vec![Token::Ident("a".to_string())]);
    }

    #[test]
    fn negative_dimension_is_not_misread_as_an_identifier() {
        assert_eq!(tokenize("-3px"), vec![Token::Dimension(-3.0, "px".to_string())]);
        assert_eq!(tokenize("-0.5em"), vec![Token::Dimension(-0.5, "em".to_string())]);
        assert_eq!(tokenize("-3"), vec![Token::Number(-3.0)]);
    }

    #[test]
    fn bare_minus_not_starting_a_number_or_identifier_is_a_delimiter() {
        assert_eq!(tokenize("1 - 2"), vec![Token::Number(1.0), Token::Whitespace, Token::Delim('-'), Token::Whitespace, Token::Number(2.0)]);
    }

    #[test]
    fn unterminated_string_consumes_to_eof() {
        assert_eq!(tokenize(r#""unterminated"#), vec![Token::QuotedString("unterminated".to_string())]);
    }

    #[test]
    fn a_full_declaration_tokenizes_as_expected() {
        assert_eq!(
            tokenize("color: red;"),
            vec![
                Token::Ident("color".to_string()),
                Token::Colon,
                Token::Whitespace,
                Token::Ident("red".to_string()),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn a_simple_selector_and_block_tokenizes_as_expected() {
        assert_eq!(
            tokenize("div.foo { color: red; }"),
            vec![
                Token::Ident("div".to_string()),
                Token::Delim('.'),
                Token::Ident("foo".to_string()),
                Token::Whitespace,
                Token::LeftBrace,
                Token::Whitespace,
                Token::Ident("color".to_string()),
                Token::Colon,
                Token::Whitespace,
                Token::Ident("red".to_string()),
                Token::Semicolon,
                Token::Whitespace,
                Token::RightBrace,
            ]
        );
    }
}
