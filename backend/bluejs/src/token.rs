// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS tokenizer: a `&str` -> [`Token`] stream, scoped to exactly
//! `phase-2-mvp-scope/PLAN.md`'s "MVP JS scope (decided)" -- not the
//! full ECMAScript lexical grammar. The edition 17 implementation track
//! now extends that original subset: Number/BigInt radix literals and
//! separators, ECMAScript whitespace/line terminators and string
//! continuations are implemented. Regex literals, `**`, tagged templates
//! and legacy octal escapes remain to be implemented.
//! [`Keyword`] mirrors this: `undefined` is deliberately NOT a keyword
//! here (unlike `null`/`true`/`false`) because it isn't one in real
//! ECMAScript either -- it's an ordinary identifier bound to a global
//! property, so it tokenizes as [`Token::Identifier`] and the parser/
//! interpreter, not the lexer, is where it becomes meaningful.
//!
//! Source characters use Rust `char`; cooked literals use UTF-16 code
//! units so Unicode escapes can preserve lone surrogates losslessly.

use crate::JsString;
use num_bigint::BigInt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    BigInt(BigInt),
    /// A plain (non-template) string literal, already "cooked" --
    /// escape sequences resolved to the characters they represent.
    String(JsString),
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
    Template {
        quasis: Vec<JsString>,
        raw_expressions: Vec<String>,
    },
    Identifier(String),
    Keyword(Keyword),
    Punct(Punct),
    Invalid(String),
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
    Void,
    Delete,
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
            "void" => Keyword::Void,
            "delete" => Keyword::Delete,
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
    StarStar,
    StarStarAssign,
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
    ShiftLeftAssign,
    ShiftRightAssign,
    UnsignedShiftRightAssign,
    AndAssign,
    XorAssign,
    OrAssign,
    EqEq,
    NotEq,
    EqEqEq,
    NotEqEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    ShiftLeft,
    ShiftRight,
    UnsignedShiftRight,
    And,
    Xor,
    Or,
    AndAnd,
    AndAndAssign,
    OrOr,
    OrOrAssign,
    Bang,
    Tilde,
    Question,
    QuestionQuestion,
    QuestionQuestionAssign,
    QuestionDot,
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
        LexError {
            message: message.into(),
        }
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
    /// Whether this IdentifierName used a Unicode escape.  Contextual
    /// keywords such as `await` cannot be escaped when the grammar requires
    /// the keyword spelling, so the parser must retain this lexical fact.
    pub identifier_escaped: bool,
}

pub(crate) type TaggedTemplateData = (Vec<JsString>, Vec<Option<JsString>>, Vec<String>);

pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
    /// Annex B HTML close comments are recognized only at the start of a
    /// physical line (after whitespace).  Keep that lexical state here so
    /// trivia does not need to rescan prior source text.
    line_start: bool,
    identifier_escaped: bool,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

impl Tokenizer {
    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    pub(crate) fn at_template(&mut self, position: usize) -> bool {
        self.pos = position;
        self.skip_trivia().is_ok() && self.peek() == Some('`')
    }

    pub(crate) fn regexp_at(&mut self, position: usize) -> Result<(JsString, JsString), LexError> {
        self.pos = position;
        self.skip_trivia()?;
        self.advance();
        let mut pattern = JsString::default();
        let mut class = false;
        loop {
            let c = self
                .advance()
                .ok_or_else(|| LexError::new("unterminated RegExp literal"))?;
            if is_line_terminator(c) {
                return Err(LexError::new("line terminator in RegExp literal"));
            }
            if c == '/' && !class {
                break;
            }
            pattern.push_code_point(c as u32);
            if c == '\\' {
                let escaped = self
                    .advance()
                    .ok_or_else(|| LexError::new("unterminated RegExp escape"))?;
                if is_line_terminator(escaped) {
                    return Err(LexError::new("line terminator in RegExp escape"));
                }
                pattern.push_code_point(escaped as u32);
            } else if c == '[' {
                class = true;
            } else if c == ']' {
                class = false;
            }
        }
        let mut flags = JsString::default();
        while self.peek().is_some_and(is_ident_continue) {
            flags.push_code_point(self.advance().unwrap() as u32);
        }
        Ok((pattern, flags))
    }

