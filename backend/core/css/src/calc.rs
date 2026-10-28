// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `calc()`, `min()`, `max()` and `clamp()`: parsed once into a [`Calc`] tree
//! and evaluated by layout, because the answer depends on the font size, the
//! viewport and the containing block that only layout knows.

use crate::tokenizer::Token;
use crate::value::{Length, LengthContext};

#[derive(Debug, Clone, PartialEq)]
pub enum Calc {
    Number(f64),
    Length(Length),
    Percent(f64),
    Sum(Box<Calc>, Box<Calc>),
    Difference(Box<Calc>, Box<Calc>),
    Product(Box<Calc>, Box<Calc>),
    Quotient(Box<Calc>, Box<Calc>),
    Min(Vec<Calc>),
    Max(Vec<Calc>),
    Clamp(Box<Calc>, Box<Calc>, Box<Calc>),
}

/// An evaluated term: a plain number, or a length in pixels.
#[derive(Clone, Copy)]
enum Term {
    Number(f64),
    Pixels(f64),
}

impl Calc {
    /// The pixel value, or `None` when it is not a length (a bare number
    /// result), divides by zero, or needs a percentage base that does not exist.
    pub fn to_px(&self, ctx: &LengthContext) -> Option<f64> {
        match self.eval(ctx)? {
            Term::Pixels(px) if px.is_nan() => None,
            Term::Pixels(px) => Some(px),
            Term::Number(_) => None,
        }
    }

    fn eval(&self, ctx: &LengthContext) -> Option<Term> {
        Some(match self {
            Calc::Number(n) => Term::Number(*n),
            Calc::Length(length) => Term::Pixels(length.to_px(ctx)),
            Calc::Percent(p) => {
                if ctx.percent_base.is_nan() {
                    return None;
                }
                Term::Pixels(ctx.percent_base * p / 100.0)
            }
            Calc::Sum(a, b) | Calc::Difference(a, b) => {
                let sign = if matches!(self, Calc::Sum(..)) {
                    1.0
                } else {
                    -1.0
                };
                match (a.eval(ctx)?, b.eval(ctx)?) {
                    (Term::Pixels(x), Term::Pixels(y)) => Term::Pixels(x + sign * y),
                    (Term::Number(x), Term::Number(y)) => Term::Number(x + sign * y),
                    _ => return None, // a length plus a bare number is invalid
                }
            }
            Calc::Product(a, b) => match (a.eval(ctx)?, b.eval(ctx)?) {
                (Term::Number(x), Term::Number(y)) => Term::Number(x * y),
                (Term::Pixels(x), Term::Number(y)) | (Term::Number(y), Term::Pixels(x)) => {
                    Term::Pixels(x * y)
                }
                _ => return None,
            },
            Calc::Quotient(a, b) => {
                let Term::Number(divisor) = b.eval(ctx)? else {
                    return None;
                };
                if divisor == 0.0 {
                    return None;
                }
                match a.eval(ctx)? {
                    Term::Number(x) => Term::Number(x / divisor),
                    Term::Pixels(x) => Term::Pixels(x / divisor),
                }
            }
            Calc::Min(items) | Calc::Max(items) => {
                let is_min = matches!(self, Calc::Min(_));
                let mut best: Option<Term> = None;
                for item in items {
                    let term = item.eval(ctx)?;
                    best = Some(match (best, term) {
                        (None, term) => term,
                        (Some(Term::Pixels(a)), Term::Pixels(b)) => {
                            Term::Pixels(if is_min { a.min(b) } else { a.max(b) })
                        }
                        (Some(Term::Number(a)), Term::Number(b)) => {
                            Term::Number(if is_min { a.min(b) } else { a.max(b) })
                        }
                        _ => return None,
                    });
                }
                best?
            }
            Calc::Clamp(low, value, high) => {
                match (low.eval(ctx)?, value.eval(ctx)?, high.eval(ctx)?) {
                    (Term::Pixels(l), Term::Pixels(v), Term::Pixels(h)) => {
                        Term::Pixels(v.max(l).min(h.max(l)))
                    }
                    (Term::Number(l), Term::Number(v), Term::Number(h)) => {
                        Term::Number(v.max(l).min(h.max(l)))
                    }
                    _ => return None,
                }
            }
        })
    }
}

