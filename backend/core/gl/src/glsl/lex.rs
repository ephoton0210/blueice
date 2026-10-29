// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! GLSL tokens.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    UInt(u32),
    Float(f32),
    Punct(&'static str),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

const PUNCT: [&str; 47] = [
    "<<=", ">>=", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "<=", ">=",
    "==", "!=", "&&", "||", "^^", "(", ")", "[", "]", "{", "}", ".", ",", ";", ":", "?", "+", "-", "*",
    "/", "%", "<", ">", "=", "!", "~", "&", "|", "^", "#", "@",
];

pub fn lex(source: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let s = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Ident(chars[s..i].iter().collect()),
                line,
            });
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let s = i;
            let hex = c == '0' && matches!(chars.get(i + 1), Some('x' | 'X'));
            if hex {
                i += 2;
                while i < chars.len() && chars[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if chars.get(i) == Some(&'.') {
                    i += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if matches!(chars.get(i), Some('e' | 'E')) {
                    let mut j = i + 1;
                    if matches!(chars.get(j), Some('+' | '-')) {
                        j += 1;
                    }
                    if chars.get(j).is_some_and(|d| d.is_ascii_digit()) {
                        i = j;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
            }
            let body: String = chars[s..i].iter().collect();
            let suffix = chars.get(i).copied().filter(|c| matches!(c, 'u' | 'U' | 'f' | 'F'));
            if suffix.is_some() {
                i += 1;
            }
            let is_float = !hex && (body.contains('.') || body.contains('e') || body.contains('E'));
            let tok = if is_float || matches!(suffix, Some('f' | 'F')) {
                Tok::Float(body.parse::<f32>().map_err(|_| format!("bad number '{body}'"))?)
            } else {
                let value = if hex {
                    i64::from_str_radix(&body[2..], 16)
                } else if body.len() > 1 && body.starts_with('0') {
                    i64::from_str_radix(&body[1..], 8)
                } else {
                    body.parse::<i64>()
                }
                .map_err(|_| format!("bad number '{body}'"))?;
                if matches!(suffix, Some('u' | 'U')) {
                    Tok::UInt(value as u32)
                } else {
                    Tok::Int(value)
                }
            };
            out.push(Token { tok, line });
        } else {
            let rest: String = chars[i..(i + 3).min(chars.len())].iter().collect();
            let Some(p) = PUNCT.iter().find(|p| rest.starts_with(**p)) else {
                return Err(format!("line {line}: unexpected character '{c}'"));
            };
            i += p.len();
            out.push(Token {
                tok: Tok::Punct(p),
                line,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn numbers_come_in_every_form() {
        assert_eq!(
            toks("1 1.5 .5 1e3 2.5e-2 3u 0x1F 010 1.0f 5."),
            vec![
                Tok::Int(1),
                Tok::Float(1.5),
                Tok::Float(0.5),
                Tok::Float(1000.0),
                Tok::Float(0.025),
                Tok::UInt(3),
                Tok::Int(31),
                Tok::Int(8),
                Tok::Float(1.0),
                Tok::Float(5.0)
            ]
        );
    }

    #[test]
    fn longest_operators_win_and_lines_count() {
        let t = lex("a <<= b\n>> c++ ^^ d").unwrap();
        let puncts: Vec<&str> = t
            .iter()
            .filter_map(|t| if let Tok::Punct(p) = t.tok { Some(p) } else { None })
            .collect();
        assert_eq!(puncts, vec!["<<=", ">>", "++", "^^"]);
        assert_eq!(t.last().unwrap().line, 2);
        assert!(lex("a $ b").is_err());
    }

    #[test]
    fn a_dot_after_an_identifier_is_a_field_access() {
        assert_eq!(
            toks("v.xy"),
            vec![Tok::Ident("v".into()), Tok::Punct("."), Tok::Ident("xy".into())]
        );
    }
}