    pub(crate) fn tagged_template_at(
        &mut self,
        position: usize,
    ) -> Result<TaggedTemplateData, LexError> {
        self.pos = position;
        self.skip_trivia()?;
        self.advance();
        let mut raw = Vec::new();
        let mut current = String::new();
        let mut expressions = Vec::new();
        loop {
            let c = self
                .advance()
                .ok_or_else(|| LexError::new("unterminated tagged template"))?;
            if c == '`' {
                raw.push(current);
                break;
            }
            if c == '$' && self.peek() == Some('{') {
                self.advance();
                raw.push(std::mem::take(&mut current));
                expressions.push(self.scan_raw_until_matching_brace()?);
            } else if c == '\\' {
                current.push(c);
                let c = self
                    .advance()
                    .ok_or_else(|| LexError::new("unterminated tagged template escape"))?;
                if c == '\r' {
                    if self.peek() == Some('\n') {
                        self.advance();
                    }
                    current.push('\n');
                } else {
                    current.push(c);
                }
            } else if c == '\r' {
                if self.peek() == Some('\n') {
                    self.advance();
                }
                current.push('\n');
            } else {
                current.push(c);
            }
        }
        let cooked = raw
            .iter()
            .map(|raw| {
                let mut lexer = Tokenizer::new(raw);
                let mut result = JsString::default();
                while let Some(c) = lexer.advance() {
                    if c == '\\' {
                        match lexer.scan_escape() {
                            Ok(Some(c)) => result.push_code_point(c),
                            Ok(None) => {}
                            Err(_) => return None,
                        }
                    } else {
                        result.push_code_point(c as u32);
                    }
                }
                Some(result)
            })
            .collect();
        Ok((
            raw.into_iter().map(Into::into).collect(),
            cooked,
            expressions,
        ))
    }

