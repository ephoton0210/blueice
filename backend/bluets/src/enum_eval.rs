// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Enum member values.
//!
//! An enum member's value is fixed at compile time when its initializer is a
//! constant expression, as TypeScript defines it: numeric and string literals,
//! `+ - ~`, the binary arithmetic, bitwise and shift operators (with
//! JavaScript's `ToInt32` semantics), `+` on strings, parentheses, and
//! references to earlier members of the same enum, alone or as `E.A` /
//! `E["A"]`. A member with no initializer takes the previous numeric value plus
//! one. Anything else is a computed member, evaluated at run time.
//!
//! The checker and the emitter both call [`evaluate_enums`], so the value a
//! member is typed with is the value the emitted object holds.

use std::collections::BTreeMap;

use crate::diagnostic::{DiagnosticCode, SourceSpan};
use crate::parser::{Declaration, EnumDeclaration, Module};
use crate::syntax::{Token, TokenKind};

/// The value of a constant member.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EnumValue {
    Number(f64),
    Text(String),
}

/// One evaluated member: its constant value, or `None` when it is computed.
#[derive(Debug, Clone)]
pub(crate) struct EvaluatedMember {
    pub(crate) name: String,
    pub(crate) value: Option<EnumValue>,
}

/// One enum declaration, evaluated.
#[derive(Debug, Clone)]
pub(crate) struct EvaluatedEnum {
    pub(crate) members: Vec<EvaluatedMember>,
    pub(crate) errors: Vec<(SourceSpan, String, DiagnosticCode)>,
}

/// Evaluates every enum declaration of `module`, in source order, so a later
/// declaration of the same enum sees the earlier one's members.
pub(crate) fn evaluate_enums(module: &Module) -> Vec<EvaluatedEnum> {
    // Each enum's members so far, by name.
    let mut known: BTreeMap<String, Vec<EvaluatedMember>> = BTreeMap::new();
    let mut result = Vec::new();
    for declaration in &module.declarations {
        let Declaration::Enum(enum_declaration) = declaration else {
            continue;
        };
        let earlier = known
            .get(&enum_declaration.name)
            .cloned()
            .unwrap_or_default();
        let merges_earlier = known.contains_key(&enum_declaration.name);
        let evaluated = evaluate_declaration(enum_declaration, &earlier, merges_earlier, &known);
        known
            .entry(enum_declaration.name.clone())
            .or_default()
            .extend(evaluated.members.iter().cloned());
        result.push(evaluated);
    }
    result
}

fn evaluate_declaration(
    declaration: &EnumDeclaration,
    earlier: &[EvaluatedMember],
    merges_earlier: bool,
    all: &BTreeMap<String, Vec<EvaluatedMember>>,
) -> EvaluatedEnum {
    let mut errors = Vec::new();
    let mut members: Vec<EvaluatedMember> = Vec::new();
    // The value the next member takes if it has no initializer; `None` after a
    // member whose value is computed or text.
    let mut next: Option<f64> = Some(0.0);
    for (index, member) in declaration.members.iter().enumerate() {
        let seen = || earlier.iter().chain(members.iter());
        if seen().any(|other| other.name == member.name) {
            errors.push((
                member.name_span.clone(),
                format!("duplicate identifier `{}`", member.name),
                DiagnosticCode::DuplicateDeclaration,
            ));
        }
        let value = match &member.initializer {
            Some(tokens) => {
                let later: Vec<&str> = declaration.members[index + 1..]
                    .iter()
                    .map(|member| member.name.as_str())
                    .collect();
                let mut context = Context {
                    enum_name: &declaration.name,
                    own: seen().cloned().collect(),
                    later,
                    all,
                    errors: &mut errors,
                    span: member.span.clone(),
                };
                evaluate_tokens(tokens, &mut context)
            }
            // In an ambient (non-const) enum a member with no initializer is
            // computed, not numbered.
            None if declaration.declared && !declaration.is_const => None,
            None => match next {
                Some(number) => {
                    if index == 0 && merges_earlier {
                        errors.push((
                            member.name_span.clone(),
                            "in an enum with several declarations, only one may omit the \
                             initializer of its first member"
                                .to_string(),
                            DiagnosticCode::TypeMismatch,
                        ));
                    }
                    Some(EnumValue::Number(number))
                }
                None => {
                    errors.push((
                        member.name_span.clone(),
                        format!("enum member `{}` must have an initializer", member.name),
                        DiagnosticCode::TypeMismatch,
                    ));
                    None
                }
            },
        };
        next = match &value {
            Some(EnumValue::Number(number)) => Some(number + 1.0),
            Some(EnumValue::Text(_)) | None => None,
        };
        members.push(EvaluatedMember {
            name: member.name.clone(),
            value,
        });
    }
    EvaluatedEnum { members, errors }
}

