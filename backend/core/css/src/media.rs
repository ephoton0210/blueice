// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded screen and print media evaluation. Unknown features retain unknown truth
//! through negation; an unsupported condition never becomes a matching rule.
use crate::tokenizer::{tokenize, Token};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaEnvironment {
    /// Select print media without changing the live screen environment.
    pub print: bool,
    pub width: f64,
    pub height: f64,
    /// Physical pixels per CSS pixel, including page zoom.
    pub resolution: f64,
    pub dark: bool,
    pub high_contrast: bool,
    pub reduced_motion: bool,
}
impl Default for MediaEnvironment {
    fn default() -> Self {
        Self {
            print: false,
            width: 1024.0,
            height: 640.0,
            resolution: 1.0,
            dark: false,
            high_contrast: false,
            reduced_motion: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Truth {
    Yes,
    No,
    Unknown,
    // Invalid query grammar is distinct from a valid, unsupported feature.
    // An invalid operand invalidates its query even next to a true `or` arm.
    Invalid,
}
impl Truth {
    fn of(value: bool) -> Self {
        if value {
            Self::Yes
        } else {
            Self::No
        }
    }
    fn not(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
            Self::Unknown => Self::Unknown,
            Self::Invalid => Self::Invalid,
        }
    }
    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Invalid, _) | (_, Self::Invalid) => Self::Invalid,
            (Self::No, _) | (_, Self::No) => Self::No,
            (Self::Yes, Self::Yes) => Self::Yes,
            _ => Self::Unknown,
        }
    }
    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Invalid, _) | (_, Self::Invalid) => Self::Invalid,
            (Self::Yes, _) | (_, Self::Yes) => Self::Yes,
            (Self::No, Self::No) => Self::No,
            _ => Self::Unknown,
        }
    }
}
fn ident(token: &Token, name: &str) -> bool {
    matches!(token, Token::Ident(value) if value.eq_ignore_ascii_case(name))
}

