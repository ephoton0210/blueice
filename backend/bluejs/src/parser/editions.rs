// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Edition checks on consumed lexical tokens and the parsed syntax tree.

use super::*;
mod expressions;
mod statements;

/// Output syntax editions. ESNext retains the parser's existing supported
/// grammar, including its implemented proposals; it promises no new grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SyntaxEdition {
    Es5,
    Es2015,
    Es2016,
    Es2017,
    Es2018,
    Es2019,
    Es2020,
    Es2021,
    Es2022,
    Es2023,
    #[default]
    EsNext,
}

impl SyntaxEdition {
    pub(super) fn require(self, minimum: Self, feature: &str) -> Result<(), ParseError> {
        if self < minimum {
            Err(ParseError {
                message: format!("{feature} requires {minimum:?} syntax; selected {self:?}"),
                resource: None,
                known_syntax: true,
            })
        } else {
            Ok(())
        }
    }
}

impl Parser {
    pub(super) fn select_edition(&mut self, edition: SyntaxEdition) -> Result<(), ParseError> {
        self.edition = edition;
        if self.source.starts_with("#!") {
            edition.require(SyntaxEdition::Es2023, "hashbang comments")?;
        }
        Ok(())
    }

    pub(super) fn validate_edition(&self) -> Result<(), ParseError> {
        self.edition_error.clone().map_or(Ok(()), Err)
    }

    // Checking only tokens that the parser consumes preserves the RegExp
    // lexical goal: discarded speculative division tokens are never checked.
    pub(super) fn check_edition_token(&mut self) {
        if self.edition == SyntaxEdition::EsNext || self.edition_error.is_some() {
            return;
        }
        let start = self.tokens[self.pos].start;
        let end = self.positions.get(self.pos + 1).copied().unwrap_or(start);
        let to_byte = |offset| match &self.char_to_byte {
            Some(offsets) => offsets[offset],
            None => offset,
        };
        let raw = &self.source[to_byte(start)..to_byte(end)];
        let feature = match self.peek() {
            Token::Number(_) | Token::BigInt(_) if raw.contains('_') => {
                Some((SyntaxEdition::Es2021, "numeric separators"))
            }
            Token::Number(_)
                if ["0b", "0B", "0o", "0O"]
                    .iter()
                    .any(|prefix| raw.starts_with(*prefix)) =>
            {
                Some((SyntaxEdition::Es2015, "binary and octal literals"))
            }
            Token::String(_) if has_unescaped_line_separator(raw) => {
                Some((SyntaxEdition::Es2019, "unescaped string line separators"))
            }
            Token::String(_) | Token::Identifier(_) | Token::Keyword(_)
                if has_code_point_escape(raw) =>
            {
                Some((SyntaxEdition::Es2015, "Unicode code point escapes"))
            }
            Token::Punct(Punct::Comma) if self.check_punct_at(1, Punct::RParen) => Some((
                SyntaxEdition::Es2017,
                "trailing parameter and argument commas",
            )),
            _ => None,
        };
        if let Some((minimum, feature)) = feature {
            self.edition_error = self.edition.require(minimum, feature).err();
        }
    }
}

fn has_code_point_escape(raw: &str) -> bool {
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.next() == Some('u') && chars.peek() == Some(&'{') {
            return true;
        }
    }
    false
}

fn has_unescaped_line_separator(raw: &str) -> bool {
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            chars.next();
        } else if matches!(ch, '\u{2028}' | '\u{2029}') {
            return true;
        }
    }
    false
}

enum Node<'a> {
    Statement(&'a Stmt),
    Expression(&'a Expr),
    Function(&'a Function),
    Class(&'a Class),
    Pattern(&'a Pattern),
    AssignmentPattern(&'a AssignmentPattern),
    Key(&'a PropertyKey),
}

struct Validator<'a> {
    edition: SyntaxEdition,
    // An iterative walk borrows child nodes instead of copying trees or
    // adding another native recursion stack to the parser.
    pending: Vec<(Node<'a>, bool)>,
}

pub(super) fn validate_body(body: &[Stmt], edition: SyntaxEdition) -> Result<(), ParseError> {
    if edition == SyntaxEdition::EsNext {
        return Ok(());
    }
    let mut validator = Validator {
        edition,
        pending: Vec::new(),
    };
    validator.statements(body, false);
    validator.run()
}

pub(super) fn validate_expression(
    expr: &Expr,
    edition: SyntaxEdition,
    in_function: bool,
) -> Result<(), ParseError> {
    if edition == SyntaxEdition::EsNext {
        return Ok(());
    }
    Validator {
        edition,
        pending: vec![(Node::Expression(expr), in_function)],
    }
    .run()
}

impl<'a> Validator<'a> {
    fn require(&self, minimum: SyntaxEdition, feature: &str) -> Result<(), ParseError> {
        self.edition.require(minimum, feature)
    }

    fn push(&mut self, node: Node<'a>, in_function: bool) {
        self.pending.push((node, in_function));
    }

    fn expression(&mut self, expr: &'a Expr, in_function: bool) {
        self.push(Node::Expression(expr), in_function);
    }

    fn optional_expression(&mut self, expr: Option<&'a Expr>, in_function: bool) {
        if let Some(expr) = expr {
            self.expression(expr, in_function);
        }
    }

    fn statements(&mut self, statements: &'a [Stmt], in_function: bool) {
        for statement in statements {
            self.push(Node::Statement(statement), in_function);
        }
    }

    fn parameters(&mut self, parameters: &'a [Param]) -> Result<(), ParseError> {
        for parameter in parameters {
            if parameter.rest || parameter.default.is_some() {
                self.require(SyntaxEdition::Es2015, "rest and default parameters")?;
            }
            self.push(Node::Pattern(&parameter.pattern), true);
            self.optional_expression(parameter.default.as_ref(), true);
        }
        Ok(())
    }

    fn run(mut self) -> Result<(), ParseError> {
        if self.edition == SyntaxEdition::EsNext {
            return Ok(());
        }
        while let Some((node, in_function)) = self.pending.pop() {
            match node {
                Node::Statement(stmt) => self.statement(stmt, in_function)?,
                Node::Expression(expr) => self.expr(expr, in_function)?,
                Node::Function(function) => {
                    if function.generator {
                        self.require(SyntaxEdition::Es2015, "generators")?;
                    }
                    if function.is_async {
                        self.require(SyntaxEdition::Es2017, "async functions")?;
                    }
                    if function.generator && function.is_async {
                        self.require(SyntaxEdition::Es2018, "async generators")?;
                    }
                    self.parameters(&function.params)?;
                    self.statements(&function.body, true);
                }
                Node::Class(class) => self.class(class, in_function)?,
                Node::Pattern(pattern) => self.pattern(pattern, in_function)?,
                Node::AssignmentPattern(pattern) => {
                    self.assignment_pattern(pattern, in_function)?
                }
                Node::Key(key) => match key {
                    PropertyKey::Computed(expr) => {
                        self.require(SyntaxEdition::Es2015, "computed property names")?;
                        self.expression(expr, in_function);
                    }
                    PropertyKey::Identifier(name) if name.starts_with('#') => {
                        self.require(SyntaxEdition::Es2022, "private names")?
                    }
                    _ => {}
                },
            }
        }
        Ok(())
    }
}