struct Context<'a> {
    enum_name: &'a str,
    /// The members of this enum defined before the one being evaluated.
    own: Vec<EvaluatedMember>,
    /// The names of the members declared after it in this declaration.
    later: Vec<&'a str>,
    all: &'a BTreeMap<String, Vec<EvaluatedMember>>,
    errors: &'a mut Vec<(SourceSpan, String, DiagnosticCode)>,
    span: SourceSpan,
}

/// The constant value of `tokens`, or `None` when the expression is computed.
fn evaluate_tokens(tokens: &[Token], context: &mut Context<'_>) -> Option<EnumValue> {
    let mut parser = Evaluator {
        tokens,
        index: 0,
        context,
        constant: true,
    };
    let value = parser.expression(0);
    if parser.index != tokens.len() || !parser.constant {
        return None;
    }
    value
}

struct Evaluator<'a, 'b> {
    tokens: &'a [Token],
    index: usize,
    context: &'a mut Context<'b>,
    /// Cleared as soon as anything non-constant is met.
    constant: bool,
}

/// Binary operator precedence, tightest last, as JavaScript ranks them.
fn precedence(operator: &str) -> Option<(u8, bool)> {
    Some(match operator {
        "|" => (1, false),
        "^" => (2, false),
        "&" => (3, false),
        "<<" | ">>" | ">>>" => (4, false),
        "+" | "-" => (5, false),
        "*" | "/" | "%" => (6, false),
        "**" => (7, true),
        _ => return None,
    })
}