pub fn matches_media(input: &str, environment: &MediaEnvironment) -> bool {
    matches_tokens(&tokenize(input), environment)
}
pub(crate) fn matches_tokens(tokens: &[Token], env: &MediaEnvironment) -> bool {
    if ![env.width, env.height, env.resolution]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
    {
        return false;
    }
    let tokens: Vec<_> = tokens
        .iter()
        .filter(|t| !matches!(t, Token::Whitespace | Token::Eof))
        .cloned()
        .collect();
    let mut start = 0;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::LeftParen => depth += 1,
            Token::RightParen => depth = depth.saturating_sub(1),
            Token::Comma if depth == 0 => {
                if query_truth(&tokens[start..index], env) == Truth::Yes {
                    return true;
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    query_truth(&tokens[start..], env) == Truth::Yes
}
fn query_truth(tokens: &[Token], env: &MediaEnvironment) -> Truth {
    let mut tokens = tokens;
    let mut negate = false;
    if tokens.len() > 1
        && (ident(&tokens[0], "only") || ident(&tokens[0], "not"))
        && matches!(&tokens[1], Token::Ident(_))
    {
        if ["not", "only", "and", "or"]
            .iter()
            .any(|word| ident(&tokens[1], word))
        {
            return Truth::Unknown;
        }
        negate = ident(&tokens[0], "not");
        tokens = &tokens[1..];
    }
    if let Some(Token::Ident(kind)) = tokens.first() {
        if !["not", "only", "and", "or"]
            .iter()
            .any(|v| kind.eq_ignore_ascii_case(v))
        {
            let medium = Truth::of(
                kind.eq_ignore_ascii_case("all")
                    || kind.eq_ignore_ascii_case(if env.print { "print" } else { "screen" }),
            );
            let result = if tokens.len() == 1 {
                medium
            } else if ident(&tokens[1], "and") && tokens.len() > 2 && !top_level_or(&tokens[2..]) {
                medium.and(condition(&tokens[2..], env, 0))
            } else {
                Truth::Unknown
            };
            return if negate { result.not() } else { result };
        }
    }
    condition(tokens, env, 0)
}
fn top_level_or(tokens: &[Token]) -> bool {
    let mut depth = 0usize;
    for token in tokens {
        match token {
            Token::LeftParen => depth += 1,
            Token::RightParen => depth = depth.saturating_sub(1),
            _ if depth == 0 && ident(token, "or") => return true,
            _ => {}
        }
    }
    false
}
fn condition(tokens: &[Token], env: &MediaEnvironment, depth: usize) -> Truth {
    if tokens.is_empty() || depth >= 64 {
        return Truth::Invalid;
    }
    let mut level = 0usize;
    let mut operations = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::LeftParen => level += 1,
            Token::RightParen => {
                let Some(next) = level.checked_sub(1) else {
                    return Truth::Invalid;
                };
                level = next;
            }
            Token::Ident(_) if level == 0 && (ident(token, "and") || ident(token, "or")) => {
                operations.push((i, ident(token, "and")))
            }
            _ => {}
        }
    }
    if level != 0 {
        return Truth::Invalid;
    }
    if let Some((_, and)) = operations.first().copied() {
        if operations.iter().any(|(_, value)| *value != and) {
            return Truth::Invalid;
        }
        let mut result = if and { Truth::Yes } else { Truth::No };
        let mut start = 0;
        for end in operations
            .iter()
            .map(|(i, _)| *i)
            .chain(std::iter::once(tokens.len()))
        {
            if tokens.get(start) != Some(&Token::LeftParen) || start == end {
                return Truth::Invalid;
            }
            let next = condition(&tokens[start..end], env, depth + 1);
            result = if and {
                result.and(next)
            } else {
                result.or(next)
            };
            start = end + 1;
        }
        return result;
    }
    if ident(&tokens[0], "not") {
        if tokens.get(1) != Some(&Token::LeftParen) {
            return Truth::Invalid;
        }
        return condition(&tokens[1..], env, depth + 1).not();
    }
    if tokens.first() != Some(&Token::LeftParen) || tokens.last() != Some(&Token::RightParen) {
        return Truth::Invalid;
    }
    // Without a top-level logical operator this must be one complete group,
    // rather than adjacent groups that happen to have balanced parentheses.
    let mut level = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::LeftParen => level += 1,
            Token::RightParen => {
                level -= 1;
                if level == 0 && index + 1 != tokens.len() {
                    return Truth::Invalid;
                }
            }
            _ => {}
        }
    }
    let inner = &tokens[1..tokens.len() - 1];
    if matches!(inner.first(), Some(Token::LeftParen))
        || inner.first().is_some_and(|t| ident(t, "not"))
    {
        condition(inner, env, depth + 1)
    } else {
        feature(inner, env)
    }
}
fn feature(tokens: &[Token], env: &MediaEnvironment) -> Truth {
    if let [Token::Ident(name)] = tokens {
        return match name.to_ascii_lowercase().as_str() {
            "prefers-color-scheme" => Truth::Yes,
            "prefers-contrast" => Truth::of(env.high_contrast),
            "prefers-reduced-motion" => Truth::of(env.reduced_motion),
            "width" | "height" | "resolution" => Truth::Yes,
            _ => Truth::Unknown,
        };
    }
    if let [Token::Ident(name), Token::Colon, value] = tokens {
        let name = name.to_ascii_lowercase();
        if let Token::Ident(value) = value {
            let value = value.to_ascii_lowercase();
            let result = match (name.as_str(), value.as_str()) {
                ("prefers-color-scheme", "dark") => Some(env.dark),
                ("prefers-color-scheme", "light") => Some(!env.dark),
                ("prefers-contrast", "more") => Some(env.high_contrast),
                ("prefers-contrast", "no-preference") => Some(!env.high_contrast),
                ("prefers-contrast", "less" | "custom") => Some(false),
                ("prefers-reduced-motion", "reduce") => Some(env.reduced_motion),
                ("prefers-reduced-motion", "no-preference") => Some(!env.reduced_motion),
                _ => None,
            };
            if let Some(result) = result {
                return Truth::of(result);
            }
        }
        let (prefix, name) = if let Some(n) = name.strip_prefix("min-") {
            (-1, n)
        } else if let Some(n) = name.strip_prefix("max-") {
            (1, n)
        } else {
            (0, name.as_str())
        };
        if let (Some(current), Some(expected)) = (dimension(name, env), numeric(value, name)) {
            return Truth::of(match prefix {
                -1 => current >= expected,
                1 => current <= expected,
                _ => current == expected,
            });
        }
        return Truth::Unknown;
    }
    range(tokens, env)
}
fn dimension(name: &str, env: &MediaEnvironment) -> Option<f64> {
    match name {
        "width" => Some(env.width),
        "height" => Some(env.height),
        "resolution" => Some(env.resolution),
        _ => None,
    }
}
fn numeric(token: &Token, name: &str) -> Option<f64> {
    let value = match token {
        Token::Number(v) if *v == 0.0 => 0.0,
        Token::Dimension(v, unit) => {
            let unit = unit.to_ascii_lowercase();
            let multiplier = if name == "resolution" {
                match unit.as_str() {
                    "dppx" | "x" => 1.0,
                    "dpi" => 1.0 / 96.0,
                    "dpcm" => 2.54 / 96.0,
                    _ => return None,
                }
            } else {
                match unit.as_str() {
                    "px" => 1.0,
                    "em" | "rem" => 16.0,
                    _ => return None,
                }
            };
            *v * multiplier
        }
        _ => return None,
    };
    (value.is_finite() && value >= 0.0).then_some(value)
}
#[derive(Clone, Copy)]
struct Comparison {
    symbol: char,
    equal: bool,
}
impl Comparison {
    fn apply(self, a: f64, b: f64) -> bool {
        match self.symbol {
            '<' => a < b || (self.equal && a == b),
            '>' => a > b || (self.equal && a == b),
            _ => a == b,
        }
    }
}
fn operator(tokens: &[Token]) -> Option<(Comparison, usize)> {
    let Some(Token::Delim(symbol @ ('<' | '>' | '='))) = tokens.first() else {
        return None;
    };
    let equal = *symbol != '=' && tokens.get(1) == Some(&Token::Delim('='));
    Some((
        Comparison {
            symbol: *symbol,
            equal,
        },
        if equal { 2 } else { 1 },
    ))
}
fn range(tokens: &[Token], env: &MediaEnvironment) -> Truth {
    if tokens.len() < 3 {
        return Truth::Unknown;
    }
    let Some((first, consumed)) = operator(&tokens[1..]) else {
        return Truth::Unknown;
    };
    let middle = 1 + consumed;
    let Some(middle_token) = tokens.get(middle) else {
        return Truth::Unknown;
    };
    let evaluate = || -> Option<bool> {
        if tokens.len() == middle + 1 {
            let (name, left_is_dimension) = match (&tokens[0], middle_token) {
                (Token::Ident(name), _) => (name.to_ascii_lowercase(), true),
                (_, Token::Ident(name)) => (name.to_ascii_lowercase(), false),
                _ => return None,
            };
            let current = dimension(&name, env)?;
            let number = numeric(
                if left_is_dimension {
                    middle_token
                } else {
                    &tokens[0]
                },
                &name,
            )?;
            return Some(if left_is_dimension {
                first.apply(current, number)
            } else {
                first.apply(number, current)
            });
        }
        let (second, count) = operator(&tokens[middle + 1..])?;
        if first.symbol == '='
            || second.symbol != first.symbol
            || tokens.len() != middle + count + 2
        {
            return None;
        }
        let Token::Ident(name) = middle_token else {
            return None;
        };
        let name = name.to_ascii_lowercase();
        let current = dimension(&name, env)?;
        Some(
            first.apply(numeric(&tokens[0], &name)?, current)
                && second.apply(current, numeric(tokens.last()?, &name)?),
        )
    };
    evaluate().map_or(Truth::Unknown, Truth::of)
}
