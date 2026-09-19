// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recursive-descent parser: [`crate::token::Token`] stream -> [`Program`].
//! Scoped to exactly `phase-2-mvp-scope/PLAN.md`'s "MVP JS scope
//! (decided)" section -- see `ast.rs`'s own doc comment for what's
//! deliberately absent.
//!
//! **Precedence, as a plain top-down chain of functions** (each tier
//! calls the next-tighter one, matching this codebase's other
//! from-scratch parsers' style, e.g. `blueice-css`'s `parser.rs`,
//! rather than a table-driven Pratt parser): assignment (and arrow
//! functions, detected here since they share `AssignmentExpression`'s
//! grammar slot) -> conditional (`?:`) -> nullish (`??`) -> logical OR
//! -> logical AND -> bitwise OR -> bitwise XOR -> bitwise AND -> equality
//! -> relational -> shift -> additive -> multiplicative -> unary -> postfix
//! update (`++`/`--`) -> left-hand-side (`new`/member/call chains) -> primary.
//! ECMAScript §13.13 keeps
//! unparenthesized coalescing and logical AND/OR in separate productions;
//! grammar-level flags enforce that distinction before parentheses are
//! discarded from the AST.
//!
//! **`for`-loop head disambiguation** follows the standard technique
//! real engines use: parse the head with the `in` operator temporarily
//! disabled (`no_in`) so `for (x in y)` doesn't get misparsed as the
//! expression `x in y`, then look at what follows to decide whether
//! it's a classic `for`, a `for-in`, or a contextual `for-of` (`of`
//! isn't a reserved keyword in real ECMAScript either -- recognized
//! here the same way, as a plain identifier whose name happens to be
//! `"of"` in exactly this grammar position).
//!
//! **Template literal placeholders are parsed by recursion, not by
//! consuming a pre-built token stream**: [`crate::token::Token::Template`]
//! carries each `${...}` placeholder as raw source text (see
//! `token.rs`'s own doc comment for why), and [`parse_template`] below
//! re-tokenizes/re-parses each one independently via
//! [`parse_expression_from_source`] -- a fresh [`Parser`] over just that
//! substring, required to consume it entirely as one expression.

use crate::ast::*;
use crate::token::{Keyword, LexError, Punct, SpannedToken, Token, Tokenizer};

mod expressions;
mod functions;
mod module;
mod module_items;
mod patterns;
mod statements;
#[cfg(test)]
mod tests;

pub use module::parse_module;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    /// Resource failures during literal validation are not SyntaxErrors.
    pub resource: Option<crate::RuntimeError>,
    /// True only when the parser recognized a specified syntax error. Other
    /// parse failures may still be unsupported valid grammar in this subset.
    pub known_syntax: bool,
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> ParseError {
        ParseError {
            message: e.message,
            resource: None,
            known_syntax: e.known_syntax,
        }
    }
}

pub fn parse(source: &str) -> Result<Program, ParseError> {
    let mut parser = Parser::new(source);
    let mut body = Vec::new();
    let mut directive_prologue = true;
    while !parser.at_eof() {
        let statement = parser.parse_statement()?;
        if directive_prologue {
            if let Stmt::Expr(Expr::String(value)) = &statement {
                if value == "use strict" {
                    parser.strict = true;
                }
            } else {
                directive_prologue = false;
            }
        }
        body.push(statement);
    }
    let program = Program { body };
    if contains_super_call_outside_class(&program)
        || contains_super_property_outside_class(&program)
    {
        return Err(parser.syntax_error("super is not valid in script code"));
    }
    crate::compiler::validate_private_early_errors(&program)
        .map_err(|error| parser.syntax_error(error.to_string()))?;
    Ok(program)
}

/// Parses direct-eval source before its caller applies context-sensitive
/// `super` early errors. At script top level those expressions are invalid,
/// but a direct eval inherits the calling method's `[[HomeObject]]`.
pub(crate) fn parse_eval(source: &str) -> Result<Program, ParseError> {
    let mut parser = Parser::new(source);
    let mut body = Vec::new();
    while !parser.at_eof() {
        body.push(parser.parse_statement()?);
    }
    Ok(Program { body })
}