/// Parses `tokens` (exactly one `calc()`, `min()`, `max()` or `clamp()`
/// call) into a [`Calc`] tree.
pub(crate) fn parse_calc(tokens: &[Token]) -> Option<Calc> {
    let tokens: Vec<Token> = tokens
        .iter()
        .filter(|t| !matches!(t, Token::Whitespace | Token::Eof))
        .cloned()
        .collect();
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        depth: 0,
    };
    let value = parser.function()?;
    (parser.pos == tokens.len()).then_some(value)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// `calc(` / `min(` / `max(` / `clamp(` ... `)` starting at the current token.
    fn function(&mut self) -> Option<Calc> {
        let Some(Token::Ident(name)) = self.peek() else {
            return None;
        };
        let name = name.to_ascii_lowercase();
        if !matches!(self.tokens.get(self.pos + 1), Some(Token::LeftParen)) || self.depth > 32 {
            return None;
        }
        self.pos += 2;
        self.depth += 1;
        let value = match name.as_str() {
            "calc" | "-webkit-calc" | "-moz-calc" => self.sum()?,
            "min" | "max" | "clamp" => {
                let mut items = vec![self.sum()?];
                while self.peek() == Some(&Token::Comma) {
                    self.pos += 1;
                    items.push(self.sum()?);
                }
                match (name.as_str(), items.len()) {
                    ("min", _) => Calc::Min(items),
                    ("max", _) => Calc::Max(items),
                    ("clamp", 3) => {
                        let mut items = items.into_iter();
                        Calc::Clamp(
                            Box::new(items.next()?),
                            Box::new(items.next()?),
                            Box::new(items.next()?),
                        )
                    }
                    _ => return None,
                }
            }
            _ => return None,
        };
        self.depth -= 1;
        (self.peek() == Some(&Token::RightParen)).then(|| {
            self.pos += 1;
            value
        })
    }

    fn sum(&mut self) -> Option<Calc> {
        let mut left = self.product()?;
        loop {
            let plus = self.peek() == Some(&Token::Delim('+'));
            let minus = self.peek() == Some(&Token::Delim('-'));
            if !plus && !minus {
                return Some(left);
            }
            self.pos += 1;
            let right = self.product()?;
            left = if plus {
                Calc::Sum(Box::new(left), Box::new(right))
            } else {
                Calc::Difference(Box::new(left), Box::new(right))
            };
        }
    }

    fn product(&mut self) -> Option<Calc> {
        let mut left = self.atom()?;
        loop {
            let times = self.peek() == Some(&Token::Delim('*'));
            let divide = self.peek() == Some(&Token::Delim('/'));
            if !times && !divide {
                return Some(left);
            }
            self.pos += 1;
            let right = self.atom()?;
            left = if times {
                Calc::Product(Box::new(left), Box::new(right))
            } else {
                Calc::Quotient(Box::new(left), Box::new(right))
            };
        }
    }

    fn atom(&mut self) -> Option<Calc> {
        match self.peek()?.clone() {
            Token::Number(n) => {
                self.pos += 1;
                Some(Calc::Number(n))
            }
            Token::Percentage(p) => {
                self.pos += 1;
                Some(Calc::Percent(p))
            }
            Token::Dimension(n, unit) => {
                self.pos += 1;
                Length::from_unit(n, &unit).map(Calc::Length)
            }
            Token::LeftParen => {
                self.pos += 1;
                self.depth += 1;
                let inner = self.sum()?;
                self.depth -= 1;
                (self.peek() == Some(&Token::RightParen)).then(|| {
                    self.pos += 1;
                    inner
                })
            }
            Token::Ident(word) if word.eq_ignore_ascii_case("infinity") => {
                self.pos += 1;
                Some(Calc::Number(f64::INFINITY))
            }
            Token::Ident(word) if word.eq_ignore_ascii_case("pi") => {
                self.pos += 1;
                Some(Calc::Number(std::f64::consts::PI))
            }
            Token::Ident(_) => self.function(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn ctx() -> LengthContext {
        LengthContext {
            font_size: 20.0,
            root_font_size: 16.0,
            viewport_width: 1000.0,
            viewport_height: 500.0,
            percent_base: 400.0,
        }
    }
    fn px(css: &str) -> Option<f64> {
        parse_calc(&tokenize(css))?.to_px(&ctx())
    }

    #[test]
    fn arithmetic_with_precedence_and_mixed_units() {
        assert_eq!(px("calc(1px + 2px * 3)"), Some(7.0));
        assert_eq!(px("calc((1px + 2px) * 3)"), Some(9.0));
        assert_eq!(px("calc(100% - 2rem)"), Some(400.0 - 32.0));
        assert_eq!(px("calc(50vw + 10em)"), Some(500.0 + 200.0));
        assert_eq!(px("calc(100vh / 4)"), Some(125.0));
        assert_eq!(px("calc(-1 * 5px)"), Some(-5.0));
        assert_eq!(px("calc(10px - 3px - 2px)"), Some(5.0), "left associative");
        assert_eq!(px("calc(100% / 3)"), Some(400.0 / 3.0));
    }

    #[test]
    fn nested_calc_and_min_max_clamp() {
        assert_eq!(px("calc(calc(1px + 1px) * 2)"), Some(4.0));
        assert_eq!(px("min(10px, 5px, 20px)"), Some(5.0));
        assert_eq!(px("max(10px, 5%)"), Some(20.0));
        assert_eq!(px("clamp(10px, 50%, 100px)"), Some(100.0));
        assert_eq!(px("clamp(10px, 1px, 100px)"), Some(10.0));
        assert_eq!(px("calc(min(50vw, 300px) + 1px)"), Some(301.0));
    }

    #[test]
    fn invalid_expressions_do_not_evaluate() {
        assert_eq!(px("calc(1px + 2)"), None, "length plus a bare number");
        assert_eq!(px("calc(1px * 2px)"), None, "length times length");
        assert_eq!(px("calc(1px / 0)"), None);
        assert_eq!(px("calc(1px /"), None);
        assert_eq!(px("calc()"), None);
        assert_eq!(px("clamp(1px, 2px)"), None);
        assert_eq!(px("calc(2)"), None, "a number is not a length");
        assert_eq!(px("calc(1px) trailing"), None);
    }

    #[test]
    fn a_percentage_without_a_base_is_indeterminate() {
        let mut no_base = ctx();
        no_base.percent_base = f64::NAN;
        let calc = parse_calc(&tokenize("calc(100% - 1px)")).unwrap();
        assert_eq!(calc.to_px(&no_base), None);
        let plain = parse_calc(&tokenize("calc(3px + 1px)")).unwrap();
        assert_eq!(plain.to_px(&no_base), Some(4.0));
    }

    #[test]
    fn infinity_times_a_length_is_infinite() {
        assert_eq!(px("calc(infinity * 1px)"), Some(f64::INFINITY));
    }
}
