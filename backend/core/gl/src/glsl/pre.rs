// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The GLSL ES preprocessor: comments, line continuations, `#define` (object-like
//! and function-like macros), `#undef`, conditional compilation with constant
//! expressions, and the directives that only record or are ignored
//! (`#version`, `#extension`, `#pragma`, `#line`, `#error` fails).

use std::collections::HashMap;

#[derive(Debug, Clone)]
struct Macro {
    /// `Some` for a function-like macro.
    params: Option<Vec<String>>,
    body: String,
}

/// What preprocessing learned besides the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preprocessed {
    pub text: String,
    /// 100 (GLSL ES 1.00) unless `#version 300 es` said otherwise.
    pub version: u32,
}

const MAX_EXPANSION_DEPTH: usize = 32;
const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

pub fn preprocess(source: &str) -> Result<Preprocessed, String> {
    let cleaned = strip_comments_and_continuations(source);
    let mut macros: HashMap<String, Macro> = HashMap::new();
    for (name, body) in [
        ("GL_ES", "1"),
        ("GL_FRAGMENT_PRECISION_HIGH", "1"),
        ("__LINE__", "0"),
        ("__FILE__", "0"),
    ] {
        macros.insert(
            name.to_string(),
            Macro {
                params: None,
                body: body.to_string(),
            },
        );
    }
    let mut version = 100u32;
    let mut out = String::new();
    // One entry per open #if: (this branch is live, some branch already was,
    // the enclosing region is live).
    let mut stack: Vec<(bool, bool, bool)> = Vec::new();
    let live = |stack: &Vec<(bool, bool, bool)>| stack.last().is_none_or(|s| s.0 && s.2);
    for line in cleaned.lines() {
        let trimmed = line.trim_start();
        let Some(directive) = trimmed.strip_prefix('#') else {
            if live(&stack) {
                out.push_str(&expand(line, &macros, 0)?);
            }
            out.push('\n');
            continue;
        };
        let directive = directive.trim_start();
        let (name, rest) = directive
            .split_once(|c: char| c.is_whitespace())
            .unwrap_or((directive, ""));
        let rest = rest.trim();
        match name {
            "if" | "ifdef" | "ifndef" => {
                let enclosing = live(&stack);
                let condition = if !enclosing {
                    false
                } else {
                    match name {
                        "ifdef" => {
                            macros.contains_key(rest.split_whitespace().next().unwrap_or(""))
                        }
                        "ifndef" => {
                            !macros.contains_key(rest.split_whitespace().next().unwrap_or(""))
                        }
                        _ => eval_condition(rest, &macros)? != 0,
                    }
                };
                stack.push((condition, condition, enclosing));
            }
            "elif" => {
                let Some(top) = stack.last().copied() else {
                    return Err("#elif without #if".into());
                };
                let condition = if top.1 || !top.2 {
                    false
                } else {
                    eval_condition(rest, &macros)? != 0
                };
                let last = stack.last_mut().expect("checked above");
                *last = (condition, top.1 || condition, top.2);
            }
            "else" => {
                let Some(last) = stack.last_mut() else {
                    return Err("#else without #if".into());
                };
                let take = !last.1 && last.2;
                *last = (take, last.1 || take, last.2);
            }
            "endif" => {
                if stack.pop().is_none() {
                    return Err("#endif without #if".into());
                }
            }
            _ if !live(&stack) => {}
            "define" => define(rest, &mut macros)?,
            "undef" => {
                macros.remove(rest.split_whitespace().next().unwrap_or(""));
            }
            "version" => {
                let number = rest.split_whitespace().next().unwrap_or("100");
                version = number
                    .parse()
                    .map_err(|_| format!("bad #version '{rest}'"))?;
                if version != 100 && version != 300 {
                    return Err(format!("unsupported GLSL version {version}"));
                }
                macros.insert(
                    "__VERSION__".to_string(),
                    Macro {
                        params: None,
                        body: version.to_string(),
                    },
                );
            }
            "extension" | "pragma" | "line" | "" => {}
            "error" => return Err(format!("#error {rest}")),
            other => return Err(format!("unknown directive #{other}")),
        }
        out.push('\n');
        if out.len() > MAX_OUTPUT_BYTES {
            return Err("shader source is too large".into());
        }
    }
    if !stack.is_empty() {
        return Err("#if without #endif".into());
    }
    if out.len() > MAX_OUTPUT_BYTES {
        return Err("shader source is too large".into());
    }
    Ok(Preprocessed { text: out, version })
}

