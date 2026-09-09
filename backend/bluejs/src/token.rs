// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS tokenizer: a `&str` -> [`Token`] stream, scoped to exactly
//! `phase-2-mvp-scope/PLAN.md`'s "MVP JS scope (decided)" -- not the
//! full ECMAScript lexical grammar. Notably absent on purpose (not
//! oversights): regex literals (the MVP JS scope defers regular
//! expressions entirely), bitwise/shift operators and `**`
//! (`&`, `|`, `^`, `~`, `<<`, `>>`, `>>>`, `**` -- the scoped "standard
//! operator set" names arithmetic/comparison/logical/ternary/
//! `typeof`/`instanceof` only, and a hand-written DOM script
//! essentially never needs bitwise math), non-decimal numeric literals
//! (`0x..`/`0o..`/`0b..`), tagged templates, and legacy octal escapes.
//! [`Keyword`] mirrors this: `undefined` is deliberately NOT a keyword
//! here (unlike `null`/`true`/`false`) because it isn't one in real
//! ECMAScript either -- it's an ordinary identifier bound to a global
//! property, so it tokenizes as [`Token::Identifier`] and the parser/
//! interpreter, not the lexer, is where it becomes meaningful.
//!
//! Every character is scanned as a `char` (a Unicode scalar value), not
//! a UTF-16 code unit -- unlike a spec-faithful engine, this project has
//! no reason to reproduce ECMAScript's UTF-16-surrogate-pair string
//! indexing for an MVP subset that never inspects `.length` against
//! astral-plane input.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    /// A plain (non-template) string literal, already "cooked" --
    /// escape sequences resolved to the characters they represent.
    String(String),
    /// A template literal (`` `...` ``). `quasis.len() ==
    /// raw_expressions.len() + 1` always holds, the same invariant
    /// real engines' `TemplateStringsArray` relies on: quasis are the
    /// cooked string pieces between `${`/`}` boundaries, and
    /// `raw_expressions` is each placeholder's *unparsed source text*
    /// (not a token stream) -- see this module's doc comment on
    /// [`Tokenizer::scan_template`] for why re-lexing/re-parsing that
    /// text later, rather than recursively tokenizing it inline here,
    /// is this crate's chosen way to handle template nesting without a
    /// stateful lexer-mode stack.
    Template { quasis: Vec<String>, raw_expressions: Vec<String> },
    Identifier(String),
    Keyword(Keyword),
    Punct(Punct),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    Var,
    Let,
    Const,
    Function,
    Return,
    If,
    Else,
    For,
    While,
    Do,
    Switch,
    Case,
    Default,
    Break,
    Continue,
    Throw,
    Try,
    Catch,
    Finally,
    New,
    Typeof,
    Instanceof,
    In,
    True,
    False,
    Null,
    This,
}