// Preserve source positions so the parser can select the RegExp lexical goal
// at PrimaryExpression and rescan the suffix. A speculative division scan may
// encounter regex-only characters; defer that lexical error until consumed.
fn tokenize_all(tokenizer: &mut Tokenizer) -> (Vec<SpannedToken>, Vec<usize>) {
    let mut tokens = Vec::new();
    let mut positions = Vec::new();
    loop {
        positions.push(tokenizer.position());
        match tokenizer.next_spanned() {
            Ok(spanned) => {
                let done = spanned.token == Token::Eof;
                tokens.push(spanned);
                if done {
                    return (tokens, positions);
                }
            }
            Err(error) => {
                tokens.push(SpannedToken {
                    token: Token::Invalid(error.message),
                    newline_before: false,
                    identifier_escaped: false,
                });
                positions.push(tokenizer.position());
                tokens.push(SpannedToken {
                    token: Token::Eof,
                    newline_before: false,
                    identifier_escaped: false,
                });
                return (tokens, positions);
            }
        }
    }
}

/// Parses `source` as one standalone expression -- used both by
/// [`parse_template`] for placeholder text and, in tests, to exercise
/// expression parsing without wrapping every fixture in a statement.
fn parse_expression_from_source(source: &str) -> Result<Expr, ParseError> {
    let mut parser = Parser::new(source);
    let expr = parser.parse_expression()?;
    if !parser.at_eof() {
        return Err(parser.error("unexpected trailing tokens after expression"));
    }
    Ok(expr)
}

pub(crate) fn closes_template_placeholder(source: &str) -> bool {
    let mut parser = Parser::new(source);
    parser.parse_expression().is_ok() && parser.eat_punct(Punct::RBrace) && parser.at_eof()
}

fn keyword_as_str(k: Keyword) -> &'static str {
    match k {
        Keyword::Var => "var",
        Keyword::Let => "let",
        Keyword::Const => "const",
        Keyword::Function => "function",
        Keyword::Return => "return",
        Keyword::If => "if",
        Keyword::Else => "else",
        Keyword::For => "for",
        Keyword::While => "while",
        Keyword::Do => "do",
        Keyword::Switch => "switch",
        Keyword::Case => "case",
        Keyword::Default => "default",
        Keyword::Break => "break",
        Keyword::Continue => "continue",
        Keyword::Throw => "throw",
        Keyword::Try => "try",
        Keyword::Catch => "catch",
        Keyword::Finally => "finally",
        Keyword::New => "new",
        Keyword::Typeof => "typeof",
        Keyword::Void => "void",
        Keyword::Delete => "delete",
        Keyword::Instanceof => "instanceof",
        Keyword::In => "in",
        Keyword::True => "true",
        Keyword::False => "false",
        Keyword::Null => "null",
        Keyword::This => "this",
    }
}

/// A plain identifier or member expression -- the ordinary assignment-target
/// and `++`/`--` operand shapes.
fn is_valid_ref_target(expr: &Expr) -> bool {
    matches!(expr, Expr::Identifier(_) | Expr::Member { .. })
        || matches!(expr, Expr::Parenthesized(inner) if is_valid_ref_target(inner))
}

/// Parentheses are normally erased from the executable AST, but they end an
/// OptionalChain.  Keep them around an optional suffix so the compiler can
/// distinguish `base?.value.more` from `(base?.value).more`.
fn optional_chain_expression(expr: &Expr) -> bool {
    match expr {
        Expr::OptionalMember { .. } | Expr::OptionalCall { .. } => true,
        Expr::Member { object, .. } | Expr::Call { callee: object, .. } => {
            optional_chain_expression(object)
        }
        _ => false,
    }
}

/// Grouping is not generally observable in the executable AST. It is kept
/// only while parsing an enclosing assignment so that AssignmentTargetType
/// can distinguish `(name)` from an IdentifierReference for name inference.
fn unparenthesize(expr: Expr) -> Expr {
    match expr {
        Expr::Parenthesized(expr) => unparenthesize(*expr),
        expr => expr,
    }
}

fn is_assignment_operator(token: &Token) -> bool {
    matches!(
        token,
        Token::Punct(
            Punct::Assign
                | Punct::PlusAssign
                | Punct::MinusAssign
                | Punct::StarAssign
                | Punct::SlashAssign
                | Punct::PercentAssign
                | Punct::ShiftLeftAssign
                | Punct::ShiftRightAssign
                | Punct::UnsignedShiftRightAssign
                | Punct::AndAssign
                | Punct::XorAssign
                | Punct::OrAssign
                | Punct::AndAndAssign
                | Punct::OrOrAssign
                | Punct::QuestionQuestionAssign
        )
    )
}