    pub fn new(input: &str) -> Tokenizer {
        Tokenizer {
            input: input.chars().collect(),
            pos: 0,
            line_start: true,
            identifier_escaped: false,
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
        if let Some(c) = c {
            self.pos += 1;
            if is_line_terminator(c) {
                self.line_start = true;
            } else if !crate::primitive::whitespace(c) {
                self.line_start = false;
            }
        }
        c
    }

    /// Skips whitespace and comments, reporting whether a line
    /// terminator was seen anywhere along the way (for ASI, see
    /// [`SpannedToken`]).
    fn skip_trivia(&mut self) -> Result<bool, LexError> {
        let mut saw_newline = false;
        // Annex B permits a legacy HTML close comment after leading trivia,
        // and directly after a block comment even when a token preceded that
        // comment on the same line.
        let mut html_close_allowed = self.line_start;
        loop {
            match self.peek() {
                Some(c) if is_line_terminator(c) => {
                    saw_newline = true;
                    self.advance();
                    html_close_allowed = true;
                }
                Some(c) if crate::primitive::whitespace(c) => {
                    self.advance();
                }
                // Annex B's legacy HTML comments are lexical comments in
                // Script code. `<!--` has no line-start restriction, while
                // `-->` is recognized only after a line terminator and any
                // following whitespace/comments.
                Some('<')
                    if self.peek_at(1) == Some('!')
                        && self.peek_at(2) == Some('-')
                        && self.peek_at(3) == Some('-') =>
                {
                    for _ in 0..4 {
                        self.advance();
                    }
                    while let Some(c) = self.peek() {
                        if is_line_terminator(c) {
                            break;
                        }
                        self.advance();
                    }
                }
                Some('-')
                    if html_close_allowed
                        && self.peek_at(1) == Some('-')
                        && self.peek_at(2) == Some('>') =>
                {
                    for _ in 0..3 {
                        self.advance();
                    }
                    while let Some(c) = self.peek() {
                        if is_line_terminator(c) {
                            break;
                        }
                        self.advance();
                    }
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if is_line_terminator(c) {
                            break;
                        }
                        self.advance();
                    }
                    html_close_allowed = true;
                }
                Some('/') if self.peek_at(1) == Some('*') => {
                    self.advance();
                    self.advance();
                    loop {
                        match self.peek() {
                            None => return Err(LexError::new("unterminated block comment")),
                            Some(c) if is_line_terminator(c) => {
                                saw_newline = true;
                                self.advance();
                            }
                            Some('*') if self.peek_at(1) == Some('/') => {
                                self.advance();
                                self.advance();
                                html_close_allowed = true;
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
        self.identifier_escaped = false;
        let token = self.next_token()?;
        Ok(SpannedToken {
            token,
            newline_before,
            identifier_escaped: self.identifier_escaped,
        })
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
        if is_ident_start(c) || (c == '\\' && self.peek_at(1) == Some('u')) {
            return self.scan_identifier_or_keyword();
        }
        self.scan_punct()
    }

    fn scan_number(&mut self) -> Result<Token, LexError> {
        let leading_zero = self.peek() == Some('0');
        if leading_zero {
            let bits = match self.peek_at(1) {
                Some('b' | 'B') => Some(1),
                Some('o' | 'O') => Some(3),
                Some('x' | 'X') => Some(4),
                _ => None,
            };
            if let Some(bits) = bits {
                self.advance();
                self.advance();
                let digits = self.scan_digits(1 << bits, true)?;
                if digits.is_empty() {
                    return Err(LexError::new("non-decimal literal requires digits"));
                }
                if self.peek() == Some('n') {
                    self.advance();
                    return self.finish_bigint(&digits, 1 << bits);
                }
                return self.finish_number(crate::primitive::radix_number(&digits, bits));
            }
        }
        let mut text = self.scan_digits(10, !leading_zero)?;
        if self.peek() == Some('n') {
            self.advance();
            if leading_zero && text.len() > 1 {
                return Err(LexError::new(
                    "decimal BigInt literal cannot have a leading zero",
                ));
            }
            return self.finish_bigint(&text, 10);
        }
        // Legacy leading-zero octal literals exist in sloppy scripts.
        // Unlike leading-zero decimals containing 8/9, they have no
        // decimal fraction/exponent production (§12.9.3).
        if leading_zero && text.len() > 1 && text.bytes().all(|b| b <= b'7') {
            return self.finish_number(crate::primitive::radix_number(&text, 3));
        }
        if self.peek() == Some('.') {
            self.advance();
            text.push('.');
            text.push_str(&self.scan_digits(10, true)?);
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            self.advance();
            text.push('e');
            if matches!(self.peek(), Some('+') | Some('-')) {
                text.push(self.advance().unwrap());
            }
            let digits = self.scan_digits(10, true)?;
            if digits.is_empty() {
                return Err(LexError::new("exponent requires digits"));
            }
            text.push_str(&digits);
        }
        // Validated decimal syntax; overflow/underflow become infinity/zero.
        self.finish_number(text.parse().expect("scanner emits valid decimal syntax"))
    }

    fn scan_digits(&mut self, radix: u32, separators: bool) -> Result<String, LexError> {
        let mut digits = String::new();
        loop {
            match self.peek() {
                Some(c) if c.is_digit(radix) => {
                    digits.push(c);
                    self.advance();
                }
                Some('_') => {
                    if !separators
                        || digits.is_empty()
                        || !self.peek_at(1).is_some_and(|c| c.is_digit(radix))
                    {
                        return Err(LexError::new("numeric separator must occur between digits"));
                    }
                    self.advance();
                }
                _ => return Ok(digits),
            }
        }
    }

    fn finish_number(&self, number: f64) -> Result<Token, LexError> {
        if self
            .peek()
            .is_some_and(|c| is_ident_start(c) || c.is_ascii_digit() || c == '\\')
        {
            return Err(LexError::new(
                "identifier or digit immediately after numeric literal",
            ));
        }
        Ok(Token::Number(number))
    }

    fn finish_bigint(&self, digits: &str, radix: u32) -> Result<Token, LexError> {
        if self
            .peek()
            .is_some_and(|c| is_ident_start(c) || c.is_ascii_digit() || c == '\\')
        {
            return Err(LexError::new(
                "identifier or digit immediately after BigInt literal",
            ));
        }
        let value = BigInt::parse_bytes(digits.as_bytes(), radix)
            .expect("scanner validates BigInt literal digits");
        Ok(Token::BigInt(value))
    }

    fn scan_escape(&mut self) -> Result<Option<u32>, LexError> {
        // Caller has already consumed the leading '\\'.
        let c = self
            .advance()
            .ok_or_else(|| LexError::new("unterminated escape sequence"))?;
        Ok(Some(match c {
            'n' => 0x0a,
            't' => 0x09,
            'r' => 0x0d,
            'b' => 0x08,
            'f' => 0x0c,
            'v' => 0x0b,
            '0' => 0,
            c if is_line_terminator(c) => {
                if c == '\r' && self.peek() == Some('\n') {
                    self.advance();
                }
                return Ok(None); // A whole LineTerminatorSequence contributes nothing.
            }
            '\'' | '"' | '`' | '\\' | '$' => c as u32,
            'x' => {
                let hex: String = (0..2)
                    .map(|_| {
                        self.advance()
                            .ok_or_else(|| LexError::new("unterminated \\x escape"))
                    })
                    .collect::<Result<_, _>>()?;
                Self::hex_escape(&hex, "\\x")?
            }
            'u' => {
                if self.peek() == Some('{') {
                    self.advance();
                    let mut hex = String::new();
                    while self.peek().is_some_and(|c| c != '}') {
                        hex.push(self.advance().unwrap());
                    }
                    self.advance()
                        .ok_or_else(|| LexError::new("unterminated \\u{...} escape"))?;
                    let code = Self::hex_escape(&hex, "\\u{...}")?;
                    if code > 0x10ffff {
                        return Err(LexError::new("invalid \\u{...} escape codepoint"));
                    }
                    code
                } else {
                    let hex: String = (0..4)
                        .map(|_| {
                            self.advance()
                                .ok_or_else(|| LexError::new("unterminated \\u escape"))
                        })
                        .collect::<Result<_, _>>()?;
                    Self::hex_escape(&hex, "\\u")?
                }
            }
            other => other as u32, // NonEscapeCharacter, including astral source characters.
        }))
    }

    fn hex_escape(hex: &str, kind: &str) -> Result<u32, LexError> {
        // ECMA-262 (2026) §12.9.4 requires HexDigits, not the optional
        // leading '+' accepted by Rust's from_str_radix.
        if hex.is_empty() || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(LexError::new(format!("invalid {kind} escape '{hex}'")));
        }
        u32::from_str_radix(hex, 16)
            .map_err(|_| LexError::new(format!("invalid {kind} escape '{hex}'")))
    }

    fn scan_string(&mut self, quote: char) -> Result<Token, LexError> {
        self.advance(); // opening quote
        let mut out = JsString::default();
        loop {
            match self.peek() {
                None => return Err(LexError::new("unterminated string literal")),
                Some(c) if c == quote => {
                    self.advance();
                    return Ok(Token::String(out));
                }
                Some('\n' | '\r') => {
                    return Err(LexError::new(
                        "unterminated string literal (line terminator)",
                    ))
                }
                Some('\\') => {
                    self.advance();
                    if let Some(c) = self.scan_escape()? {
                        out.push_code_point(c);
                    }
                }
                Some(c) => {
                    self.advance();
                    out.push_code_point(c as u32);
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
        let mut current = JsString::default();
        loop {
            match self.peek() {
                None => return Err(LexError::new("unterminated template literal")),
                Some('`') => {
                    self.advance();
                    quasis.push(current);
                    return Ok(Token::Template {
                        quasis,
                        raw_expressions,
                    });
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
                        current.push_code_point(c);
                    }
                }
                Some('\r') => {
                    self.advance();
                    if self.peek() == Some('\n') {
                        self.advance();
                    }
                    current.push_code_point('\n' as u32);
                }
                Some(c) => {
                    self.advance();
                    current.push_code_point(c as u32);
                }
            }
        }
    }

    /// Ask the expression parser which closing brace ends this placeholder.
    /// Brace counting cannot distinguish a RegExp body, division, comments,
    /// object literals, or nested templates. Each candidate is a finite prefix
    /// ending in `}`, so recursive template parsing always consumes less source.
    fn scan_raw_until_matching_brace(&mut self) -> Result<String, LexError> {
        let mut out = String::new();
        while let Some(c) = self.advance() {
            out.push(c);
            if c == '}' && crate::parser::closes_template_placeholder(&out) {
                out.pop();
                return Ok(out);
            }
        }
        Err(LexError::new(
            "unterminated or invalid template placeholder",
        ))
    }

    fn scan_identifier_or_keyword(&mut self) -> Result<Token, LexError> {
        let mut text = String::new();
        loop {
            let first = text.is_empty();
            let character = match self.peek() {
                Some('\\') => {
                    self.identifier_escaped = true;
                    self.scan_identifier_escape()?
                }
                Some(character)
                    if if first {
                        is_ident_start(character)
                    } else {
                        is_ident_continue(character)
                    } =>
                {
                    self.advance();
                    character
                }
                _ => break,
            };
            if if first {
                !is_ident_start(character)
            } else {
                !is_ident_continue(character)
            } {
                return Err(LexError::new(
                    "unicode escape does not form a valid identifier character",
                ));
            }
            text.push(character);
        }
        match Keyword::from_str(&text) {
            Some(kw) => Ok(Token::Keyword(kw)),
            None => Ok(Token::Identifier(text)),
        }
    }

    fn scan_identifier_escape(&mut self) -> Result<char, LexError> {
        debug_assert_eq!(self.peek(), Some('\\'));
        self.advance();
        if self.advance() != Some('u') {
            return Err(LexError::new("identifier escape must use \\u"));
        }
        let code = if self.peek() == Some('{') {
            self.advance();
            let mut hex = String::new();
            while self.peek().is_some_and(|character| character != '}') {
                hex.push(self.advance().unwrap());
            }
            self.advance()
                .ok_or_else(|| LexError::new("unterminated \\u{...} identifier escape"))?;
            let code = Self::hex_escape(&hex, "\\u{...}")?;
            if code > 0x10ffff {
                return Err(LexError::new(
                    "invalid \\u{...} identifier escape codepoint",
                ));
            }
            code
        } else {
            let hex: String = (0..4)
                .map(|_| {
                    self.advance()
                        .ok_or_else(|| LexError::new("unterminated \\u identifier escape"))
                })
                .collect::<Result<_, _>>()?;
            Self::hex_escape(&hex, "\\u")?
        };
        char::from_u32(code)
            .ok_or_else(|| LexError::new("identifier escape is not a Unicode scalar value"))
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
                if self.peek() == Some('*') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Punct::StarStarAssign
                    } else {
                        Punct::StarStar
                    }
                } else if self.peek() == Some('=') {
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
            '<' => {
                if self.peek() == Some('<') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Punct::ShiftLeftAssign
                    } else {
                        Punct::ShiftLeft
                    }
                } else {
                    two!('=', Punct::LtEq, Punct::Lt)
                }
            }
            '>' => {
                if self.peek() == Some('>') {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        if self.peek() == Some('=') {
                            self.advance();
                            Punct::UnsignedShiftRightAssign
                        } else {
                            Punct::UnsignedShiftRight
                        }
                    } else if self.peek() == Some('=') {
                        self.advance();
                        Punct::ShiftRightAssign
                    } else {
                        Punct::ShiftRight
                    }
                } else {
                    two!('=', Punct::GtEq, Punct::Gt)
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Punct::AndAndAssign
                    } else {
                        Punct::AndAnd
                    }
                } else if self.peek() == Some('=') {
                    self.advance();
                    Punct::AndAssign
                } else {
                    Punct::And
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Punct::OrOrAssign
                    } else {
                        Punct::OrOr
                    }
                } else if self.peek() == Some('=') {
                    self.advance();
                    Punct::OrAssign
                } else {
                    Punct::Or
                }
            }
            '^' => two!('=', Punct::XorAssign, Punct::Xor),
            '~' => Punct::Tilde,
            '?' => {
                if self.peek() == Some('?') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        Punct::QuestionQuestionAssign
                    } else {
                        Punct::QuestionQuestion
                    }
                } else if self.peek() == Some('.') {
                    self.advance();
                    Punct::QuestionDot
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
        // Assert token boundaries independently of the full parser/VM.
        assert_eq!(
            tokens("0xA_B 0b1_0 0o7_0 07 08 1_0 0.5"),
            vec![
                Token::Number(171.0),
                Token::Number(2.0),
                Token::Number(56.0),
                Token::Number(7.0),
                Token::Number(8.0),
                Token::Number(10.0),
                Token::Number(0.5),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn an_exponent_without_digits_is_a_lexical_error() {
        assert!(Tokenizer::new("1.e").next_spanned().is_err());
        for source in ["0x", "1_", "123abc"] {
            assert!(Tokenizer::new(source).next_spanned().is_err(), "{source}");
        }
    }

    #[test]
    fn template_cooking_normalizes_newlines_but_preserves_placeholder_comments() {
        assert_eq!(
            tokens("`a\r\nb${1/* } */+2}c\rd`"),
            vec![
                Token::Template {
                    quasis: vec!["a\nb".into(), "c\nd".into()],
                    raw_expressions: vec!["1/* } */+2".into()]
                },
                Token::Eof,
            ]
        );
        for newline in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
            assert_eq!(
                tokens(&format!("'a\\{newline}b'")),
                vec![Token::String("ab".into()), Token::Eof]
            );
            let mut tokenizer = Tokenizer::new(&format!("/*{newline}*/x"));
            let spanned = tokenizer.next_spanned().unwrap();
            assert!(spanned.newline_before);
            assert_eq!(spanned.token, Token::Identifier("x".into()));
        }
        assert!(Tokenizer::new("`${1/* unterminated }`")
            .next_spanned()
            .is_err());
    }

    #[test]
    fn scans_every_simple_escape_sequence() {
        assert_eq!(
            tokens(r"'\t\r\b\f\v\0'"),
            vec![Token::String("\t\r\u{8}\u{c}\u{b}\0".into()), Token::Eof]
        );
        assert_eq!(tokens(r"'\q'"), vec![Token::String("q".into()), Token::Eof]);
        // unrecognized escape: falls back to the escaped character itself
    }

    #[test]
    fn a_backslash_newline_is_a_line_continuation_contributing_no_character() {
        assert_eq!(
            tokens("'a\\\nb'"),
            vec![Token::String("ab".into()), Token::Eof]
        );
    }

    #[test]
    fn scans_a_bare_four_hex_digit_unicode_escape_without_braces() {
        assert_eq!(
            tokens("'\\u0041'"),
            vec![Token::String("A".into()), Token::Eof]
        );
    }

    #[test]
    fn scans_string_literals_with_escapes() {
        assert_eq!(
            tokens(r#""hello""#),
            vec![Token::String("hello".into()), Token::Eof]
        );
        assert_eq!(
            tokens("'hello'"),
            vec![Token::String("hello".into()), Token::Eof]
        );
        assert_eq!(
            tokens(r#""a\nb""#),
            vec![Token::String("a\nb".into()), Token::Eof]
        );
        assert_eq!(
            tokens(r#""a\"b""#),
            vec![Token::String("a\"b".into()), Token::Eof]
        );
        assert_eq!(
            tokens(r"'\u{1F600}'"),
            vec![Token::String("\u{1F600}".into()), Token::Eof]
        );
        assert_eq!(tokens(r"'A'"), vec![Token::String("A".into()), Token::Eof]);
        assert_eq!(
            tokens(r"'\x41'"),
            vec![Token::String("A".into()), Token::Eof]
        );
    }

    #[test]
    fn unterminated_string_is_an_error() {
        assert!(Tokenizer::new("\"abc").next_spanned().is_err());
        assert!(Tokenizer::new("\"abc\n\"").next_spanned().is_err());
    }

    #[test]
    fn scans_identifiers_and_keywords() {
        assert_eq!(
            tokens("foo _bar $baz"),
            vec![
                Token::Identifier("foo".to_string()),
                Token::Identifier("_bar".to_string()),
                Token::Identifier("$baz".to_string()),
                Token::Eof
            ]
        );
        assert_eq!(
            tokens(r"\u0065xtends \u{61}sync"),
            vec![
                Token::Identifier("extends".to_string()),
                Token::Identifier("async".to_string()),
                Token::Eof
            ]
        );
        assert!(Tokenizer::new(r"\u0030").next_spanned().is_err());
        assert_eq!(
            tokens("undefined"),
            vec![Token::Identifier("undefined".to_string()), Token::Eof]
        );
        assert_eq!(
            tokens("let x = true"),
            vec![
                Token::Keyword(Keyword::Let),
                Token::Identifier("x".to_string()),
                Token::Punct(Punct::Assign),
                Token::Keyword(Keyword::True),
                Token::Eof
            ]
        );
    }

    #[test]
    fn scans_a_simple_template_literal_with_no_placeholders() {
        assert_eq!(
            tokens("`hello`"),
            vec![
                Token::Template {
                    quasis: vec!["hello".into()],
                    raw_expressions: vec![]
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn scans_a_template_literal_with_placeholders() {
        assert_eq!(
            tokens("`a${x}b${y + 1}c`"),
            vec![
                Token::Template {
                    quasis: vec!["a".into(), "b".into(), "c".into()],
                    raw_expressions: vec!["x".to_string(), "y + 1".to_string()]
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn template_literal_body_text_resolves_escapes() {
        assert_eq!(
            tokens("`a\\nb`"),
            vec![
                Token::Template {
                    quasis: vec!["a\nb".into()],
                    raw_expressions: vec![]
                },
                Token::Eof
            ]
        );
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
        // Exercises nested strings and templates with
        // own backslash-escape handling (distinct from `scan_escape`,
        // since this is *raw* copying for brace-balancing, not cooking).
        assert_eq!(
            tokens(r#"`${ "a\"}" }`"#),
            vec![
                Token::Template {
                    quasis: vec![Default::default(), Default::default()],
                    raw_expressions: vec![" \"a\\\"}\" ".to_string()]
                },
                Token::Eof
            ]
        );
        let toks = tokens("`${ `a\\`b${1}` }`");
        assert_eq!(
            toks,
            vec![
                Token::Template {
                    quasis: vec![Default::default(), Default::default()],
                    raw_expressions: vec![" `a\\`b${1}` ".to_string()]
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn template_placeholder_can_contain_braces_strings_and_nested_templates() {
        assert_eq!(
            tokens("`${ {} }`"),
            vec![
                Token::Template {
                    quasis: vec![Default::default(), Default::default()],
                    raw_expressions: vec![" {} ".to_string()]
                },
                Token::Eof
            ]
        );
        assert_eq!(
            tokens(r#"`${ "}" }`"#),
            vec![
                Token::Template {
                    quasis: vec![Default::default(), Default::default()],
                    raw_expressions: vec![r#" "}" "#.to_string()]
                },
                Token::Eof
            ]
        );
        let toks = tokens("`${ `${a}` }`");
        assert_eq!(
            toks,
            vec![
                Token::Template {
                    quasis: vec![Default::default(), Default::default()],
                    raw_expressions: vec![" `${a}` ".to_string()]
                },
                Token::Eof
            ]
        );
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
        assert_eq!(
            tokens("=> ..."),
            vec![
                Token::Punct(Punct::Arrow),
                Token::Punct(Punct::Ellipsis),
                Token::Eof
            ]
        );
        assert_eq!(
            tokens("++ -- + -"),
            vec![
                Token::Punct(Punct::PlusPlus),
                Token::Punct(Punct::MinusMinus),
                Token::Punct(Punct::Plus),
                Token::Punct(Punct::Minus),
                Token::Eof
            ]
        );
        assert_eq!(
            tokens("&& || ??"),
            vec![
                Token::Punct(Punct::AndAnd),
                Token::Punct(Punct::OrOr),
                Token::Punct(Punct::QuestionQuestion),
                Token::Eof
            ]
        );
        assert_eq!(
            tokens("<= >= < >"),
            vec![
                Token::Punct(Punct::LtEq),
                Token::Punct(Punct::GtEq),
                Token::Punct(Punct::Lt),
                Token::Punct(Punct::Gt),
                Token::Eof
            ]
        );
        assert_eq!(
            tokens("+= -= *= **= /= %="),
            vec![
                Token::Punct(Punct::PlusAssign),
                Token::Punct(Punct::MinusAssign),
                Token::Punct(Punct::StarAssign),
                Token::Punct(Punct::StarStarAssign),
                Token::Punct(Punct::SlashAssign),
                Token::Punct(Punct::PercentAssign),
                Token::Eof
            ]
        );
        assert_eq!(
            tokens("a / b % c"),
            vec![
                Token::Identifier("a".to_string()),
                Token::Punct(Punct::Slash),
                Token::Identifier("b".to_string()),
                Token::Punct(Punct::Percent),
                Token::Identifier("c".to_string()),
                Token::Eof
            ]
        );
    }

    #[test]
    fn scans_bitwise_shift_and_compound_punctuators_longest_first() {
        assert_eq!(
            tokens("& &= | |= ^ ^= ~ << <<= >> >>= >>> >>>="),
            vec![
                Token::Punct(Punct::And),
                Token::Punct(Punct::AndAssign),
                Token::Punct(Punct::Or),
                Token::Punct(Punct::OrAssign),
                Token::Punct(Punct::Xor),
                Token::Punct(Punct::XorAssign),
                Token::Punct(Punct::Tilde),
                Token::Punct(Punct::ShiftLeft),
                Token::Punct(Punct::ShiftLeftAssign),
                Token::Punct(Punct::ShiftRight),
                Token::Punct(Punct::ShiftRightAssign),
                Token::Punct(Punct::UnsignedShiftRight),
                Token::Punct(Punct::UnsignedShiftRightAssign),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn skips_line_and_block_comments() {
        assert_eq!(
            tokens("1 // comment\n2"),
            vec![Token::Number(1.0), Token::Number(2.0), Token::Eof]
        );
        assert_eq!(
            tokens("1 /* block \n comment */ 2"),
            vec![Token::Number(1.0), Token::Number(2.0), Token::Eof]
        );
    }

    #[test]
    fn skips_annex_b_html_comments() {
        assert_eq!(
            tokens("<!-- ignored\n1\n  --> ignored\n2"),
            vec![Token::Number(1.0), Token::Number(2.0), Token::Eof]
        );
        // The close spelling remains ordinary punctuator source away from a
        // line start; accepting it there would change executable code.
        assert_eq!(
            tokens("1 --> 2"),
            vec![
                Token::Number(1.0),
                Token::Punct(Punct::MinusMinus),
                Token::Punct(Punct::Gt),
                Token::Number(2.0),
                Token::Eof,
            ]
        );
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
