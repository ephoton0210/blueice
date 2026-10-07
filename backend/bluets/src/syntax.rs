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
    /// A whole outermost JSX element of a `.tsx` module, as one operand; its
    /// structure is scanned on demand by [`crate::jsx`].
    JsxElement,
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
    let (mut tokens, _) = lex_range(module, source, 0, false, max_tokens)?;
    tokens.push(Token {
        kind: TokenKind::Eof,
        text: String::new(),
        start: source.len(),
        end: source.len(),
    });
    Ok(tokens)
}

/// The tokens of an expression embedded in JSX braces, whose text starts at
/// `start`, and the offset of the `}` that closes it.
pub(crate) fn lex_expression(
    module: &str,
    source: &str,
    start: usize,
) -> Result<(usize, Vec<Token>), String> {
    lex_range(module, source, start, true, MAX_TOKENS)
        .map(|(tokens, end)| (end, tokens))
        .map_err(|diagnostics| {
            diagnostics
                .first()
                .map_or_else(|| "invalid expression".to_string(), |d| d.message.clone())
        })
}

/// Scans the JSX element at `start` of a `.tsx` module's source.
pub fn parse_jsx(
    module: &str,
    source: &str,
    start: usize,
) -> Result<crate::jsx::JsxElement, crate::jsx::JsxError> {
    let lexer = |from: usize| lex_expression(module, source, from);
    crate::jsx::parse_element(source, start, &lexer)
}

