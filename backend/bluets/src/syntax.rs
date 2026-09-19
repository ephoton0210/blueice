// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A lossless-enough TypeScript lexical layer shared by the parser and emitter.

use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};

/// Maximum tokens accepted in one source record.  It keeps a hostile source
/// from turning diagnostics or compiler work into an unbounded allocation.
pub const MAX_TOKENS: usize = 1_000_000;

/// Maximum UTF-8 source bytes accepted by the default parser policy.  Token
/// limits alone do not bound whitespace-only input, so both limits are needed.
pub const MAX_SOURCE_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Identifier,
    Keyword,
    Number,
    String,
    Template,
    Punct,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub start: usize,
    pub end: usize,
}

impl Token {
    pub fn is(&self, text: &str) -> bool {
        self.text == text
    }

    pub fn span(&self, module: &str) -> SourceSpan {
        SourceSpan::new(module, self.start, self.end)
    }
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "any",
    "as",
    "asserts",
    "async",
    "await",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "constructor",
    "continue",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "infer",
    "instanceof",
    "interface",
    "is",
    "keyof",
    "let",
    "module",
    "namespace",
    "never",
    "new",
    "null",
    "number",
    "object",
    "of",
    "override",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "satisfies",
    "set",
    "static",
    "string",
    "super",
    "switch",
    "symbol",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "unique",
    "unknown",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

pub fn lex(module: &str, source: &str) -> Result<Vec<Token>, Vec<Diagnostic>> {
    lex_with_limits(module, source, MAX_SOURCE_BYTES, MAX_TOKENS)
}

pub(crate) fn lex_with_limits(
    module: &str,
    source: &str,
    max_source_bytes: usize,
    max_tokens: usize,
) -> Result<Vec<Token>, Vec<Diagnostic>> {
    if source.len() > max_source_bytes {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            SourceSpan::new(module, 0, source.len()),
            format!("source exceeds the {max_source_bytes} byte limit"),
        )]);
    }
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut diagnostics = Vec::new();

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if source[index..].starts_with("//") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' && bytes[index] != b'\r' {
                index += 1;
            }
            continue;
        }
        if source[index..].starts_with("/*") {
            let start = index;
            index += 2;
            while index + 1 < bytes.len() && !source[index..].starts_with("*/") {
                index += 1;
            }
            if index + 1 >= bytes.len() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ParseError,
                    SourceSpan::new(module, start, bytes.len()),
                    "unterminated block comment",
                ));
                break;
            }
            index += 2;
            continue;
        }

        let start = index;
        if is_ident_start(source[index..].chars().next().expect("index is in source")) {
            index += source[index..]
                .chars()
                .next()
                .expect("index is in source")
                .len_utf8();
            while index < bytes.len()
                && is_ident_continue(source[index..].chars().next().expect("index is in source"))
            {
                index += source[index..]
                    .chars()
                    .next()
                    .expect("index is in source")
                    .len_utf8();
            }
            let text = &source[start..index];
            let kind = if KEYWORDS.contains(&text) {
                TokenKind::Keyword
            } else {
                TokenKind::Identifier
            };
            tokens.push(Token {
                kind,
                text: text.to_string(),
                start,
                end: index,
            });
        } else if byte.is_ascii_digit() {
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'.' | b'_'))
            {
                index += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Number,
                text: source[start..index].to_string(),
                start,
                end: index,
            });
        } else if matches!(byte, b'\'' | b'\"') {
            let quote = byte;
            index += 1;
            let mut terminated = false;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    terminated = true;
                    break;
                } else if matches!(bytes[index], b'\n' | b'\r') {
                    break;
                } else {
                    index += 1;
                }
            }
            if !terminated {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ParseError,
                    SourceSpan::new(module, start, index),
                    "unterminated string literal",
                ));
                continue;
            }
            tokens.push(Token {
                kind: TokenKind::String,
                text: source[start..index].to_string(),
                start,
                end: index,
            });
        } else if byte == b'`' {
            // Templates with substitutions remain a single lexical unit here.
            // The initial compiler does not type-check template expressions, but
            // it can faithfully preserve an already-valid JavaScript template.
            index += 1;
            let mut terminated = false;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'`' {
                    index += 1;
                    terminated = true;
                    break;
                } else {
                    index += 1;
                }
            }
            if !terminated {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ParseError,
                    SourceSpan::new(module, start, index),
                    "unterminated template literal",
                ));
                continue;
            }
            tokens.push(Token {
                kind: TokenKind::Template,
                text: source[start..index].to_string(),
                start,
                end: index,
            });
        } else {
            let text = longest_punctuation(&source[index..]);
            index += text.len();
            tokens.push(Token {
                kind: TokenKind::Punct,
                text: text.to_string(),
                start,
                end: index,
            });
        }

        if tokens.len() > max_tokens {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(module, start, index),
                format!("source exceeds the {max_tokens} token limit"),
            ));
            break;
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        text: String::new(),
        start: source.len(),
        end: source.len(),
    });
    if diagnostics.is_empty() {
        Ok(tokens)
    } else {
        Err(diagnostics)
    }
}

pub fn string_contents(token: &Token) -> Option<String> {
    if token.kind != TokenKind::String || token.text.len() < 2 {
        return None;
    }
    let body = &token.text[1..token.text.len() - 1];
    let mut value = String::new();
    let mut escaped = false;
    for character in body.chars() {
        if escaped {
            value.push(match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            value.push(character);
        }
    }
    if escaped {
        return None;
    }
    Some(value)
}

fn is_ident_start(character: char) -> bool {
    character.is_alphabetic() || matches!(character, '_' | '$')
}

fn is_ident_continue(character: char) -> bool {
    is_ident_start(character) || character.is_ascii_digit()
}

fn longest_punctuation(source: &str) -> &str {
    const MULTI: &[&str] = &[
        "===", "!==", ">>>", "...", "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "++",
        "--", "**", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "??=", "&&=",
        "||=",
    ];
    for punctuation in MULTI {
        if source.starts_with(punctuation) {
            return punctuation;
        }
    }
    &source[..source.chars().next().map_or(0, char::len_utf8)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_comments_and_multibyte_source_without_losing_spans() {
        let source = "// note\nconst 名 = 'hi'; /* end */";
        let tokens = lex("memory:///a.ts", source).unwrap();
        assert_eq!(tokens[0].text, "const");
        assert_eq!(tokens[1].text, "名");
        assert_eq!(&source[tokens[1].start..tokens[1].end], "名");
    }

    #[test]
    fn reports_an_unterminated_literal() {
        let diagnostics = lex("memory:///a.ts", "const x = 'no").unwrap_err();
        assert_eq!(diagnostics[0].code, DiagnosticCode::ParseError);
    }

    #[test]
    fn enforces_source_byte_and_token_limits_before_unbounded_work() {
        let bytes =
            lex_with_limits("memory:///a.ts", "const answer = 1;", 4, MAX_TOKENS).unwrap_err();
        assert_eq!(bytes[0].code, DiagnosticCode::ResourceLimit);

        let tokens = lex_with_limits("memory:///a.ts", "const answer = 1;", MAX_SOURCE_BYTES, 1)
            .unwrap_err();
        assert_eq!(tokens[0].code, DiagnosticCode::ResourceLimit);
    }
}