/// Annex B's optional web-compat extension recognizes only CallExpression
/// targets. In sloppy code they are evaluated and then throw ReferenceError;
/// strict code still rejects them during static semantics.
fn is_annex_b_call_assignment_target(expr: &Expr) -> bool {
    matches!(expr, Expr::Call { .. })
}

/// Converts an already-parsed expression into a `for-in`/`for-of` head.
/// Assignment references use the same target form as destructuring
/// assignments. Annex B preserves a CallExpression target so its observable
/// call happens before the web-compat runtime ReferenceError.
fn expr_to_for_head(expr: Expr) -> Result<ForHead, ParseError> {
    if is_valid_ref_target(&expr) {
        return Ok(ForHead::Assignment(AssignmentPattern::Target(Box::new(
            expr,
        ))));
    }
    if is_annex_b_call_assignment_target(&expr) {
        return Ok(ForHead::Expr(expr));
    }
    Err(ParseError {
        message: "invalid for-in/for-of assignment target".to_string(),
        resource: None,
        known_syntax: false,
    })
}

fn known_syntax(mut error: ParseError) -> ParseError {
    error.known_syntax = true;
    error
}

struct Parser {
    tokens: Vec<SpannedToken>,
    positions: Vec<usize>,
    tokenizer: Tokenizer,
    pos: usize,
    /// Set while parsing a `for`-loop head's init clause, so the
    /// relational-expression tier refuses to consume a bare `in` as a
    /// binary operator there -- see this module's doc comment.
    no_in: bool,
    generator_depth: u32,
    async_depth: u32,
    /// `await` is a keyword at the outermost level of the Module goal, but
    /// remains an IdentifierName in nested ordinary functions.
    module_await: bool,
    /// The Module goal remains in force below nested statements and functions
    /// even where `await` temporarily becomes an IdentifierName. Static
    /// import/export declarations are restricted to the ModuleItem list.
    module: bool,
    /// Parser contexts where IdentifierReference excludes strict-reserved
    /// words. This is activated by a script directive prologue or the Module
    /// goal so parse-only negative tests do not defer a mandated early error
    /// to the compiler.
    strict: bool,
    function_depth: u32,
    static_block_function_depths: Vec<u32>,
}

impl Parser {
    fn new(source: &str) -> Parser {
        Self::from_tokenizer(Tokenizer::new(source))
    }

    fn new_module(source: &str) -> Parser {
        Self::from_tokenizer(Tokenizer::new_module(source))
    }

    fn from_tokenizer(mut tokenizer: Tokenizer) -> Parser {
        let (tokens, positions) = tokenize_all(&mut tokenizer);
        Parser {
            tokens,
            positions,
            tokenizer,
            pos: 0,
            no_in: false,
            generator_depth: 0,
            async_depth: 0,
            module_await: false,
            module: false,
            strict: false,
            function_depth: 0,
            static_block_function_depths: Vec::new(),
        }
    }