impl Evaluator<'_, '_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index)
    }

    /// The binary operator at the cursor and how many tokens it spans. The
    /// parser presents `>>` and `>>>` as separate `>` tokens, one byte apart.
    fn operator(&self) -> Option<(String, usize)> {
        let token = self.peek()?;
        if token.text == ">" {
            let mut width = 1;
            let mut end = token.end;
            while width < 3 {
                match self.tokens.get(self.index + width) {
                    Some(next) if next.text == ">" && next.start == end => {
                        width += 1;
                        end = next.end;
                    }
                    _ => break,
                }
            }
            return match width {
                2 => Some((">>".to_string(), 2)),
                3 => Some((">>>".to_string(), 3)),
                _ => None,
            };
        }
        Some((token.text.clone(), 1))
    }

    fn expression(&mut self, minimum: u8) -> Option<EnumValue> {
        let mut left = self.unary()?;
        while self.peek().is_some() {
            let Some((operator, width)) = self.operator() else {
                break;
            };
            let Some((level, right_associative)) = precedence(&operator) else {
                break;
            };
            if level < minimum {
                break;
            }
            self.index += width;
            let right = self.expression(if right_associative { level } else { level + 1 })?;
            left = apply_binary(&operator, left, right)?;
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<EnumValue> {
        let token = self.peek()?.clone();
        match token.text.as_str() {
            "+" | "-" | "~" if token.kind == TokenKind::Punct => {
                self.index += 1;
                let EnumValue::Number(number) = self.unary()? else {
                    self.constant = false;
                    return None;
                };
                Some(EnumValue::Number(match token.text.as_str() {
                    "+" => number,
                    "-" => -number,
                    _ => f64::from(!to_int32(number)),
                }))
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Option<EnumValue> {
        let mut token = self.peek()?.clone();
        self.index += 1;
        // `.5` and `5.` arrive as a number and a `.` one byte apart.
        if token.text == "." {
            if let Some(next) = self
                .peek()
                .filter(|next| next.kind == TokenKind::Number && next.start == token.end)
            {
                token = Token {
                    kind: TokenKind::Number,
                    text: format!(".{}", next.text),
                    start: token.start,
                    end: next.end,
                };
                self.index += 1;
            }
        } else if token.kind == TokenKind::Number {
            if let Some(dot) = self.peek().filter(|dot| {
                dot.text == "."
                    && dot.start == token.end
                    && self
                        .tokens
                        .get(self.index + 1)
                        .is_none_or(|after| after.start != dot.end)
            }) {
                token = Token {
                    kind: TokenKind::Number,
                    text: format!("{}.", token.text),
                    start: token.start,
                    end: dot.end,
                };
                self.index += 1;
            }
        }
        match token.kind {
            TokenKind::Number => match parse_number(&token.text) {
                Some(number) => Some(EnumValue::Number(number)),
                None => {
                    self.constant = false;
                    None
                }
            },
            TokenKind::String => match decode_plain_string(&token.text) {
                Some(text) => Some(EnumValue::Text(text)),
                None => {
                    self.constant = false;
                    None
                }
            },
            TokenKind::Template => {
                // A template literal with no substitution.
                let inner = token
                    .text
                    .strip_prefix('`')
                    .and_then(|text| text.strip_suffix('`'))
                    .filter(|text| !text.contains("${") && !text.contains('\\'));
                match inner {
                    Some(text) => Some(EnumValue::Text(text.to_string())),
                    None => {
                        self.constant = false;
                        None
                    }
                }
            }
            TokenKind::Punct if token.text == "(" => {
                let value = self.expression(0);
                if self.peek().is_some_and(|close| close.text == ")") {
                    self.index += 1;
                } else {
                    self.constant = false;
                }
                value
            }
            TokenKind::Identifier | TokenKind::Keyword => self.reference(&token),
            _ => {
                self.constant = false;
                None
            }
        }
    }

    /// A member of this enum (`A`, `E.A`, `E["A"]`) or of an earlier enum of
    /// the module, or `Infinity` / `NaN`.
    fn reference(&mut self, first: &Token) -> Option<EnumValue> {
        // `Enum.member` / `Enum["member"]`
        if let Some(dot) = self
            .peek()
            .filter(|token| token.text == "." || token.text == "[")
        {
            let bracket = dot.text == "[";
            let Some(name) = self.tokens.get(self.index + 1) else {
                self.constant = false;
                return None;
            };
            let member = if bracket {
                (name.kind == TokenKind::String)
                    .then(|| decode_plain_string(&name.text))
                    .flatten()
            } else {
                Some(name.text.clone())
            };
            let advance = if bracket { 3 } else { 2 };
            let known = if first.text == self.context.enum_name {
                Some(self.context.own.clone())
            } else {
                self.context.all.get(&first.text).cloned()
            };
            if let (Some(member), Some(members)) = (member, known) {
                if bracket
                    && self
                        .tokens
                        .get(self.index + 2)
                        .is_none_or(|close| close.text != "]")
                {
                    self.constant = false;
                    return None;
                }
                self.index += advance;
                return match members.iter().find(|candidate| candidate.name == member) {
                    Some(EvaluatedMember {
                        value: Some(value), ..
                    }) => Some(value.clone()),
                    _ => {
                        self.constant = false;
                        None
                    }
                };
            }
            self.constant = false;
            return None;
        }
        match first.text.as_str() {
            "Infinity" => return Some(EnumValue::Number(f64::INFINITY)),
            "NaN" => return Some(EnumValue::Number(f64::NAN)),
            _ => {}
        }
        if let Some(member) = self
            .context
            .own
            .iter()
            .find(|member| member.name == first.text)
        {
            return match &member.value {
                Some(value) => Some(value.clone()),
                None => {
                    self.constant = false;
                    None
                }
            };
        }
        if self.context.later.contains(&first.text.as_str()) {
            self.context.errors.push((
                SourceSpan::new(&self.context.span.module, first.start, first.end),
                format!(
                    "enum member `{}` is used before its initialization",
                    first.text
                ),
                DiagnosticCode::TypeMismatch,
            ));
        }
        self.constant = false;
        None
    }
}

fn apply_binary(operator: &str, left: EnumValue, right: EnumValue) -> Option<EnumValue> {
    use EnumValue::{Number, Text};
    Some(match (operator, left, right) {
        ("+", Text(a), Text(b)) => Text(a + &b),
        ("+", Text(a), Number(b)) => Text(format!("{a}{}", js_number_text(b))),
        ("+", Number(a), Text(b)) => Text(format!("{}{b}", js_number_text(a))),
        (_, Number(a), Number(b)) => Number(match operator {
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => a / b,
            "%" => a % b,
            "**" => js_power(a, b),
            "|" => f64::from(to_int32(a) | to_int32(b)),
            "&" => f64::from(to_int32(a) & to_int32(b)),
            "^" => f64::from(to_int32(a) ^ to_int32(b)),
            "<<" => f64::from(to_int32(a).wrapping_shl(to_uint32(b) & 31)),
            ">>" => f64::from(to_int32(a) >> (to_uint32(b) & 31)),
            ">>>" => f64::from(to_uint32(a) >> (to_uint32(b) & 31)),
            _ => return None,
        }),
        _ => return None,
    })
}

/// `ToInt32`: the number modulo 2^32, as a signed 32-bit integer.
fn to_int32(value: f64) -> i32 {
    to_uint32(value) as i32
}

fn to_uint32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    (value.trunc().rem_euclid(4_294_967_296.0)) as u32
}

/// `**`, with the cases where JavaScript and `powf` disagree.
fn js_power(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() || (base.abs() == 1.0 && exponent.is_infinite()) {
        return f64::NAN;
    }
    base.powf(exponent)
}

/// A JavaScript numeric literal (decimal with separators and an exponent, or
/// `0x`, `0o`, `0b`).
pub(crate) fn parse_number(text: &str) -> Option<f64> {
    let cleaned: String = text.chars().filter(|character| *character != '_').collect();
    let lower = cleaned.to_ascii_lowercase();
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = lower.strip_prefix(prefix) {
            return u128::from_str_radix(digits, radix)
                .ok()
                .map(|value| value as f64);
        }
    }
    if lower.ends_with('n') {
        return None;
    }
    lower.parse::<f64>().ok()
}

/// The value of a string literal, with its escape sequences applied, or `None`
/// for a malformed one.
pub(crate) fn decode_plain_string(text: &str) -> Option<String> {
    let inner = text.get(1..text.len().checked_sub(1)?)?;
    let mut output = String::new();
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next()? {
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            'b' => output.push('\u{8}'),
            'f' => output.push('\u{c}'),
            'v' => output.push('\u{b}'),
            '0' => output.push('\0'),
            'x' => {
                let digits: String = (&mut characters).take(2).collect();
                output.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
            }
            'u' => {
                let mut rest = characters.clone();
                let code = if rest.next()? == '{' {
                    let digits: String = (&mut rest).take_while(|digit| *digit != '}').collect();
                    characters = rest;
                    u32::from_str_radix(&digits, 16).ok()?
                } else {
                    let digits: String = (&mut characters).take(4).collect();
                    u32::from_str_radix(&digits, 16).ok()?
                };
                // A lone surrogate has no `char`; leave such a string to the
                // run-time path.
                output.push(char::from_u32(code)?);
            }
            '\n' => {}
            other => output.push(other),
        }
    }
    Some(output)
}

/// A number as JavaScript's `Number.prototype.toString` prints it.
pub(crate) fn js_number_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    if !(1e-6..1e21).contains(&magnitude) {
        let text = format!("{value:e}");
        return match text.split_once('e') {
            Some((mantissa, exponent)) if !exponent.starts_with('-') => {
                format!("{mantissa}e+{exponent}")
            }
            _ => text,
        };
    }
    format!("{value}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_like_javascript() {
        for (value, expected) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.5, "0.5"),
            (1026.0, "1026"),
            (5.5, "5.5"),
            (1e21, "1e+21"),
            (1.5e22, "1.5e+22"),
            (1e-7, "1e-7"),
            (123456789012345680000.0, "123456789012345680000"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
        ] {
            assert_eq!(js_number_text(value), expected, "{value}");
        }
    }

    #[test]
    fn literals_parse_like_javascript() {
        for (text, expected) in [
            ("10", 10.0),
            ("0x10", 16.0),
            ("0b11", 3.0),
            ("0o7", 7.0),
            ("1_000", 1000.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("1e3", 1000.0),
        ] {
            assert_eq!(parse_number(text), Some(expected), "{text}");
        }
        assert_eq!(parse_number("10n"), None);
    }

    #[test]
    fn string_escapes_decode_like_javascript() {
        for (text, expected) in [
            ("'a\\nb'", "a\nb"),
            ("\"q\\\"q\"", "q\"q"),
            ("'\\x41\\u0042\\u{43}'", "ABC"),
            ("'\\u{1F600}'", "\u{1F600}"),
            ("'a\\\\b'", "a\\b"),
            ("'\\0'", "\0"),
        ] {
            assert_eq!(
                decode_plain_string(text).as_deref(),
                Some(expected),
                "{text}"
            );
        }
        assert_eq!(
            decode_plain_string("'\\ud800'"),
            None,
            "a lone surrogate is not decoded"
        );
    }

    #[test]
    fn integer_conversions_wrap_like_javascript() {
        assert_eq!(to_int32(4_294_967_296.0), 0);
        assert_eq!(to_int32(2_147_483_648.0), -2_147_483_648);
        assert_eq!(to_int32(-1.5), -1);
        assert_eq!(to_uint32(-1.0), 4_294_967_295);
        assert_eq!(to_int32(f64::NAN), 0);
        assert!(js_power(1.0, f64::INFINITY).is_nan());
        assert_eq!(js_power(2.0, -1.0), 0.5);
    }
}