fn strip_comments_and_continuations(source: &str) -> String {
    // Line continuations first, then comments (a block comment keeps its newlines
    // so line numbers survive).
    let joined = source.replace("\\\r\n", "").replace("\\\n", "");
    let bytes: Vec<char> = joined.chars().collect();
    let mut out = String::with_capacity(joined.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '/' && bytes.get(i + 1) == Some(&'/') {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
        } else if bytes[i] == '/' && bytes.get(i + 1) == Some(&'*') {
            i += 2;
            while i < bytes.len() && !(bytes[i] == '*' && bytes.get(i + 1) == Some(&'/')) {
                if bytes[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 2;
            out.push(' ');
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn define(rest: &str, macros: &mut HashMap<String, Macro>) -> Result<(), String> {
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() && is_ident(chars[i]) {
        i += 1;
    }
    let name: String = chars[..i].iter().collect();
    if name.is_empty() || !is_ident_start(chars[0]) {
        return Err(format!("bad #define '{rest}'"));
    }
    // A `(` directly after the name makes it function-like.
    if chars.get(i) == Some(&'(') {
        let close = chars[i..]
            .iter()
            .position(|&c| c == ')')
            .ok_or("unterminated macro parameter list")?
            + i;
        let params: Vec<String> = chars[i + 1..close]
            .iter()
            .collect::<String>()
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        let body: String = chars[close + 1..].iter().collect();
        macros.insert(
            name,
            Macro {
                params: Some(params),
                body: body.trim().to_string(),
            },
        );
    } else {
        let body: String = chars[i..].iter().collect();
        macros.insert(
            name,
            Macro {
                params: None,
                body: body.trim().to_string(),
            },
        );
    }
    Ok(())
}

/// Expands macros in one line of text.
fn expand(text: &str, macros: &HashMap<String, Macro>, depth: usize) -> Result<String, String> {
    if depth > MAX_EXPANSION_DEPTH {
        return Err("macro expansion is too deep".into());
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_ident_start(c) {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match macros.get(&word) {
                Some(Macro { params: None, body }) => {
                    // Expanding a macro inside its own body would recurse; a
                    // body that mentions its own name is left as written.
                    let mut inner = macros.clone();
                    inner.remove(&word);
                    out.push_str(&expand(body, &inner, depth + 1)?);
                }
                Some(Macro {
                    params: Some(params),
                    body,
                }) => {
                    let mut j = i;
                    while j < chars.len() && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if chars.get(j) != Some(&'(') {
                        out.push_str(&word);
                        continue;
                    }
                    let (arguments, end) = split_arguments(&chars, j)?;
                    i = end;
                    let arguments: Vec<String> =
                        if params.is_empty() && arguments == [String::new()] {
                            Vec::new()
                        } else {
                            arguments
                        };
                    if arguments.len() != params.len() {
                        return Err(format!(
                            "macro {word} takes {} arguments, {} given",
                            params.len(),
                            arguments.len()
                        ));
                    }
                    // Arguments are expanded, then substituted into the body.
                    let mut substituted = String::new();
                    let body_chars: Vec<char> = body.chars().collect();
                    let mut k = 0;
                    while k < body_chars.len() {
                        if is_ident_start(body_chars[k]) {
                            let s = k;
                            while k < body_chars.len() && is_ident(body_chars[k]) {
                                k += 1;
                            }
                            let ident: String = body_chars[s..k].iter().collect();
                            match params.iter().position(|p| *p == ident) {
                                Some(n) => {
                                    substituted.push_str(&expand(&arguments[n], macros, depth + 1)?)
                                }
                                None => substituted.push_str(&ident),
                            }
                        } else {
                            substituted.push(body_chars[k]);
                            k += 1;
                        }
                    }
                    // `##` pastes its neighbours together.
                    let substituted = substituted
                        .split("##")
                        .map(str::trim)
                        .collect::<Vec<_>>()
                        .join("");
                    let mut inner = macros.clone();
                    inner.remove(&word);
                    out.push_str(&expand(&substituted, &inner, depth + 1)?);
                }
                None => out.push_str(&word),
            }
        } else if c.is_ascii_digit() {
            // Numbers (including `1e5`, `0x1F`, `1.0f`) are not identifiers.
            let start = i;
            while i < chars.len() && (is_ident(chars[i]) || chars[i] == '.') {
                i += 1;
            }
            out.extend(&chars[start..i]);
        } else {
            out.push(c);
            i += 1;
        }
    }
    Ok(out)
}

/// The comma-separated arguments of the call whose `(` is at `open`, and the
/// index just past its `)`.
fn split_arguments(chars: &[char], open: usize) -> Result<(Vec<String>, usize), String> {
    let mut depth = 0usize;
    let mut args = Vec::new();
    let mut current = String::new();
    let mut i = open;
    while i < chars.len() {
        match chars[i] {
            '(' => {
                depth += 1;
                if depth > 1 {
                    current.push('(');
                }
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    args.push(current.trim().to_string());
                    return Ok((args, i + 1));
                }
                current.push(')');
            }
            ',' if depth == 1 => {
                args.push(current.trim().to_string());
                current.clear();
            }
            c => current.push(c),
        }
        i += 1;
    }
    Err("unterminated macro call".into())
}

/// Evaluates an `#if` expression: integers, `defined`, macros, `! ~ - +`, `* / %`,
/// `+ -`, shifts, comparisons, `& ^ |`, `&& ||` and parentheses.
fn eval_condition(text: &str, macros: &HashMap<String, Macro>) -> Result<i64, String> {
    // `defined X` / `defined(X)` before expansion, so X is not expanded away.
    let mut replaced = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_ident_start(chars[i]) {
            let s = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[s..i].iter().collect();
            if word == "defined" {
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                let paren = chars.get(i) == Some(&'(');
                if paren {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                let s = i;
                while i < chars.len() && is_ident(chars[i]) {
                    i += 1;
                }
                let target: String = chars[s..i].iter().collect();
                if paren {
                    while i < chars.len() && chars[i] != ')' {
                        i += 1;
                    }
                    i += 1;
                }
                replaced.push_str(if macros.contains_key(&target) {
                    "1"
                } else {
                    "0"
                });
            } else {
                replaced.push_str(&word);
            }
        } else {
            replaced.push(chars[i]);
            i += 1;
        }
    }
    let expanded = expand(&replaced, macros, 0)?;
    let mut parser = ConditionParser {
        tokens: tokenize_condition(&expanded)?,
        at: 0,
    };
    let value = parser.or()?;
    if parser.at != parser.tokens.len() {
        return Err(format!("unexpected tokens in #if '{text}'"));
    }
    Ok(value)
}

fn tokenize_condition(text: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let s = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            out.push(chars[s..i].iter().collect());
        } else if is_ident_start(c) {
            // An unknown identifier is 0.
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            out.push("0".to_string());
        } else {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            if ["&&", "||", "==", "!=", "<=", ">=", "<<", ">>"].contains(&two.as_str()) {
                out.push(two);
                i += 2;
            } else if "()!~+-*/%<>&|^".contains(c) {
                out.push(c.to_string());
                i += 1;
            } else {
                return Err(format!("unexpected '{c}' in #if"));
            }
        }
    }
    Ok(out)
}

struct ConditionParser {
    tokens: Vec<String>,
    at: usize,
}

impl ConditionParser {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.at).map(String::as_str)
    }
    fn take(&mut self) -> Option<String> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }
    fn or(&mut self) -> Result<i64, String> {
        let mut v = self.and()?;
        while self.peek() == Some("||") {
            self.at += 1;
            let r = self.and()?;
            v = i64::from(v != 0 || r != 0);
        }
        Ok(v)
    }
    fn and(&mut self) -> Result<i64, String> {
        let mut v = self.bit_or()?;
        while self.peek() == Some("&&") {
            self.at += 1;
            let r = self.bit_or()?;
            v = i64::from(v != 0 && r != 0);
        }
        Ok(v)
    }
    fn bit_or(&mut self) -> Result<i64, String> {
        let mut v = self.bit_xor()?;
        while self.peek() == Some("|") {
            self.at += 1;
            v |= self.bit_xor()?;
        }
        Ok(v)
    }
    fn bit_xor(&mut self) -> Result<i64, String> {
        let mut v = self.bit_and()?;
        while self.peek() == Some("^") {
            self.at += 1;
            v ^= self.bit_and()?;
        }
        Ok(v)
    }
    fn bit_and(&mut self) -> Result<i64, String> {
        let mut v = self.equality()?;
        while self.peek() == Some("&") {
            self.at += 1;
            v &= self.equality()?;
        }
        Ok(v)
    }
    fn equality(&mut self) -> Result<i64, String> {
        let mut v = self.relational()?;
        while let Some(op) = self.peek().filter(|o| *o == "==" || *o == "!=") {
            let equal = op == "==";
            self.at += 1;
            let r = self.relational()?;
            v = i64::from((v == r) == equal);
        }
        Ok(v)
    }
    fn relational(&mut self) -> Result<i64, String> {
        let mut v = self.shift()?;
        while let Some(op) = self.peek().filter(|o| ["<", ">", "<=", ">="].contains(o)) {
            let op = op.to_string();
            self.at += 1;
            let r = self.shift()?;
            v = i64::from(match op.as_str() {
                "<" => v < r,
                ">" => v > r,
                "<=" => v <= r,
                _ => v >= r,
            });
        }
        Ok(v)
    }
    fn shift(&mut self) -> Result<i64, String> {
        let mut v = self.additive()?;
        while let Some(op) = self.peek().filter(|o| *o == "<<" || *o == ">>") {
            let left = op == "<<";
            self.at += 1;
            let r = self.additive()?.clamp(0, 63) as u32;
            v = if left { v.wrapping_shl(r) } else { v >> r };
        }
        Ok(v)
    }
    fn additive(&mut self) -> Result<i64, String> {
        let mut v = self.multiplicative()?;
        while let Some(op) = self.peek().filter(|o| *o == "+" || *o == "-") {
            let plus = op == "+";
            self.at += 1;
            let r = self.multiplicative()?;
            v = if plus {
                v.wrapping_add(r)
            } else {
                v.wrapping_sub(r)
            };
        }
        Ok(v)
    }
    fn multiplicative(&mut self) -> Result<i64, String> {
        let mut v = self.unary()?;
        while let Some(op) = self.peek().filter(|o| ["*", "/", "%"].contains(o)) {
            let op = op.to_string();
            self.at += 1;
            let r = self.unary()?;
            v = match op.as_str() {
                "*" => v.wrapping_mul(r),
                "/" if r != 0 => v / r,
                "%" if r != 0 => v % r,
                _ => return Err("division by zero in #if".into()),
            };
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<i64, String> {
        match self.peek() {
            Some("!") => {
                self.at += 1;
                Ok(i64::from(self.unary()? == 0))
            }
            Some("~") => {
                self.at += 1;
                Ok(!self.unary()?)
            }
            Some("-") => {
                self.at += 1;
                Ok(self.unary()?.wrapping_neg())
            }
            Some("+") => {
                self.at += 1;
                self.unary()
            }
            Some("(") => {
                self.at += 1;
                let v = self.or()?;
                if self.take().as_deref() != Some(")") {
                    return Err("missing ) in #if".into());
                }
                Ok(v)
            }
            Some(_) => {
                let t = self.take().expect("peeked");
                let t = t.trim_end_matches(['u', 'U']);
                if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                    i64::from_str_radix(hex, 16).map_err(|_| format!("bad number '{t}'"))
                } else {
                    t.parse().map_err(|_| format!("bad number '{t}'"))
                }
            }
            None => Err("unexpected end of #if".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> String {
        preprocess(source).unwrap().text
    }

    #[test]
    fn comments_and_continuations_are_removed_but_lines_are_kept() {
        let out = run("a // gone\nb /* x\ny */ c\nd \\\ne");
        assert_eq!(
            out.lines().collect::<Vec<_>>(),
            vec!["a ", "b ", "  c", "d e"]
        );
    }

    #[test]
    fn object_and_function_macros_expand() {
        assert_eq!(
            run("#define N 3\nfloat a[N];").lines().nth(1).unwrap(),
            "float a[3];"
        );
        let out = run("#define SQ(x) ((x)*(x))\nfloat y = SQ(1.0 + 2.0);");
        assert_eq!(
            out.lines().nth(1).unwrap(),
            "float y = ((1.0 + 2.0)*(1.0 + 2.0));"
        );
        let out = run("#define ADD(a,b) a+b\nint z = ADD(ADD(1,2),3);");
        assert_eq!(out.lines().nth(1).unwrap(), "int z = 1+2+3;");
    }

    #[test]
    fn a_macro_may_not_expand_itself_and_numbers_are_not_identifiers() {
        let out = run("#define a a+1\nint x = a;\n#define e 5\nfloat f = 1e3 + 2.0e;");
        assert_eq!(out.lines().nth(1).unwrap(), "int x = a+1;");
        assert!(out
            .lines()
            .nth(3)
            .unwrap()
            .starts_with("float f = 1e3 + 2.0e"));
    }

    #[test]
    fn conditionals_pick_one_branch_and_nest() {
        let src = "#define A 1\n#if defined(A) && A > 0\nyes\n#elif 1\nelif\n#else\nno\n#endif\n#ifdef B\nb\n#else\nnotb\n#endif\n#if 0\n#if 1\nhidden\n#endif\n#endif";
        let out = run(src);
        let kept: Vec<&str> = out.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(kept, vec!["yes", "notb"]);
    }

    #[test]
    fn elif_and_else_follow_a_taken_branch_correctly() {
        let out = run("#if 0\na\n#elif 0\nb\n#elif 1\nc\n#elif 1\nd\n#else\ne\n#endif");
        let kept: Vec<&str> = out.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(kept, vec!["c"]);
    }

    #[test]
    fn the_version_and_predefined_macros_are_known() {
        let p = preprocess(
            "#version 300 es\n#if __VERSION__ >= 300\nmodern\n#endif\n#ifdef GL_ES\nes\n#endif",
        )
        .unwrap();
        assert_eq!(p.version, 300);
        let kept: Vec<&str> = p.text.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(kept, vec!["modern", "es"]);
        assert_eq!(preprocess("void main(){}").unwrap().version, 100);
        assert!(preprocess("#version 450\n").is_err());
    }

    #[test]
    fn extensions_and_pragmas_are_accepted_and_errors_fail() {
        assert!(preprocess(
            "#extension GL_OES_standard_derivatives : enable\n#pragma optimize(on)\n"
        )
        .is_ok());
        assert!(preprocess("#error nope").is_err());
        assert!(preprocess("#if 1\nx").is_err());
        assert!(preprocess("#endif").is_err());
        assert!(preprocess("#define F(a) a\nF(1,2)").is_err());
    }

    #[test]
    fn token_pasting_and_condition_arithmetic_work() {
        let out = run("#define CAT(a,b) a##b\nint CAT(foo,bar) = (1 << 3) + 7 % 4;\n#if (1 << 3) + 7 % 4 == 11\nok\n#endif");
        assert_eq!(
            out.lines().nth(1).unwrap(),
            "int foobar = (1 << 3) + 7 % 4;"
        );
        assert!(out.contains("ok"));
    }

    #[test]
    fn recursive_function_macros_stop() {
        assert!(preprocess("#define F(x) F(x)\nF(1)").is_ok());
        let deep = "#define A B\n#define B C\n#define C A\nA";
        assert!(preprocess(deep).is_ok());
    }
}
