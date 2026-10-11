// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX syntax (`.tsx`): one element is scanned into a tree of source ranges.
//!
//! The lexer turns a whole outermost element into a single `JsxElement` token, so
//! the statement parser and the checker's token-slice inference see it as an
//! operand. The tree here is built on demand from that token's source text, by
//! the checker (typing), the emitter (lowering by edits to the JSX syntax only,
//! leaving embedded expressions in place for every other pass) and the direct
//! bridge. Nothing is copied: every node is a byte range of the module source.
//!
//! The text rules (whitespace trimming of JSX text, entity decoding) are
//! TypeScript 5.9.3's, from the pinned compiler's JSX transform.

mod entities;

use crate::syntax::Token;
use entities::ENTITIES;

/// A failure to scan an element. `late` is set once the opening tag was complete,
/// which means the text really was meant as JSX; an early failure may instead be
/// a type-argument list the lexer should read as punctuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsxError {
    pub offset: usize,
    pub message: String,
    pub late: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsxName {
    /// As written, without trivia: `div`, `my-element`, `a.b.C`, `svg:rect`.
    pub text: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxValue {
    /// A quoted string, quotes included.
    String {
        start: usize,
        end: usize,
    },
    /// `{expression}`: the offsets of `{`, of the expression's text and of `}`.
    Expression {
        open: usize,
        start: usize,
        end: usize,
        close: usize,
        /// The expression's tokens, with absolute offsets.
        tokens: Vec<Token>,
    },
    Element(Box<JsxElement>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxAttribute {
    Named {
        name: JsxName,
        value: Option<JsxValue>,
        start: usize,
        end: usize,
    },
    /// `{...expression}`.
    Spread {
        open: usize,
        start: usize,
        end: usize,
        close: usize,
        tokens: Vec<Token>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxChild {
    Text {
        start: usize,
        end: usize,
    },
    /// `{expression}`, `{...expression}` or an empty `{}`/`{/* comment */}`.
    Expression {
        open: usize,
        start: usize,
        end: usize,
        close: usize,
        spread: bool,
        tokens: Vec<Token>,
    },
    Element(JsxElement),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsxElement {
    pub start: usize,
    pub end: usize,
    /// `None` for a fragment.
    pub name: Option<JsxName>,
    /// The contents of an explicit `<T, U>` on a value tag, without delimiters.
    pub type_arguments: Option<std::ops::Range<usize>>,
    pub attributes: Vec<JsxAttribute>,
    pub children: Vec<JsxChild>,
    pub self_closing: bool,
    /// One past the `>` that ends the opening tag.
    pub opening_end: usize,
    /// The offset of the `<` of the closing tag (the element's end when it is
    /// self-closing).
    pub closing_start: usize,
}

impl JsxElement {
    pub fn is_fragment(&self) -> bool {
        self.name.is_none()
    }
}

/// How the lexer reads an embedded `{ expression }`: given the offset just after
/// `{` (after `...` for a spread), the offset of the matching `}` and the
/// expression's tokens.
pub type ExpressionLexer<'a> = &'a dyn Fn(usize) -> Result<(usize, Vec<Token>), String>;

struct Scanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
    index: usize,
    expression_end: ExpressionLexer<'a>,
    depth: usize,
}

const MAX_DEPTH: usize = 128;

/// Scans the element that starts at `start` (which holds `<`).
pub fn parse_element(
    source: &str,
    start: usize,
    expression_end: ExpressionLexer<'_>,
) -> Result<JsxElement, JsxError> {
    let mut scanner = Scanner {
        source,
        bytes: source.as_bytes(),
        index: start,
        expression_end,
        depth: 0,
    };
    scanner.element(false)
}

impl Scanner<'_> {
    fn error(&self, message: &str, late: bool) -> JsxError {
        JsxError {
            offset: self.index.min(self.bytes.len()),
            message: message.to_string(),
            late,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    /// Whitespace and comments between the parts of a tag.
    fn trivia(&mut self) {
        loop {
            match self.peek() {
                Some(byte) if byte.is_ascii_whitespace() => self.index += 1,
                Some(b'/') if self.bytes.get(self.index + 1) == Some(&b'/') => {
                    while self.peek().is_some_and(|byte| byte != b'\n') {
                        self.index += 1;
                    }
                }
                Some(b'/') if self.bytes.get(self.index + 1) == Some(&b'*') => {
                    self.index += 2;
                    while self.index < self.bytes.len()
                        && !self.source[self.index..].starts_with("*/")
                    {
                        self.index += 1;
                    }
                    self.index = (self.index + 2).min(self.bytes.len());
                }
                _ => return,
            }
        }
    }

    fn name_char(&self, character: char, first: bool) -> bool {
        character.is_alphabetic()
            || matches!(character, '_' | '$')
            || (!first && (character.is_ascii_digit() || character == '-'))
    }

    /// `a`, `a-b`, `a:b`, `a.b.c`.
    fn name(&mut self, allow_member: bool) -> Option<JsxName> {
        let start = self.index;
        let mut text = String::new();
        let mut first = true;
        loop {
            let character = self.source[self.index..].chars().next()?;
            if self.name_char(character, first) {
                text.push(character);
                self.index += character.len_utf8();
                first = false;
                continue;
            }
            if first {
                return None;
            }
            let separator = match character {
                '.' if allow_member => '.',
                ':' => ':',
                _ => break,
            };
            // The part after a separator starts a name again.
            let after = self.source[self.index + 1..].chars().next();
            if !after.is_some_and(|next| self.name_char(next, true)) {
                break;
            }
            text.push(separator);
            self.index += 1;
            first = true;
        }
        Some(JsxName {
            text,
            start,
            end: self.index,
        })
    }

    /// At `{`: the offsets of `{`, of the text after it, and of `}`, and the
    /// tokens of the expression. A leading `...` is skipped when `spread_ok`.
    fn expression(&mut self) -> Result<(usize, usize, usize, bool, Vec<Token>), JsxError> {
        let open = self.index;
        let mut inner = open + 1;
        let after = &self.source[inner..];
        let leading = after.len() - after.trim_start().len();
        let spread = after[leading..].starts_with("...");
        if spread {
            inner += leading + 3;
        }
        let (close, tokens) = (self.expression_end)(inner).map_err(|message| JsxError {
            offset: inner,
            message,
            late: true,
        })?;
        self.index = close + 1;
        Ok((open, inner, close, spread, tokens))
    }

    fn element(&mut self, nested: bool) -> Result<JsxElement, JsxError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error("JSX nesting is too deep", true));
        }
        let start = self.index;
        debug_assert_eq!(self.peek(), Some(b'<'));
        self.index += 1;
        self.trivia();
        let mut element = JsxElement {
            start,
            end: start,
            name: None,
            type_arguments: None,
            attributes: Vec::new(),
            children: Vec::new(),
            self_closing: false,
            opening_end: start,
            closing_start: start,
        };
        if self.peek() == Some(b'>') {
            // A fragment.
            self.index += 1;
        } else {
            let Some(name) = self.name(true) else {
                return Err(self.error("expected a JSX tag name", false));
            };
            element.name = Some(name);
            self.trivia();
            if self.peek() == Some(b'<') {
                element.type_arguments = Some(self.type_arguments()?);
            }
            self.attributes(&mut element)?;
            match self.peek() {
                Some(b'/') => {
                    self.index += 1;
                    self.trivia();
                    if self.peek() != Some(b'>') {
                        return Err(self.error("expected `>` after `/` in a JSX tag", true));
                    }
                    self.index += 1;
                    element.self_closing = true;
                    element.opening_end = self.index;
                    element.closing_start = self.index;
                    element.end = self.index;
                    self.depth -= 1;
                    return Ok(element);
                }
                Some(b'>') => self.index += 1,
                _ => return Err(self.error("expected `>` or `/>` to end a JSX tag", false)),
            }
        }
        element.opening_end = self.index;
        self.children(&mut element)?;
        // At `</`.
        element.closing_start = self.index;
        self.index += 2;
        self.trivia();
        match (&element.name, self.name(true)) {
            (None, None) if self.peek() == Some(b'>') => {}
            (Some(open), Some(close)) if open.text == close.text => {}
            _ => return Err(self.error("the closing JSX tag does not match the opening tag", true)),
        }
        self.trivia();
        if self.peek() != Some(b'>') {
            return Err(self.error("expected `>` to end the closing JSX tag", true));
        }
        self.index += 1;
        element.end = self.index;
        self.depth -= 1;
        let _ = nested;
        Ok(element)
    }

    fn type_arguments(&mut self) -> Result<std::ops::Range<usize>, JsxError> {
        self.index += 1;
        let start = self.index;
        let mut depth = 1usize;
        while let Some(byte) = self.peek() {
            match byte {
                b'\'' | b'"' | b'`' => {
                    self.index += 1;
                    while let Some(next) = self.peek() {
                        self.index += 1;
                        if next == b'\\' {
                            self.index = (self.index + 1).min(self.bytes.len());
                        } else if next == byte {
                            break;
                        }
                    }
                }
                b'/' if matches!(self.bytes.get(self.index + 1), Some(b'/' | b'*')) => {
                    self.trivia();
                }
                b'<' => {
                    depth += 1;
                    self.index += 1;
                }
                b'>' if self.bytes.get(self.index.wrapping_sub(1)) != Some(&b'=') => {
                    depth -= 1;
                    let end = self.index;
                    self.index += 1;
                    if depth == 0 {
                        return Ok(start..end);
                    }
                }
                _ => self.index += 1,
            }
        }
        Err(self.error("unterminated JSX type arguments", true))
    }

    fn attributes(&mut self, element: &mut JsxElement) -> Result<(), JsxError> {
        loop {
            self.trivia();
            match self.peek() {
                Some(b'/' | b'>') | None => return Ok(()),
                Some(b'{') => {
                    let (open, inner, close, spread, tokens) = self.expression()?;
                    if !spread {
                        return Err(
                            self.error("a JSX attribute spread must be `{...expression}`", true)
                        );
                    }
                    element.attributes.push(JsxAttribute::Spread {
                        open,
                        start: inner,
                        end: close,
                        close,
                        tokens,
                    });
                }
                Some(_) => {
                    let attribute_start = self.index;
                    let Some(name) = self.name(false) else {
                        return Err(self.error("expected a JSX attribute name", false));
                    };
                    self.trivia();
                    let mut value = None;
                    if self.peek() == Some(b'=') {
                        self.index += 1;
                        self.trivia();
                        value = Some(match self.peek() {
                            Some(quote @ (b'"' | b'\'')) => {
                                let value_start = self.index;
                                self.index += 1;
                                while self.peek().is_some_and(|byte| byte != quote) {
                                    self.index += 1;
                                }
                                if self.peek().is_none() {
                                    return Err(self.error("unterminated JSX string", true));
                                }
                                self.index += 1;
                                JsxValue::String {
                                    start: value_start,
                                    end: self.index,
                                }
                            }
                            Some(b'{') => {
                                let (open, inner, close, spread, tokens) = self.expression()?;
                                if spread || is_empty_expression(&self.source[inner..close]) {
                                    return Err(self.error(
                                        "a JSX attribute value must be a non-empty expression",
                                        true,
                                    ));
                                }
                                JsxValue::Expression {
                                    open,
                                    start: inner,
                                    end: close,
                                    close,
                                    tokens,
                                }
                            }
                            Some(b'<') => JsxValue::Element(Box::new(self.element(true)?)),
                            _ => {
                                return Err(self.error(
                                    "a JSX attribute value must be a string, `{expression}` or an element",
                                    true,
                                ))
                            }
                        });
                    }
                    element.attributes.push(JsxAttribute::Named {
                        name,
                        value,
                        start: attribute_start,
                        end: self.index,
                    });
                }
            }
        }
    }

    fn children(&mut self, element: &mut JsxElement) -> Result<(), JsxError> {
        loop {
            let text_start = self.index;
            while let Some(byte) = self.peek() {
                match byte {
                    b'<' | b'{' => break,
                    b'>' => return Err(self.error("unexpected `>` in JSX text; use `{'>'}`", true)),
                    b'}' => return Err(self.error("unexpected `}` in JSX text; use `{'}'}`", true)),
                    _ => self.index += 1,
                }
            }
            if self.index > text_start {
                element.children.push(JsxChild::Text {
                    start: text_start,
                    end: self.index,
                });
            }
            match self.peek() {
                None => return Err(self.error("unterminated JSX element", true)),
                Some(b'{') => {
                    let (open, start, close, spread, tokens) = self.expression()?;
                    element.children.push(JsxChild::Expression {
                        open,
                        start,
                        end: close,
                        close,
                        spread,
                        tokens,
                    });
                }
                Some(b'<') => {
                    if self.bytes.get(self.index + 1) == Some(&b'/') {
                        return Ok(());
                    }
                    element
                        .children
                        .push(JsxChild::Element(self.element(true)?));
                }
                Some(_) => unreachable!(
                    "the text loop stops only at an opening angle or brace, or the end"
                ),
            }
        }
    }
}

impl JsxElement {
    /// The tokens of every embedded expression, in document order (attribute
    /// values and spreads, then children, nested elements included), each paired
    /// with the offset of its text.
    pub fn expressions(&self) -> Vec<(usize, &[Token])> {
        let mut out = Vec::new();
        self.collect_expressions(&mut out);
        out
    }

    fn collect_expressions<'a>(&'a self, out: &mut Vec<(usize, &'a [Token])>) {
        for attribute in &self.attributes {
            match attribute {
                JsxAttribute::Spread { start, tokens, .. } => out.push((*start, tokens)),
                JsxAttribute::Named { value, .. } => match value {
                    Some(JsxValue::Expression { start, tokens, .. }) => out.push((*start, tokens)),
                    Some(JsxValue::Element(element)) => element.collect_expressions(out),
                    _ => {}
                },
            }
        }
        for child in &self.children {
            match child {
                JsxChild::Expression { start, tokens, .. } => {
                    if !tokens.is_empty() {
                        out.push((*start, tokens));
                    }
                }
                JsxChild::Element(element) => element.collect_expressions(out),
                JsxChild::Text { .. } => {}
            }
        }
    }
}

/// The `@jsx*` pragmas in a file's comments, which override the options for that
/// file (TypeScript honors them in any comment before the first token).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pragmas {
    /// `@jsx h` (classic factory).
    pub factory: Option<String>,
    /// `@jsxFrag Fragment`.
    pub fragment: Option<String>,
    /// `@jsxImportSource preact`.
    pub import_source: Option<String>,
    /// `@jsxRuntime classic` or `automatic`.
    pub runtime: Option<String>,
}

impl Pragmas {
    pub fn of(source: &str) -> Self {
        let mut pragmas = Self::default();
        let mut rest = source.trim_start();
        // Only the comments before the first token count.
        loop {
            let comment = if let Some(after) = rest.strip_prefix("/*") {
                let Some(end) = after.find("*/") else { break };
                let body = &after[..end];
                rest = after[end + 2..].trim_start();
                body
            } else if let Some(after) = rest.strip_prefix("//") {
                let end = after.find('\n').unwrap_or(after.len());
                let body = &after[..end];
                rest = after[end..].trim_start();
                body
            } else {
                break;
            };
            for line in comment.lines() {
                let line = line.trim().trim_start_matches('*').trim();
                let Some(tag) = line.strip_prefix('@') else {
                    continue;
                };
                let mut parts = tag.split_whitespace();
                let (Some(name), Some(value)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let value = value.to_string();
                match name {
                    "jsx" => pragmas.factory = Some(value),
                    "jsxFrag" => pragmas.fragment = Some(value),
                    "jsxImportSource" => pragmas.import_source = Some(value),
                    "jsxRuntime" => pragmas.runtime = Some(value),
                    _ => {}
                }
            }
        }
        pragmas
    }
}

/// Whether the text between two braces holds only trivia: `{}` or `{/* c */}`.
pub fn is_empty_expression(text: &str) -> bool {
    let mut rest = text.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("/*") {
            match after.find("*/") {
                Some(end) => rest = after[end + 2..].trim_start(),
                None => return false,
            }
        } else if let Some(after) = rest.strip_prefix("//") {
            rest = after.find('\n').map_or("", |end| after[end..].trim_start());
        } else {
            return rest.is_empty();
        }
    }
}

/// TypeScript's value of a run of JSX text: lines trimmed (the first line's
/// leading and the last line's trailing space are kept), blank lines dropped,
/// the rest joined by one space, entities decoded. `None` when nothing is left.
pub fn text_value(raw: &str) -> Option<String> {
    let mut accumulated: Option<String> = None;
    let mut first_non_whitespace: Option<usize> = Some(0);
    let mut last_non_whitespace: Option<usize> = None;
    let characters: Vec<(usize, char)> = raw.char_indices().collect();
    let add = |accumulated: &mut Option<String>, line: &str| {
        let decoded = decode_entities(line);
        *accumulated = Some(match accumulated.take() {
            None => decoded,
            Some(previous) => format!("{previous} {decoded}"),
        });
    };
    for (index, character) in &characters {
        if matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            if let (Some(first), Some(last)) = (first_non_whitespace, last_non_whitespace) {
                let end = last + raw[last..].chars().next().map_or(1, char::len_utf8);
                add(&mut accumulated, &raw[first..end]);
            }
            first_non_whitespace = None;
        } else if !is_single_line_whitespace(*character) {
            last_non_whitespace = Some(*index);
            if first_non_whitespace.is_none() {
                first_non_whitespace = Some(*index);
            }
        }
    }
    if let Some(first) = first_non_whitespace {
        add(&mut accumulated, &raw[first..]);
    }
    accumulated
}

fn is_single_line_whitespace(character: char) -> bool {
    matches!(
        character,
        ' ' | '\t' | '\u{000b}' | '\u{000c}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200b}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// Whether text is only whitespace that contains a line break: TypeScript drops
/// such a child from the semantic children.
pub fn is_blank_with_newline(raw: &str) -> bool {
    raw.chars().all(char::is_whitespace) && raw.contains(['\n', '\r'])
}

/// `&amp;`, `&#38;` and `&#x26;` decoded; an unknown name is left as written.
pub fn decode_entities(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(ampersand) = rest.find('&') {
        output.push_str(&rest[..ampersand]);
        let after = &rest[ampersand + 1..];
        let decoded = after.find(';').and_then(|semicolon| {
            let body = &after[..semicolon];
            let value = if let Some(hex) =
                body.strip_prefix("#x").or_else(|| body.strip_prefix("#X"))
            {
                (!hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()))
                    .then(|| u32::from_str_radix(hex, 16).ok())
                    .flatten()
            } else if let Some(decimal) = body.strip_prefix('#') {
                (!decimal.is_empty() && decimal.chars().all(|c| c.is_ascii_digit()))
                    .then(|| decimal.parse::<u32>().ok())
                    .flatten()
            } else if !body.is_empty() && body.chars().all(|c| c.is_alphanumeric() || c == '_') {
                ENTITIES
                    .binary_search_by_key(&body, |(name, _)| name)
                    .ok()
                    .map(|index| ENTITIES[index].1)
            } else {
                None
            }?;
            Some((char::from_u32(value).unwrap_or('\u{fffd}'), semicolon + 1))
        });
        match decoded {
            Some((character, consumed)) => {
                output.push(character);
                rest = &after[consumed..];
            }
            None => {
                output.push('&');
                rest = after;
            }
        }
    }
    output.push_str(rest);
    output
}

/// Whether a tag name is an intrinsic (a host element) rather than a value:
/// TypeScript's `isIntrinsicJsxName`, a lower-case first letter or a `-`, and a
/// namespaced `a:b` name.
pub fn is_intrinsic_name(name: &str) -> bool {
    if name.contains('.') {
        return false;
    }
    name.contains(':')
        || name.contains('-')
        || name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
}

#[cfg(test)]
mod tests;