    fn rescan_suffix(&mut self) {
        let (tokens, positions) = tokenize_all(&mut self.tokenizer);
        self.tokens.truncate(self.pos);
        self.positions.truncate(self.pos);
        self.tokens.extend(tokens);
        self.positions.extend(positions);
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx].token
    }

    fn newline_before(&self) -> bool {
        self.tokens[self.pos].newline_before
    }

    fn current_identifier_escaped(&self) -> bool {
        self.tokens[self.pos].identifier_escaped
    }

    /// Contextual `async` is a terminal symbol in the async-function and
    /// async-arrow productions. Its decoded spelling alone is insufficient:
    /// `\\u0061sync` must not select either production.
    fn require_unescaped_async(&self) -> Result<(), ParseError> {
        if self.current_identifier_escaped() {
            return Err(self.syntax_error("the async keyword cannot contain an escape"));
        }
        Ok(())
    }

    /// `AssignmentProperty : IdentifierReference Initializer_opt` is more
    /// restrictive than an object literal's PropertyName. In particular a
    /// keyword is legal as `{ keyword: target }` but cannot be a shorthand
    /// assignment target. The token retains whether its spelling was escaped
    /// so a decoded reserved word is rejected too.
    fn assignment_property_is_identifier_reference(&self) -> bool {
        let Token::Identifier(name) = self.peek() else {
            return false;
        };
        if name == "enum" {
            return false;
        }
        if matches!(
            name.as_str(),
            "class" | "debugger" | "export" | "extends" | "import" | "super" | "with"
        ) {
            return false;
        }
        if name == "yield" && (self.generator_depth != 0 || self.strict) {
            return false;
        }
        if name == "await" && (self.async_depth != 0 || self.module_await) {
            return false;
        }
        !(self.strict
            && matches!(
                name.as_str(),
                "implements"
                    | "interface"
                    | "let"
                    | "package"
                    | "private"
                    | "protected"
                    | "public"
                    | "static"
                    | "yield"
            ))
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].token.clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn check_punct(&self, p: Punct) -> bool {
        matches!(self.peek(), Token::Punct(pp) if *pp == p)
    }

    fn check_punct_at(&self, offset: usize, p: Punct) -> bool {
        matches!(self.peek_at(offset), Token::Punct(pp) if *pp == p)
    }

    fn check_keyword(&self, k: Keyword) -> bool {
        matches!(self.peek(), Token::Keyword(kk) if *kk == k)
    }

    fn check_identifier(&self, expected: &str) -> bool {
        matches!(self.peek(), Token::Identifier(name) if name == expected)
    }

    fn check_identifier_at(&self, offset: usize, expected: &str) -> bool {
        matches!(self.peek_at(offset), Token::Identifier(name) if name == expected)
    }

    fn eat_identifier(&mut self, expected: &str) -> bool {
        if self.check_identifier(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_punct(&mut self, p: Punct) -> bool {
        if self.check_punct(p) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_keyword(&mut self, k: Keyword) -> bool {
        if self.check_keyword(k) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, p: Punct) -> Result<(), ParseError> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            Err(self.error(format!("expected {p:?}")))
        }
    }

    fn expect_keyword(&mut self, k: Keyword) -> Result<(), ParseError> {
        if self.eat_keyword(k) {
            Ok(())
        } else {
            Err(self.error(format!("expected keyword {k:?}")))
        }
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        let known_syntax = matches!(self.peek(), Token::Invalid(message) if !message.contains("not supported") && !message.contains("unexpected character '#'"));
        ParseError {
            message: format!("{} (found {:?})", message.into(), self.peek()),
            resource: None,
            known_syntax,
        }
    }

    fn syntax_error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            message: format!("{} (found {:?})", message.into(), self.peek()),
            resource: None,
            known_syntax: true,
        }
    }

    fn expect_identifier_name(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            Token::Keyword(k) => {
                self.advance();
                Ok(keyword_as_str(k).to_string())
            }
            _ => Err(self.error("expected an identifier")),
        }
    }

    fn expect_member_name(&mut self) -> Result<String, ParseError> {
        if let Token::PrivateIdentifier(name) = self.peek().clone() {
            self.advance();
            return Ok(format!("#{name}"));
        }
        self.expect_identifier_name()
    }

    fn expect_binding_identifier(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.syntax_error("expected a binding identifier")),
        }
    }

    fn expect_module_name(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Token::String(name) => name
                .to_utf8()
                .map_err(|_| self.syntax_error("module specifier must be well-formed Unicode")),
            _ => Err(self.syntax_error("expected a module specifier string")),
        }
    }

    fn expect_module_export_name(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::String(name) => {
                self.advance();
                name.to_utf8().map_err(|_| {
                    self.syntax_error("module export name must be well-formed Unicode")
                })
            }
            _ => self.expect_identifier_name(),
        }
    }

    fn is_contextual_of(&self) -> bool {
        matches!(self.peek(), Token::Identifier(name) if name == "of")
    }
}

fn class_element_name(key: &PropertyKey) -> String {
    match key {
        PropertyKey::Identifier(name) => name.clone(),
        PropertyKey::String(name) => name.to_utf8().unwrap_or_default(),
        PropertyKey::Number(number) => number.to_string(),
        PropertyKey::Computed(_) => String::new(),
    }
}

fn parse_template(
    quasis: Vec<crate::JsString>,
    raw_expressions: Vec<String>,
) -> Result<Expr, ParseError> {
    let expressions = raw_expressions
        .iter()
        .map(|src| parse_expression_from_source(src))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Expr::Template {
        quasis,
        expressions,
    })
}