impl Keyword {
    fn from_str(s: &str) -> Option<Keyword> {
        Some(match s {
            "var" => Keyword::Var,
            "let" => Keyword::Let,
            "const" => Keyword::Const,
            "function" => Keyword::Function,
            "return" => Keyword::Return,
            "if" => Keyword::If,
            "else" => Keyword::Else,
            "for" => Keyword::For,
            "while" => Keyword::While,
            "do" => Keyword::Do,
            "switch" => Keyword::Switch,
            "case" => Keyword::Case,
            "default" => Keyword::Default,
            "break" => Keyword::Break,
            "continue" => Keyword::Continue,
            "throw" => Keyword::Throw,
            "try" => Keyword::Try,
            "catch" => Keyword::Catch,
            "finally" => Keyword::Finally,
            "new" => Keyword::New,
            "typeof" => Keyword::Typeof,
            "instanceof" => Keyword::Instanceof,
            "in" => Keyword::In,
            "true" => Keyword::True,
            "false" => Keyword::False,
            "null" => Keyword::Null,
            "this" => Keyword::This,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Punct {
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semicolon,
    Comma,
    Colon,
    Dot,
    Ellipsis,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PlusPlus,
    MinusMinus,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    EqEq,
    NotEq,
    EqEqEq,
    NotEqEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AndAnd,
    OrOr,
    Bang,
    Question,
    QuestionQuestion,
}

/// One lexical error -- a plain message rather than a structured enum,
/// matching this codebase's precedent of not over-engineering error
/// types for a from-scratch MVP subsystem (see e.g. `blueice-html`'s
/// own error handling, which mostly just recovers instead of erroring
/// at all). Unlike HTML/CSS's "never fail the whole parse" philosophy,
/// JS syntax errors are real spec-mandated failures with no defined
/// recovery, so this crate surfaces them as `Err` rather than silently
/// skipping malformed input.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
}

impl LexError {
    fn new(message: impl Into<String>) -> LexError {
        LexError { message: message.into() }
    }
}

/// One token plus whether a line terminator appeared anywhere in the
/// whitespace/comments immediately before it -- the one piece of
/// lexical information ECMAScript's automatic-semicolon-insertion
/// rules need from the lexer (see `research/js-bytecode-eventloop.md`'s
/// neighboring engines-agree-on-shape precedent; ASI itself isn't
/// covered there, but real engines universally thread this same bit
/// from lexer to parser rather than having the parser re-scan
/// whitespace itself).
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub newline_before: bool,
}

pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

impl Tokenizer {
    pub fn new(input: &str) -> Tokenizer {
        Tokenizer { input: input.chars().collect(), pos: 0 }
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

    /// Skips whitespace and comments, reporting whether a line
    /// terminator was seen anywhere along the way (for ASI, see
    /// [`SpannedToken`]).
    fn skip_trivia(&mut self) -> Result<bool, LexError> {
        let mut saw_newline = false;
        loop {
            match self.peek() {
                Some('\n') => {
                    saw_newline = true;
                    self.advance();
                }
                Some(c) if c.is_whitespace() => {
                    self.advance();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                Some('/') if self.peek_at(1) == Some('*') => {
                    self.advance();
                    self.advance();
                    loop {
                        match self.peek() {
                            None => return Err(LexError::new("unterminated block comment")),
                            Some('\n') => {
                                saw_newline = true;
                                self.advance();
                            }
                            Some('*') if self.peek_at(1) == Some('/') => {
                                self.advance();
                                self.advance();
                                break;
                            }
                            Some(_) => {
                                self.advance();
                            }
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(saw_newline)
    }

    pub fn next_spanned(&mut self) -> Result<SpannedToken, LexError> {
        let newline_before = self.skip_trivia()?;
        let token = self.next_token()?;
        Ok(SpannedToken { token, newline_before })
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        let c = match self.peek() {
            None => return Ok(Token::Eof),
            Some(c) => c,
        };
        if c.is_ascii_digit() || (c == '.' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit())) {
            return self.scan_number();
        }
        if c == '"' || c == '\'' {
            return self.scan_string(c);
        }
        if c == '`' {
            self.advance();
            return self.scan_template();
        }
        if is_ident_start(c) {
            return Ok(self.scan_identifier_or_keyword());
        }
        self.scan_punct()
    }

    fn scan_number(&mut self) -> Result<Token, LexError> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.advance();
        }
        if self.peek() == Some('.') {
            self.advance();
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.advance();
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            let save = self.pos;
            self.advance();
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.advance();
            }
            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.advance();
                }
            } else {
                // Not actually an exponent (e.g. `1.e`, a trailing
                // stray identifier char) -- back out rather than
                // consuming a malformed exponent.
                self.pos = save;
            }
        }
        let text: String = self.input[start..self.pos].iter().collect();
        text.parse::<f64>().map(Token::Number).map_err(|_| LexError::new(format!("invalid number literal '{text}'")))
    }

    fn scan_escape(&mut self) -> Result<Option<char>, LexError> {
        // Caller has already consumed the leading '\\'.
        let c = self.advance().ok_or_else(|| LexError::new("unterminated escape sequence"))?;
        Ok(Some(match c {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'v' => '\u{b}',
            '0' => '\0',
            '\n' => return Ok(None), // line continuation: escaped newline contributes nothing
            '\'' | '"' | '`' | '\\' | '$' => c,
            'x' => {
                let hex: String = (0..2).map(|_| self.advance().ok_or_else(|| LexError::new("unterminated \\x escape"))).collect::<Result<_, _>>()?;
                let code = u32::from_str_radix(&hex, 16).map_err(|_| LexError::new(format!("invalid \\x escape '{hex}'")))?;
                char::from_u32(code).ok_or_else(|| LexError::new("invalid \\x escape codepoint"))?
            }
            'u' => {
                if self.peek() == Some('{') {
                    self.advance();
                    let mut hex = String::new();
                    while self.peek().is_some_and(|c| c != '}') {
                        hex.push(self.advance().unwrap());
                    }
                    self.advance().ok_or_else(|| LexError::new("unterminated \\u{...} escape"))?;
                    let code = u32::from_str_radix(&hex, 16).map_err(|_| LexError::new(format!("invalid \\u{{...}} escape '{hex}'")))?;
                    char::from_u32(code).ok_or_else(|| LexError::new("invalid \\u{...} escape codepoint"))?
                } else {
                    let hex: String = (0..4).map(|_| self.advance().ok_or_else(|| LexError::new("unterminated \\u escape"))).collect::<Result<_, _>>()?;
                    let code = u32::from_str_radix(&hex, 16).map_err(|_| LexError::new(format!("invalid \\u escape '{hex}'")))?;
                    char::from_u32(code).ok_or_else(|| LexError::new("invalid \\u escape codepoint"))?
                }
            }
            other => other, // an unrecognized escape just yields the escaped character itself, matching ECMAScript's `NonEscapeCharacter` fallback
        }))
    }

    fn scan_string(&mut self, quote: char) -> Result<Token, LexError> {
        self.advance(); // opening quote
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(LexError::new("unterminated string literal")),
                Some(c) if c == quote => {
                    self.advance();
                    return Ok(Token::String(out));
                }
                Some('\n') => return Err(LexError::new("unterminated string literal (line terminator)")),
                Some('\\') => {
                    self.advance();
                    if let Some(c) = self.scan_escape()? {
                        out.push(c);
                    }
                }
                Some(c) => {
                    self.advance();
                    out.push(c);
                }
            }
        }
    }

    /// Scans a template literal's body (opening backtick already
    /// consumed). Placeholder expressions (`${...}`) are captured as
    /// raw, unparsed source text rather than recursively tokenized
    /// in-line -- this crate's `Parser` re-tokenizes/re-parses each one
    /// independently as an ordinary expression once template AST
    /// construction needs it (`parser.rs`'s `parse_template`). This
    /// trades a small amount of re-scanning for avoiding a second,
    /// stateful "lexer mode stack" (the technique real engines use to
    /// resume template scanning after a `}` closes a placeholder) --
    /// a deliberate simplicity-over-performance call consistent with
    /// this phase's own "design for later performance, don't build it
    /// now" priority.
    fn scan_template(&mut self) -> Result<Token, LexError> {
        let mut quasis = Vec::new();
        let mut raw_expressions = Vec::new();
        let mut current = String::new();
        loop {
            match self.peek() {
                None => return Err(LexError::new("unterminated template literal")),
                Some('`') => {
                    self.advance();
                    quasis.push(current);
                    return Ok(Token::Template { quasis, raw_expressions });
                }
                Some('$') if self.peek_at(1) == Some('{') => {
                    self.advance();
                    self.advance();
                    quasis.push(std::mem::take(&mut current));
                    raw_expressions.push(self.scan_raw_until_matching_brace()?);
                }
                Some('\\') => {
                    self.advance();
                    if let Some(c) = self.scan_escape()? {
                        current.push(c);
                    }
                }
                Some(c) => {
                    self.advance();
                    current.push(c);
                }
            }
        }
    }

    /// Consumes raw source text up to (and including, but not returning)
    /// the `}` that closes a template placeholder's `${` -- tracking
    /// brace depth and skipping over nested string/template literals so
    /// a `}` or a `{`/`}` pair *inside* a nested string or template
    /// (e.g. `` `${ `${a}` }` `` or `${ "}" }`) doesn't miscount.
    fn scan_raw_until_matching_brace(&mut self) -> Result<String, LexError> {
        let mut depth = 1u32;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(LexError::new("unterminated template placeholder")),
                Some('{') => {
                    depth += 1;
                    out.push(self.advance().unwrap());
                }
                Some('}') => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance();
                        return Ok(out);
                    }
                    out.push(self.advance().unwrap());
                }
                Some(q @ ('"' | '\'')) => {
                    out.push(q);
                    self.advance();
                    self.copy_raw_string_body(q, &mut out)?;
                }
                Some('`') => {
                    out.push('`');
                    self.advance();
                    self.copy_raw_template_body(&mut out)?;
                }
                Some(c) => {
                    out.push(c);
                    self.advance();
                }
            }
        }
    }

    /// Copies a plain string literal's raw source (delimiter already
    /// consumed by the caller and pushed to `out`) verbatim into `out`,
    /// including its closing delimiter -- for
    /// [`Tokenizer::scan_raw_until_matching_brace`]'s brace-counting to
    /// skip over quoted braces without needing to interpret escapes.
    fn copy_raw_string_body(&mut self, quote: char, out: &mut String) -> Result<(), LexError> {
        loop {
            match self.peek() {
                None => return Err(LexError::new("unterminated string literal inside template placeholder")),
                Some(c) if c == quote => {
                    out.push(c);
                    self.advance();
                    return Ok(());
                }
                Some('\\') => {
                    out.push('\\');
                    self.advance();
                    if let Some(c) = self.advance() {
                        out.push(c);
                    }
                }
                Some(c) => {
                    out.push(c);
                    self.advance();
                }
            }
        }
    }

    /// Same idea as [`Tokenizer::copy_raw_string_body`], but for a
    /// nested template literal, recursing into its own `${...}`
    /// placeholders (via [`Tokenizer::scan_raw_until_matching_brace`])
    /// so a `{`/`}` nested arbitrarily deep still balances correctly.
    fn copy_raw_template_body(&mut self, out: &mut String) -> Result<(), LexError> {
        loop {
            match self.peek() {
                None => return Err(LexError::new("unterminated template literal inside template placeholder")),
                Some('`') => {
                    out.push('`');
                    self.advance();
                    return Ok(());
                }
                Some('\\') => {
                    out.push('\\');
                    self.advance();
                    if let Some(c) = self.advance() {
                        out.push(c);
                    }
                }
                Some('$') if self.peek_at(1) == Some('{') => {
                    out.push_str("${");
                    self.advance();
                    self.advance();
                    let inner = self.scan_raw_until_matching_brace()?;
                    out.push_str(&inner);
                    out.push('}');
                }
                Some(c) => {
                    out.push(c);
                    self.advance();
                }
            }
        }
    }

    fn scan_identifier_or_keyword(&mut self) -> Token {
        let start = self.pos;
        while self.peek().is_some_and(is_ident_continue) {
            self.advance();
        }
        let text: String = self.input[start..self.pos].iter().collect();
        match Keyword::from_str(&text) {
            Some(kw) => Token::Keyword(kw),
            None => Token::Identifier(text),
        }
    }

    fn scan_punct(&mut self) -> Result<Token, LexError> {
        // Note: the leading character is already consumed by `let c =
        // self.advance()` below, so this only needs to check for (and
        // possibly consume) the second character.
        macro_rules! two {
            ($second:expr, $with:expr, $without:expr) => {{
                if self.peek() == Some($second) {
                    self.advance();
                    $with
                } else {
                    $without
                }
            }};
        }
        let c = self.advance().unwrap();
        let punct = match c {
            '(' => Punct::LParen,
            ')' => Punct::RParen,
            '{' => Punct::LBrace,
            '}' => Punct::RBrace,
            '[' => Punct::LBracket,
            ']' => Punct::RBracket,
            ';' => Punct::Semicolon,
            ',' => Punct::Comma,
            ':' => Punct::Colon,
            '.' => {
                if self.peek() == Some('.') && self.peek_at(1) == Some('.') {
                    self.advance();
                    self.advance();
                    Punct::Ellipsis
                } else {
                    Punct::Dot
                }
            }
            '+' => match self.peek() {
                Some('+') => {
                    self.advance();
                    Punct::PlusPlus
                }
                Some('=') => {
                    self.advance();
                    Punct::PlusAssign
                }
                _ => Punct::Plus,
            },
            '-' => match self.peek() {
                Some('-') => {
                    self.advance();
                    Punct::MinusMinus
                }
                Some('=') => {
                    self.advance();
                    Punct::MinusAssign
                }
                _ => Punct::Minus,
            },
            '*' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Punct::StarAssign
                } else {
                    Punct::Star
                }
            }
            '/' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Punct::SlashAssign
                } else {
                    Punct::Slash
                }
            }
            '%' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Punct::PercentAssign
                } else {
                    Punct::Percent
                }
            }
            '=' => {
                if self.peek() == Some('=') && self.peek_at(1) == Some('=') {
                    self.advance();
                    self.advance();
                    Punct::EqEqEq
                } else if self.peek() == Some('=') {
                    self.advance();
                    Punct::EqEq
                } else if self.peek() == Some('>') {
                    self.advance();
                    Punct::Arrow
                } else {
                    Punct::Assign
                }
            }
            '!' => {
                if self.peek() == Some('=') && self.peek_at(1) == Some('=') {
                    self.advance();
                    self.advance();
                    Punct::NotEqEq
                } else if self.peek() == Some('=') {
                    self.advance();
                    Punct::NotEq
                } else {
                    Punct::Bang
                }
            }
            '<' => two!('=', Punct::LtEq, Punct::Lt),
            '>' => two!('=', Punct::GtEq, Punct::Gt),
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    Punct::AndAnd
                } else {
                    return Err(LexError::new("bitwise '&' is not supported (out of BlueJS's MVP scope)"));
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    Punct::OrOr
                } else {
                    return Err(LexError::new("bitwise '|' is not supported (out of BlueJS's MVP scope)"));
                }
            }
            '?' => {
                if self.peek() == Some('?') {
                    self.advance();
                    Punct::QuestionQuestion
                } else {
                    Punct::Question
                }
            }
            other => return Err(LexError::new(format!("unexpected character '{other}'"))),
        };
        Ok(Token::Punct(punct))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(src: &str) -> Vec<Token> {
        let mut t = Tokenizer::new(src);
        let mut out = Vec::new();
        loop {
            let spanned = t.next_spanned().unwrap();
            let is_eof = spanned.token == Token::Eof;
            out.push(spanned.token);
            if is_eof {
                return out;
            }
        }
    }

    #[test]
    fn scans_numbers() {
        assert_eq!(tokens("42"), vec![Token::Number(42.0), Token::Eof]);
        assert_eq!(tokens("2.5"), vec![Token::Number(2.5), Token::Eof]);
        assert_eq!(tokens(".5"), vec![Token::Number(0.5), Token::Eof]);
        assert_eq!(tokens("1e3"), vec![Token::Number(1000.0), Token::Eof]);
        assert_eq!(tokens("1.5e-2"), vec![Token::Number(0.015), Token::Eof]);
        assert_eq!(tokens("0"), vec![Token::Number(0.0), Token::Eof]);
    }

    #[test]
    fn a_trailing_e_with_no_exponent_digits_is_not_consumed_as_an_exponent() {
        // `1.e` isn't a valid exponent (no digits after 'e'), so the
        // tokenizer backs out and leaves `e` to be scanned as its own
        // token, matching real engines' behavior for this edge case.
        assert_eq!(tokens("1.e"), vec![Token::Number(1.0), Token::Identifier("e".to_string()), Token::Eof]);
    }

    #[test]
    fn scans_every_simple_escape_sequence() {
        assert_eq!(tokens(r"'\t\r\b\f\v\0'"), vec![Token::String("\t\r\u{8}\u{c}\u{b}\0".to_string()), Token::Eof]);
        assert_eq!(tokens(r"'\q'"), vec![Token::String("q".to_string()), Token::Eof]); // unrecognized escape: falls back to the escaped character itself
    }

    #[test]
    fn a_backslash_newline_is_a_line_continuation_contributing_no_character() {
        assert_eq!(tokens("'a\\\nb'"), vec![Token::String("ab".to_string()), Token::Eof]);
    }

    #[test]
    fn scans_a_bare_four_hex_digit_unicode_escape_without_braces() {
        assert_eq!(tokens("'\\u0041'"), vec![Token::String("A".to_string()), Token::Eof]);
    }

    #[test]
    fn scans_string_literals_with_escapes() {
        assert_eq!(tokens(r#""hello""#), vec![Token::String("hello".to_string()), Token::Eof]);
        assert_eq!(tokens("'hello'"), vec![Token::String("hello".to_string()), Token::Eof]);
        assert_eq!(tokens(r#""a\nb""#), vec![Token::String("a\nb".to_string()), Token::Eof]);
        assert_eq!(tokens(r#""a\"b""#), vec![Token::String("a\"b".to_string()), Token::Eof]);
        assert_eq!(tokens(r"'\u{1F600}'"), vec![Token::String("\u{1F600}".to_string()), Token::Eof]);
        assert_eq!(tokens(r"'A'"), vec![Token::String("A".to_string()), Token::Eof]);
        assert_eq!(tokens(r"'\x41'"), vec![Token::String("A".to_string()), Token::Eof]);
    }

    #[test]
    fn unterminated_string_is_an_error() {
        assert!(Tokenizer::new("\"abc").next_spanned().is_err());
        assert!(Tokenizer::new("\"abc\n\"").next_spanned().is_err());
    }

    #[test]
    fn scans_identifiers_and_keywords() {
        assert_eq!(tokens("foo _bar $baz"), vec![Token::Identifier("foo".to_string()), Token::Identifier("_bar".to_string()), Token::Identifier("$baz".to_string()), Token::Eof]);
        assert_eq!(tokens("undefined"), vec![Token::Identifier("undefined".to_string()), Token::Eof]);
        assert_eq!(tokens("let x = true"), vec![Token::Keyword(Keyword::Let), Token::Identifier("x".to_string()), Token::Punct(Punct::Assign), Token::Keyword(Keyword::True), Token::Eof]);
    }

    #[test]
    fn scans_a_simple_template_literal_with_no_placeholders() {
        assert_eq!(tokens("`hello`"), vec![Token::Template { quasis: vec!["hello".to_string()], raw_expressions: vec![] }, Token::Eof]);
    }

    #[test]
    fn scans_a_template_literal_with_placeholders() {
        assert_eq!(
            tokens("`a${x}b${y + 1}c`"),
            vec![
                Token::Template { quasis: vec!["a".to_string(), "b".to_string(), "c".to_string()], raw_expressions: vec!["x".to_string(), "y + 1".to_string()] },
                Token::Eof
            ]
        );
    }

    #[test]
    fn template_literal_body_text_resolves_escapes() {
        assert_eq!(tokens("`a\\nb`"), vec![Token::Template { quasis: vec!["a\nb".to_string()], raw_expressions: vec![] }, Token::Eof]);
    }

    #[test]
    fn template_placeholder_errors_propagate_for_unterminated_nested_string_or_template() {
        assert!(Tokenizer::new("`${ \"unterminated").next_spanned().is_err());
        assert!(Tokenizer::new("`${ `unterminated").next_spanned().is_err());
    }

    #[test]
    fn unterminated_template_placeholder_itself_is_an_error() {
        // Runs out of input before the `${`'s own closing `}`, as
        // opposed to the surrounding template's closing backtick.
        assert!(Tokenizer::new("`${ 1 + 2").next_spanned().is_err());
    }

    #[test]
    fn template_placeholder_nested_string_and_template_bodies_handle_escapes() {
        // Exercises `copy_raw_string_body`'s and `copy_raw_template_body`'s
        // own backslash-escape handling (distinct from `scan_escape`,
        // since this is *raw* copying for brace-balancing, not cooking).
        assert_eq!(tokens(r#"`${ "a\"}" }`"#), vec![Token::Template { quasis: vec![String::new(), String::new()], raw_expressions: vec![" \"a\\\"}\" ".to_string()] }, Token::Eof]);
        let toks = tokens("`${ `a\\`b${1}` }`");
        assert_eq!(toks, vec![Token::Template { quasis: vec![String::new(), String::new()], raw_expressions: vec![" `a\\`b${1}` ".to_string()] }, Token::Eof]);
    }

    #[test]
    fn template_placeholder_can_contain_braces_strings_and_nested_templates() {
        assert_eq!(tokens("`${ {} }`"), vec![Token::Template { quasis: vec![String::new(), String::new()], raw_expressions: vec![" {} ".to_string()] }, Token::Eof]);
        assert_eq!(tokens(r#"`${ "}" }`"#), vec![Token::Template { quasis: vec![String::new(), String::new()], raw_expressions: vec![r#" "}" "#.to_string()] }, Token::Eof]);
        let toks = tokens("`${ `${a}` }`");
        assert_eq!(toks, vec![Token::Template { quasis: vec![String::new(), String::new()], raw_expressions: vec![" `${a}` ".to_string()] }, Token::Eof]);
    }

    #[test]
    fn unterminated_template_is_an_error() {
        assert!(Tokenizer::new("`abc").next_spanned().is_err());
        assert!(Tokenizer::new("`abc${x}").next_spanned().is_err());
    }

    #[test]
    fn scans_punctuators_longest_match_first() {
        assert_eq!(
            tokens("=== == = ! != !=="),
            vec![
                Token::Punct(Punct::EqEqEq),
                Token::Punct(Punct::EqEq),
                Token::Punct(Punct::Assign),
                Token::Punct(Punct::Bang),
                Token::Punct(Punct::NotEq),
                Token::Punct(Punct::NotEqEq),
                Token::Eof
            ]
        );
        assert_eq!(tokens("=> ..."), vec![Token::Punct(Punct::Arrow), Token::Punct(Punct::Ellipsis), Token::Eof]);
        assert_eq!(tokens("++ -- + -"), vec![Token::Punct(Punct::PlusPlus), Token::Punct(Punct::MinusMinus), Token::Punct(Punct::Plus), Token::Punct(Punct::Minus), Token::Eof]);
        assert_eq!(tokens("&& || ??"), vec![Token::Punct(Punct::AndAnd), Token::Punct(Punct::OrOr), Token::Punct(Punct::QuestionQuestion), Token::Eof]);
        assert_eq!(tokens("<= >= < >"), vec![Token::Punct(Punct::LtEq), Token::Punct(Punct::GtEq), Token::Punct(Punct::Lt), Token::Punct(Punct::Gt), Token::Eof]);
        assert_eq!(
            tokens("+= -= *= /= %="),
            vec![Token::Punct(Punct::PlusAssign), Token::Punct(Punct::MinusAssign), Token::Punct(Punct::StarAssign), Token::Punct(Punct::SlashAssign), Token::Punct(Punct::PercentAssign), Token::Eof]
        );
        assert_eq!(tokens("a / b % c"), vec![Token::Identifier("a".to_string()), Token::Punct(Punct::Slash), Token::Identifier("b".to_string()), Token::Punct(Punct::Percent), Token::Identifier("c".to_string()), Token::Eof]);
    }

    #[test]
    fn bitwise_and_and_or_are_rejected_as_out_of_scope() {
        assert!(Tokenizer::new("a & b").next_spanned().is_ok()); // `a` itself is fine
        let mut t = Tokenizer::new("&");
        assert!(t.next_spanned().is_err());
        let mut t = Tokenizer::new("|");
        assert!(t.next_spanned().is_err());
    }

    #[test]
    fn skips_line_and_block_comments() {
        assert_eq!(tokens("1 // comment\n2"), vec![Token::Number(1.0), Token::Number(2.0), Token::Eof]);
        assert_eq!(tokens("1 /* block \n comment */ 2"), vec![Token::Number(1.0), Token::Number(2.0), Token::Eof]);
    }

    #[test]
    fn unterminated_block_comment_is_an_error() {
        let mut t = Tokenizer::new("1 /* unterminated");
        assert_eq!(t.next_spanned().unwrap().token, Token::Number(1.0));
        assert!(t.next_spanned().is_err());
    }

    #[test]
    fn tracks_newline_before_a_token_for_asi() {
        let mut t = Tokenizer::new("1\n2");
        assert!(!t.next_spanned().unwrap().newline_before);
        assert!(t.next_spanned().unwrap().newline_before);
    }

    #[test]
    fn unexpected_character_is_an_error_not_a_panic() {
        assert!(Tokenizer::new("@").next_spanned().is_err());
        assert!(Tokenizer::new("#").next_spanned().is_err());
    }

    #[test]
    fn empty_input_yields_immediate_eof() {
        assert_eq!(tokens(""), vec![Token::Eof]);
        assert_eq!(tokens("   \n\t  "), vec![Token::Eof]);
    }
}