/// Whether a `<` after this token can begin a JSX element rather than a
/// comparison or a type-argument list. `Some(true)` is certain; `Some(false)` is
/// possible (a failed scan then reads `<` as punctuation).
fn jsx_may_start(previous: Option<&Token>) -> Option<bool> {
    let Some(previous) = previous else {
        return Some(false);
    };
    match previous.kind {
        TokenKind::Punct => match previous.text.as_str() {
            "(" | "," | "=" | "=>" | "?" | "[" | "&&" | "||" | "??" | "!" | "..." | "+" | "-"
            | "*" | "/" | "%" | "==" | "!=" | "===" | "!==" | "<=" | ">=" | "&" | "|" | "^"
            | "~" | "+=" | "-=" | "*=" | "/=" | "%=" | "&&=" | "||=" | "??=" => Some(true),
            ":" | ";" | "{" | "}" => Some(false),
            _ => None,
        },
        TokenKind::Keyword => match previous.text.as_str() {
            "return" | "yield" | "await" | "typeof" | "void" | "throw" | "case" | "else" | "in"
            | "default" | "of" | "delete" => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// TypeScript reads `<T,>` and `<T extends U>` in a `.tsx` file as type
/// parameters of a generic arrow function, never as an element.
fn starts_generic_arrow(source: &str, index: usize) -> bool {
    let rest = source[index + 1..].trim_start();
    let rest = rest.strip_prefix("const ").map_or(rest, str::trim_start);
    let name_length = rest
        .char_indices()
        .find(|(_, character)| !is_ident_continue(*character))
        .map_or(rest.len(), |(offset, _)| offset);
    if name_length == 0 {
        return false;
    }
    let after = rest[name_length..].trim_start();
    after.starts_with(',')
        || after
            .strip_prefix("extends")
            .is_some_and(|more| more.starts_with(|c: char| c.is_whitespace()))
}

/// The token loop. With `until_brace` it stops (without consuming) at the first
/// `}` that is not matched by a `{` inside, and returns its offset.
fn lex_range(
    module: &str,
    source: &str,
    start_at: usize,
    until_brace: bool,
    max_tokens: usize,
) -> Result<(Vec<Token>, usize), Vec<Diagnostic>> {
    let tsx = module.ends_with(".tsx");
    let bytes = source.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut index = start_at;
    let mut diagnostics = Vec::new();
    let mut brace_depth = 0usize;
    let mut closing_brace = None;

    while index < bytes.len() {
        let byte = bytes[index];
        if until_brace && byte == b'}' && brace_depth == 0 {
            closing_brace = Some(index);
            break;
        }
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
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
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
            // The checker treats the complete literal as a string; direct
            // BlueTS-to-BlueJS lowering tokenizes a supported substitution only
            // when it needs to construct the corresponding expression AST.
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
        } else if tsx
            && byte == b'<'
            && !source[index..].starts_with("<=")
            && !source[index..].starts_with("<<")
            && !starts_generic_arrow(source, index)
            && jsx_may_start(tokens.last()).is_some()
        {
            let certain = jsx_may_start(tokens.last()) == Some(true);
            let lexer = |from: usize| lex_expression(module, source, from);
            match crate::jsx::parse_element(source, index, &lexer) {
                Ok(element) => {
                    index = element.end;
                    tokens.push(Token {
                        kind: TokenKind::JsxElement,
                        text: source[start..index].to_string(),
                        start,
                        end: index,
                    });
                    // The embedded expressions follow as the arguments of the
                    // element, so every pass that walks tokens (arrow functions,
                    // erased annotations, name rewriting) reaches them.
                    let synthetic = |text: &str, at: usize| Token {
                        kind: TokenKind::Punct,
                        text: text.to_string(),
                        start: at,
                        end: at,
                    };
                    tokens.push(synthetic("(", start));
                    for (position, (offset, expression)) in
                        element.expressions().into_iter().enumerate()
                    {
                        if position > 0 {
                            tokens.push(synthetic(",", offset));
                        }
                        tokens.extend(expression.iter().cloned());
                    }
                    tokens.push(synthetic(")", index));
                }
                Err(error) if error.late || certain => {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::ParseError,
                        SourceSpan::new(module, error.offset, error.offset.max(start + 1)),
                        error.message,
                    ));
                    break;
                }
                Err(_) => {
                    index += 1;
                    tokens.push(Token {
                        kind: TokenKind::Punct,
                        text: "<".to_string(),
                        start,
                        end: index,
                    });
                }
            }
        } else {
            let text = longest_punctuation(&source[index..]);
            index += text.len();
            if text == "{" {
                brace_depth += 1;
            } else if text == "}" {
                brace_depth = brace_depth.saturating_sub(1);
            }
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

    if until_brace && closing_brace.is_none() && diagnostics.is_empty() {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::ParseError,
            SourceSpan::new(module, start_at, source.len()),
            "unterminated JSX expression",
        ));
    }
    if diagnostics.is_empty() {
        Ok((tokens, closing_brace.unwrap_or(index)))
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
        "===", "!==", ">>>=", "**=", "...", "<<=", ">>=", ">>>", "=>", "==", "!=", "<=", ">=",
        "&&=", "||=", "??=", "&&", "||", "??", "?.", "++", "--", "**", "<<", ">>", "+=", "-=",
        "*=", "/=", "%=", "&=", "|=", "^=",
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
    fn scans_multibyte_block_comments_without_slicing_inside_utf8() {
        let source = "/* 🚀 comment */ const answer = 42;";
        let tokens = lex("memory:///a.ts", source).unwrap();
        assert_eq!(tokens[0].text, "const");
        assert_eq!(&source[tokens[0].start..tokens[0].end], "const");

        let diagnostics = lex("memory:///a.ts", "/* 🚀 unterminated").unwrap_err();
        assert_eq!(diagnostics[0].code, DiagnosticCode::ParseError);
    }

    #[test]
    fn reports_an_unterminated_literal() {
        let diagnostics = lex("memory:///a.ts", "const x = 'no").unwrap_err();
        assert_eq!(diagnostics[0].code, DiagnosticCode::ParseError);
    }

    #[test]
    fn lexes_compound_assignments_longest_first() {
        let tokens = lex(
            "memory:///a.ts",
            "value **= 2; value <<= 1; value >>= 1; value >>>= 0; value &= 3; value ^= 1; value |= 2; value &&= 4; value ||= 5; value ??= 6;",
        )
        .unwrap();
        let punctuators = tokens
            .iter()
            .filter(|token| token.kind == TokenKind::Punct && token.text.ends_with('='))
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            punctuators,
            ["**=", "<<=", ">>=", ">>>=", "&=", "^=", "|=", "&&=", "||=", "??="]
        );
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
