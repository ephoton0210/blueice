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
//! -> logical AND -> equality -> relational -> additive ->
//! multiplicative -> unary -> postfix update (`++`/`--`) -> left-hand-
//! side (`new`/member/call chains) -> primary. ECMAScript §13.13 keeps
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
            known_syntax: false,
        }
    }
}

pub fn parse(source: &str) -> Result<Program, ParseError> {
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
                });
                positions.push(tokenizer.position());
                tokens.push(SpannedToken {
                    token: Token::Eof,
                    newline_before: false,
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

/// A plain identifier or member expression -- the only two shapes real
/// ECMAScript accepts as an assignment target or an `++`/`--` operand.
fn is_valid_ref_target(expr: &Expr) -> bool {
    matches!(expr, Expr::Identifier(_) | Expr::Member { .. })
}

/// Converts an already-parsed expression into a [`Pattern`], for the
/// no-declaration-keyword `for-in`/`for-of` head (`for (x of arr)`
/// where `x` was declared elsewhere). Restricted to a bare identifier
/// -- see [`ForHead`]'s own doc comment in `ast.rs` for why full
/// left-hand-side-expression/destructuring support here is an
/// intentional MVP cut, not an oversight.
fn expr_to_for_head_pattern(expr: Expr) -> Result<Pattern, ParseError> {
    match expr {
        Expr::Identifier(name) => Ok(Pattern::Identifier(name)),
        _ => Err(ParseError {
            message: "only a plain identifier is supported as a for-in/for-of target when no declaration keyword precedes it".to_string(),
            resource: None,
            known_syntax: false,
        }),
    }
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
    function_depth: u32,
    static_block_function_depths: Vec<u32>,
}

impl Parser {
    fn new(source: &str) -> Parser {
        let mut tokenizer = Tokenizer::new(source);
        let (tokens, positions) = tokenize_all(&mut tokenizer);
        Parser {
            tokens,
            positions,
            tokenizer,
            pos: 0,
            no_in: false,
            generator_depth: 0,
            async_depth: 0,
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

    fn check_keyword(&self, k: Keyword) -> bool {
        matches!(self.peek(), Token::Keyword(kk) if *kk == k)
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

    fn is_contextual_of(&self) -> bool {
        matches!(self.peek(), Token::Identifier(name) if name == "of")
    }

    /// Consumes a statement-terminating `;`, or applies automatic
    /// semicolon insertion (see `token.rs`'s [`SpannedToken`] doc
    /// comment): a `}`, EOF, or a preceding line terminator all count
    /// as an implicit semicolon, matching the common cases real
    /// hand-written scripts rely on (MVP scope doesn't need the full
    /// spec algorithm's edge cases, e.g. the restricted-token-list
    /// exceptions).
    fn consume_semicolon(&mut self) -> Result<(), ParseError> {
        if self.eat_punct(Punct::Semicolon) {
            return Ok(());
        }
        if self.check_punct(Punct::RBrace) || self.at_eof() || self.newline_before() {
            return Ok(());
        }
        Err(self.error("expected ';'"))
    }

    // ---- Statements ----

    fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().clone() {
            Token::Punct(Punct::Semicolon) => {
                self.advance();
                Ok(Stmt::Empty)
            }
            Token::Punct(Punct::LBrace) => Ok(Stmt::Block(self.parse_block()?)),
            Token::Keyword(Keyword::Var) => self.parse_var_decl_stmt(DeclKind::Var),
            Token::Keyword(Keyword::Let) => self.parse_var_decl_stmt(DeclKind::Let),
            Token::Keyword(Keyword::Const) => self.parse_var_decl_stmt(DeclKind::Const),
            Token::Keyword(Keyword::Function) => {
                self.advance();
                let f = self.parse_function()?;
                if f.name.is_none() {
                    return Err(self.error("function declarations require a name"));
                }
                if self.static_block_function_depths.last() == Some(&self.function_depth)
                    && f.name.as_deref() == Some("await")
                {
                    return Err(self.syntax_error(
                        "await cannot be bound by a function declaration in a class static block",
                    ));
                }
                Ok(Stmt::FunctionDecl(f))
            }
            Token::Identifier(name) if name == "async" && self.async_function_follows() => {
                self.advance();
                self.expect_keyword(Keyword::Function)?;
                let f = self.parse_function_with_async(true)?;
                if f.name.is_none() {
                    return Err(self.error("function declarations require a name"));
                }
                Ok(Stmt::FunctionDecl(f))
            }
            Token::Identifier(name) if name == "class" => {
                self.advance();
                let class = self.parse_class()?;
                if class.name.is_none() {
                    return Err(self.error("class declarations require a name"));
                }
                Ok(Stmt::ClassDecl(class))
            }
            Token::Identifier(name) if name == "with" => self.parse_with_stmt(),
            Token::Keyword(Keyword::If) => self.parse_if_stmt(),
            Token::Keyword(Keyword::For) => self.parse_for_stmt(),
            Token::Keyword(Keyword::While) => self.parse_while_stmt(),
            Token::Keyword(Keyword::Do) => self.parse_do_while_stmt(),
            Token::Keyword(Keyword::Switch) => self.parse_switch_stmt(),
            Token::Keyword(Keyword::Break) => self.parse_break_or_continue(false),
            Token::Keyword(Keyword::Continue) => self.parse_break_or_continue(true),
            Token::Keyword(Keyword::Return) => self.parse_return_stmt(),
            Token::Keyword(Keyword::Throw) => self.parse_throw_stmt(),
            Token::Keyword(Keyword::Try) => self.parse_try_stmt(),
            Token::Keyword(Keyword::Catch | Keyword::Finally) => {
                Err(self.syntax_error("catch/finally require a preceding try block"))
            }
            Token::Identifier(_) if matches!(self.peek_at(1), Token::Punct(Punct::Colon)) => {
                self.parse_labelled_stmt()
            }
            _ => {
                let expr = self.parse_expression()?;
                self.consume_semicolon()?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_break_or_continue(&mut self, is_continue: bool) -> Result<Stmt, ParseError> {
        self.advance();
        // A line terminator triggers ASI, so an identifier on the following
        // line begins the next statement rather than naming this transfer.
        let label = if !self.newline_before() {
            match self.peek().clone() {
                Token::Identifier(name) => {
                    self.advance();
                    Some(name)
                }
                _ => None,
            }
        } else {
            None
        };
        self.consume_semicolon()?;
        Ok(if is_continue {
            Stmt::Continue(label)
        } else {
            Stmt::Break(label)
        })
    }

    fn parse_labelled_stmt(&mut self) -> Result<Stmt, ParseError> {
        let label = self.expect_identifier_name()?;
        self.expect_punct(Punct::Colon)?;
        if label == "await"
            && self.static_block_function_depths.last() == Some(&self.function_depth)
        {
            return Err(
                self.syntax_error("await cannot be used as a label in a class static block")
            );
        }

        // In sloppy code, `let` may begin the labelled expression statement
        // `L: let` when ASI follows. It is not a lexical declaration there.
        // `let [` is the one prohibited lookahead form.
        let item = if self.check_keyword(Keyword::Let)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| token.newline_before)
        {
            self.advance();
            if self.check_punct(Punct::LBracket) {
                return Err(self.syntax_error("a labelled expression cannot begin with 'let ['"));
            }
            self.consume_semicolon()?;
            Stmt::Expr(Expr::Identifier("let".to_string()))
        } else {
            self.parse_statement()?
        };
        match &item {
            Stmt::VarDecl(kind, _) if *kind != DeclKind::Var => {
                return Err(
                    self.syntax_error("a labelled statement cannot contain a lexical declaration")
                );
            }
            Stmt::ClassDecl(_) => {
                return Err(
                    self.syntax_error("a labelled statement cannot contain a class declaration")
                )
            }
            Stmt::FunctionDecl(function) if function.generator || function.is_async => {
                return Err(self.syntax_error(
                    "a labelled statement cannot contain a generator or async function declaration",
                ));
            }
            _ => {}
        }
        Ok(Stmt::Labelled {
            label,
            item: Box::new(item),
        })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let mut stmts = Vec::new();
        while !self.check_punct(Punct::RBrace) {
            if self.at_eof() {
                return Err(self.error("unterminated block, expected '}'"));
            }
            stmts.push(self.parse_statement()?);
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(stmts)
    }

    fn parse_var_decl_stmt(&mut self, kind: DeclKind) -> Result<Stmt, ParseError> {
        self.advance();
        let declarators = self.parse_var_declarators()?;
        self.consume_semicolon()?;
        Ok(Stmt::VarDecl(kind, declarators))
    }

    fn parse_var_declarators(&mut self) -> Result<Vec<VarDeclarator>, ParseError> {
        let mut decls = Vec::new();
        loop {
            let pattern = self.parse_binding_pattern()?;
            let init = if self.eat_punct(Punct::Assign) {
                Some(self.parse_assignment()?)
            } else {
                None
            };
            decls.push(VarDeclarator { pattern, init });
            if !self.eat_punct(Punct::Comma) {
                break;
            }
        }
        Ok(decls)
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        self.expect_punct(Punct::LParen)?;
        let test = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        let consequent = Box::new(self.parse_statement()?);
        let alternate = if self.eat_keyword(Keyword::Else) {
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };
        Ok(Stmt::If {
            test,
            consequent,
            alternate,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        self.expect_punct(Punct::LParen)?;
        let test = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        let body = Box::new(self.parse_statement()?);
        Ok(Stmt::While { test, body })
    }

    fn parse_do_while_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        let body = Box::new(self.parse_statement()?);
        self.expect_keyword(Keyword::While)?;
        self.expect_punct(Punct::LParen)?;
        let test = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        // A trailing `;` after `do ... while (test)` is conventional
        // but the spec (and real scripts) tolerate its absence too;
        // ASI's `consume_semicolon` already accepts either.
        self.consume_semicolon()?;
        Ok(Stmt::DoWhile { body, test })
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        // Async functions accept `for await (...)`. The current AST uses the
        // ordinary ForOf shape because async execution is rejected before
        // bytecode generation; preserving the syntax keeps that rejection
        // correctly classified instead of reporting malformed source.
        if self.async_depth != 0
            && matches!(self.peek(), Token::Identifier(name) if name == "await")
        {
            self.advance();
        }
        self.expect_punct(Punct::LParen)?;

        if self.eat_punct(Punct::Semicolon) {
            return self.parse_for_rest(None);
        }

        let decl_kind = match self.peek() {
            Token::Keyword(Keyword::Var) => Some(DeclKind::Var),
            Token::Keyword(Keyword::Let) => Some(DeclKind::Let),
            Token::Keyword(Keyword::Const) => Some(DeclKind::Const),
            _ => None,
        };
        if let Some(decl_kind) = decl_kind {
            self.advance();
            let pattern = self.parse_binding_pattern()?;

            if self.eat_keyword(Keyword::In) {
                let right = self.parse_expression()?;
                self.expect_punct(Punct::RParen)?;
                let body = Box::new(self.parse_statement()?);
                return Ok(Stmt::ForIn {
                    left: ForHead::Decl(decl_kind, pattern),
                    right,
                    body,
                });
            }
            if self.is_contextual_of() {
                self.advance();
                let right = self.parse_assignment()?;
                self.expect_punct(Punct::RParen)?;
                let body = Box::new(self.parse_statement()?);
                return Ok(Stmt::ForOf {
                    left: ForHead::Decl(decl_kind, pattern),
                    right,
                    body,
                });
            }

            let mut declarators = vec![VarDeclarator {
                pattern,
                init: self.parse_optional_for_init_value()?,
            }];
            while self.eat_punct(Punct::Comma) {
                let pattern = self.parse_binding_pattern()?;
                declarators.push(VarDeclarator {
                    pattern,
                    init: self.parse_optional_for_init_value()?,
                });
            }
            self.expect_punct(Punct::Semicolon)?;
            return self.parse_for_rest(Some(ForInit::VarDecl(decl_kind, declarators)));
        }

        self.no_in = true;
        let expr = self.parse_expression();
        self.no_in = false;
        let expr = expr?;

        if self.eat_keyword(Keyword::In) {
            let left = expr_to_for_head_pattern(expr)?;
            let right = self.parse_expression()?;
            self.expect_punct(Punct::RParen)?;
            let body = Box::new(self.parse_statement()?);
            return Ok(Stmt::ForIn {
                left: ForHead::Pattern(left),
                right,
                body,
            });
        }
        if self.is_contextual_of() {
            self.advance();
            let left = expr_to_for_head_pattern(expr)?;
            let right = self.parse_assignment()?;
            self.expect_punct(Punct::RParen)?;
            let body = Box::new(self.parse_statement()?);
            return Ok(Stmt::ForOf {
                left: ForHead::Pattern(left),
                right,
                body,
            });
        }
        self.expect_punct(Punct::Semicolon)?;
        self.parse_for_rest(Some(ForInit::Expr(expr)))
    }

    /// `= <assignment expr>` with `in` disabled, or nothing -- shared by
    /// each declarator in a classic `for (let a = 1, b = 2; ...)` head.
    fn parse_optional_for_init_value(&mut self) -> Result<Option<Expr>, ParseError> {
        if !self.eat_punct(Punct::Assign) {
            return Ok(None);
        }
        self.no_in = true;
        let value = self.parse_assignment();
        self.no_in = false;
        Ok(Some(value?))
    }

    fn parse_for_rest(&mut self, init: Option<ForInit>) -> Result<Stmt, ParseError> {
        let test = if self.check_punct(Punct::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect_punct(Punct::Semicolon)?;
        let update = if self.check_punct(Punct::RParen) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect_punct(Punct::RParen)?;
        let body = Box::new(self.parse_statement()?);
        Ok(Stmt::For {
            init,
            test,
            update,
            body,
        })
    }

    fn parse_switch_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        self.expect_punct(Punct::LParen)?;
        let discriminant = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        self.expect_punct(Punct::LBrace)?;
        let mut cases = Vec::new();
        let mut saw_default = false;
        while !self.check_punct(Punct::RBrace) {
            let test = if self.eat_keyword(Keyword::Case) {
                let e = self.parse_expression()?;
                self.expect_punct(Punct::Colon)?;
                Some(e)
            } else {
                if saw_default {
                    return Err(
                        self.syntax_error("a switch statement can contain only one default clause")
                    );
                }
                self.expect_keyword(Keyword::Default)?;
                self.expect_punct(Punct::Colon)?;
                saw_default = true;
                None
            };
            let mut consequent = Vec::new();
            while !self.check_punct(Punct::RBrace)
                && !self.check_keyword(Keyword::Case)
                && !self.check_keyword(Keyword::Default)
            {
                if self.at_eof() {
                    return Err(self.error("unterminated switch statement, expected '}'"));
                }
                consequent.push(self.parse_statement()?);
            }
            cases.push(SwitchCase { test, consequent });
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Stmt::Switch {
            discriminant,
            cases,
        })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        if self.check_punct(Punct::Semicolon)
            || self.check_punct(Punct::RBrace)
            || self.at_eof()
            || self.newline_before()
        {
            self.consume_semicolon()?;
            return Ok(Stmt::Return(None));
        }
        let expr = self.parse_expression()?;
        self.consume_semicolon()?;
        Ok(Stmt::Return(Some(expr)))
    }

    fn parse_throw_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        if self.newline_before() {
            return Err(self.error("illegal line terminator between 'throw' and its expression"));
        }
        let expr = self.parse_expression()?;
        self.consume_semicolon()?;
        Ok(Stmt::Throw(expr))
    }

    fn parse_try_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        if !self.check_punct(Punct::LBrace) {
            return Err(self.syntax_error("try requires a block"));
        }
        let block = self.parse_block()?;
        let handler = if self.eat_keyword(Keyword::Catch) {
            let param = if self.eat_punct(Punct::LParen) {
                let p = self.parse_binding_pattern().map_err(known_syntax)?;
                self.expect_punct(Punct::RParen).map_err(known_syntax)?;
                if self.static_block_function_depths.last() == Some(&self.function_depth)
                    && matches!(&p, Pattern::Identifier(name) if name == "await")
                {
                    return Err(
                        self.syntax_error("await cannot be bound directly in a class static block")
                    );
                }
                Some(p)
            } else {
                None
            };
            if !self.check_punct(Punct::LBrace) {
                return Err(self.syntax_error("catch requires a block"));
            }
            Some(CatchClause {
                param,
                body: self.parse_block()?,
            })
        } else {
            None
        };
        let finalizer = if self.eat_keyword(Keyword::Finally) {
            if !self.check_punct(Punct::LBrace) {
                return Err(self.syntax_error("finally requires a block"));
            }
            Some(self.parse_block()?)
        } else {
            None
        };
        if handler.is_none() && finalizer.is_none() {
            return Err(self.syntax_error("'try' must be followed by 'catch', 'finally', or both"));
        }
        Ok(Stmt::Try {
            block,
            handler,
            finalizer,
        })
    }

    fn parse_with_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        self.expect_punct(Punct::LParen)?;
        let object = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        let body = self.parse_statement()?;
        Ok(Stmt::With {
            object,
            body: Box::new(body),
        })
    }

    // ---- Patterns ----

    fn parse_binding_pattern(&mut self) -> Result<Pattern, ParseError> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(Pattern::Identifier(name))
            }
            Token::Punct(Punct::LBracket) => self.parse_array_pattern(),
            Token::Punct(Punct::LBrace) => self.parse_object_pattern(),
            _ => Err(self.error("expected a binding target (identifier, '[', or '{')")),
        }
    }

    fn parse_array_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.expect_punct(Punct::LBracket)?;
        let mut elements = Vec::new();
        while !self.check_punct(Punct::RBracket) {
            if self.check_punct(Punct::Comma) {
                self.advance();
                elements.push(None);
                continue;
            }
            if self.eat_punct(Punct::Ellipsis) {
                let pattern = self.parse_binding_pattern()?;
                elements.push(Some(ArrayPatternElement {
                    pattern,
                    default: None,
                    rest: true,
                }));
                if !self.check_punct(Punct::RBracket) {
                    return Err(self.syntax_error("a binding rest element must be final"));
                }
                break;
            } else {
                let pattern = self.parse_binding_pattern()?;
                let default = if self.eat_punct(Punct::Assign) {
                    Some(self.parse_assignment()?)
                } else {
                    None
                };
                elements.push(Some(ArrayPatternElement {
                    pattern,
                    default,
                    rest: false,
                }));
            }
            if !self.check_punct(Punct::RBracket) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBracket)?;
        Ok(Pattern::Array(elements))
    }

    fn parse_object_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let mut props = Vec::new();
        while !self.check_punct(Punct::RBrace) {
            if self.eat_punct(Punct::Ellipsis) {
                props.push(ObjectPatternProp::Rest(self.parse_binding_pattern()?));
                if !self.check_punct(Punct::RBrace) {
                    return Err(self.syntax_error("a binding rest property must be final"));
                }
                break;
            } else {
                let key = self.parse_property_key()?;
                if self.eat_punct(Punct::Colon) {
                    let value = self.parse_binding_pattern()?;
                    let default = if self.eat_punct(Punct::Assign) {
                        Some(self.parse_assignment()?)
                    } else {
                        None
                    };
                    props.push(ObjectPatternProp::KeyValue {
                        key,
                        value,
                        default,
                    });
                } else {
                    let name = match &key {
                        PropertyKey::Identifier(n) => n.clone(),
                        _ => return Err(self.error("expected ':' in destructuring pattern")),
                    };
                    let default = if self.eat_punct(Punct::Assign) {
                        Some(self.parse_assignment()?)
                    } else {
                        None
                    };
                    props.push(ObjectPatternProp::KeyValue {
                        key: PropertyKey::Identifier(name.clone()),
                        value: Pattern::Identifier(name),
                        default,
                    });
                }
            }
            if !self.check_punct(Punct::RBrace) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Pattern::Object(props))
    }

    fn parse_property_key(&mut self) -> Result<PropertyKey, ParseError> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(PropertyKey::Identifier(name))
            }
            Token::Keyword(k) => {
                self.advance();
                Ok(PropertyKey::Identifier(keyword_as_str(k).to_string()))
            }
            Token::String(s) => {
                self.advance();
                Ok(PropertyKey::String(s))
            }
            Token::Number(n) => {
                self.advance();
                Ok(PropertyKey::Number(n))
            }
            Token::Punct(Punct::LBracket) => {
                self.advance();
                let expr = self.parse_assignment()?;
                self.expect_punct(Punct::RBracket)?;
                Ok(PropertyKey::Computed(Box::new(expr)))
            }
            _ => Err(self.error("expected a property key")),
        }
    }

    // ---- Functions ----

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect_punct(Punct::LParen)?;
        let mut params = Vec::new();
        while !self.check_punct(Punct::RParen) {
            if self.eat_punct(Punct::Ellipsis) {
                let pattern = self.parse_binding_pattern()?;
                if self.eat_punct(Punct::Assign) {
                    return Err(self.syntax_error("a rest parameter cannot have a default value"));
                }
                params.push(Param {
                    pattern,
                    default: None,
                    rest: true,
                });
                if self.eat_punct(Punct::Comma) {
                    return Err(self.syntax_error("a rest parameter cannot have a trailing comma"));
                }
                break;
            } else {
                let pattern = self.parse_binding_pattern()?;
                let default = if self.eat_punct(Punct::Assign) {
                    if self.async_depth != 0
                        && matches!(self.peek(), Token::Identifier(name) if name == "await")
                    {
                        return Err(self.syntax_error(
                            "await is not allowed in an async function parameter initializer",
                        ));
                    }
                    if self.generator_depth != 0
                        && matches!(self.peek(), Token::Identifier(name) if name == "yield")
                    {
                        return Err(self.syntax_error(
                            "yield is not allowed in a generator parameter initializer",
                        ));
                    }
                    Some(self.parse_assignment()?)
                } else {
                    None
                };
                params.push(Param {
                    pattern,
                    default,
                    rest: false,
                });
            }
            if !self.check_punct(Punct::RParen) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RParen)?;
        Ok(params)
    }

    /// Parses a `function` declaration/expression body -- the
    /// `function` keyword itself is already consumed by the caller,
    /// since callers need to branch on it first (statement vs.
    /// expression position).
    fn parse_function(&mut self) -> Result<Function, ParseError> {
        self.parse_function_with_async(false)
    }

    fn parse_function_with_async(&mut self, is_async: bool) -> Result<Function, ParseError> {
        let generator = self.eat_punct(Punct::Star);
        if matches!(self.peek(), Token::Invalid(message) if message.contains("unexpected character '#'"))
        {
            return Err(self.syntax_error("a function cannot have a private name"));
        }
        let name = if let Token::Identifier(_) = self.peek() {
            Some(self.expect_identifier_name()?)
        } else {
            None
        };
        if !self.check_punct(Punct::LParen) {
            return Err(self.syntax_error("a function parameter list must begin with '('"));
        }
        let function = self.parse_method_function(name, generator, is_async)?;
        if function_contains_super_call_outside_class(&function)
            || function_contains_super_property_outside_class(&function)
        {
            return Err(self.syntax_error("a normal function cannot contain super"));
        }
        Ok(function)
    }

    fn parse_method_function(
        &mut self,
        name: Option<String>,
        generator: bool,
        is_async: bool,
    ) -> Result<Function, ParseError> {
        let outer_async_depth = std::mem::replace(&mut self.async_depth, u32::from(is_async));
        let outer_generator_depth =
            std::mem::replace(&mut self.generator_depth, u32::from(generator));
        // `parse_params` also enters grammar that the subset may not yet
        // implement. Preserve an unclassified parse failure from that grammar;
        // explicit parameter early errors mark themselves as known syntax.
        let params = self.parse_params()?;
        self.function_depth += 1;
        let body = self.parse_block();
        self.function_depth -= 1;
        self.generator_depth = outer_generator_depth;
        self.async_depth = outer_async_depth;
        Ok(Function {
            name,
            params,
            body: body?,
            generator,
            is_async,
        })
    }

    fn parse_arrow_body(&mut self, is_async: bool) -> Result<ArrowBody, ParseError> {
        self.async_depth += u32::from(is_async);
        self.function_depth += 1;
        let body = if self.check_punct(Punct::LBrace) {
            self.parse_block().map(ArrowBody::Block)
        } else {
            self.parse_assignment()
                .map(|value| ArrowBody::Expr(Box::new(value)))
        };
        self.function_depth -= 1;
        self.async_depth -= u32::from(is_async);
        body
    }

    fn parse_class(&mut self) -> Result<Class, ParseError> {
        let name = if matches!(self.peek(), Token::Identifier(name) if name != "extends") {
            Some(self.expect_identifier_name()?)
        } else {
            None
        };
        let extends = if matches!(self.peek(), Token::Identifier(keyword) if keyword == "extends") {
            self.advance();
            Some(Box::new(self.parse_lhs_expression()?))
        } else {
            None
        };
        self.expect_punct(Punct::LBrace)?;
        let mut elements = Vec::new();
        let mut has_constructor = false;
        while !self.check_punct(Punct::RBrace) {
            if self.eat_punct(Punct::Semicolon) {
                continue;
            }
            let is_static = matches!(self.peek(), Token::Identifier(static_keyword) if static_keyword == "static")
                && !matches!(self.peek_at(1), Token::Punct(Punct::LParen));
            if is_static {
                self.advance();
            }
            if is_static && self.check_punct(Punct::LBrace) {
                self.static_block_function_depths.push(self.function_depth);
                let body = self.parse_block();
                self.static_block_function_depths.pop();
                let body = body?;
                if statements_contain_super_call_outside_class(&body) {
                    return Err(self.syntax_error("a static block cannot contain super()"));
                }
                elements.push(ClassElement::StaticBlock(body));
                continue;
            }
            let is_async = self.class_async_method_follows();
            if is_async {
                self.advance();
            }
            let accessor = match self.peek() {
                Token::Identifier(keyword)
                    if (keyword == "get" || keyword == "set")
                        && !matches!(self.peek_at(1), Token::Punct(Punct::LParen)) =>
                {
                    let getter = keyword == "get";
                    self.advance();
                    Some(getter)
                }
                _ => None,
            };
            let generator = self.eat_punct(Punct::Star);
            let key = self.parse_property_key()?;
            let method_name = class_element_name(&key);
            if !self.check_punct(Punct::LParen) {
                if generator || accessor.is_some() {
                    return Err(self.error("expected class method parameters"));
                }
                let initializer = if self.eat_punct(Punct::Assign) {
                    Some(self.parse_assignment()?)
                } else {
                    None
                };
                if initializer
                    .as_ref()
                    .is_some_and(expr_contains_super_call_outside_class)
                {
                    return Err(
                        self.syntax_error("a class field initializer cannot contain super()")
                    );
                }
                self.eat_punct(Punct::Semicolon);
                elements.push(ClassElement::Field {
                    key,
                    initializer,
                    is_static,
                });
                continue;
            }
            let function = self.parse_method_function(Some(method_name), generator, is_async)?;
            let constructor = accessor.is_none()
                && !is_static
                && !matches!(&key, PropertyKey::Computed(_))
                && class_element_name(&key) == "constructor";
            if function_contains_super_call_outside_class(&function)
                && (!constructor || extends.is_none())
            {
                return Err(self.syntax_error("super() is only valid in a derived constructor"));
            }
            if let Some(getter) = accessor {
                if is_async
                    || generator
                    || (getter && !function.params.is_empty())
                    || (!getter && (function.params.len() != 1 || function.params[0].rest))
                {
                    return Err(self.error("invalid class accessor parameter list"));
                }
                elements.push(ClassElement::Accessor {
                    key,
                    function,
                    getter,
                    is_static,
                });
            } else {
                if constructor {
                    if is_async || generator || has_constructor {
                        return Err(self.error("invalid class constructor"));
                    }
                    has_constructor = true;
                }
                elements.push(ClassElement::Method {
                    key,
                    function,
                    is_static,
                });
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Class {
            name,
            extends,
            elements,
        })
    }

    /// `async` is a contextual class-element modifier only when the next
    /// token starts a method without an intervening line terminator. Keeping
    /// `async = value` and `async() {}` as ordinary field/method names is
    /// essential for the class element grammar.
    fn class_async_method_follows(&self) -> bool {
        if !matches!(self.peek(), Token::Identifier(name) if name == "async")
            || self
                .tokens
                .get(self.pos + 1)
                .is_none_or(|token| token.newline_before)
        {
            return false;
        }
        match self.peek_at(1) {
            Token::Punct(Punct::Star) => true,
            Token::Identifier(_) | Token::Keyword(_) | Token::String(_) | Token::Number(_) => {
                matches!(self.peek_at(2), Token::Punct(Punct::LParen))
            }
            _ => false,
        }
    }

    fn async_function_follows(&self) -> bool {
        matches!(self.peek(), Token::Identifier(name) if name == "async")
            && self.tokens.get(self.pos + 1).is_some_and(|token| {
                !token.newline_before && matches!(token.token, Token::Keyword(Keyword::Function))
            })
    }

    fn async_arrow_follows(&self) -> bool {
        if !matches!(self.peek(), Token::Identifier(name) if name == "async")
            || self
                .tokens
                .get(self.pos + 1)
                .is_none_or(|token| token.newline_before)
        {
            return false;
        }
        match self.peek_at(1) {
            Token::Identifier(_) => matches!(self.peek_at(2), Token::Punct(Punct::Arrow)),
            Token::Punct(Punct::LParen) => match self.matching_close_paren(self.pos + 1) {
                Some(close) => matches!(self.tokens[close + 1].token, Token::Punct(Punct::Arrow)),
                None => false,
            },
            _ => false,
        }
    }

    /// Finds the index of the `)` matching the `(` at `open_idx`, or
    /// `None` if the input runs out first (malformed input; the
    /// eventual real parse of that `(...)` will surface its own,
    /// clearer error).
    fn matching_close_paren(&self, open_idx: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut i = open_idx;
        loop {
            match self.tokens.get(i)?.token {
                Token::Punct(Punct::LParen) => depth += 1,
                Token::Punct(Punct::RParen) => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                Token::Eof => return None,
                _ => {}
            }
            i += 1;
        }
    }

    /// Arrow functions (`x => ...` / `(a, b) => ...`) share
    /// `AssignmentExpression`'s grammar slot in real ECMAScript, so
    /// this is checked at the top of [`Parser::parse_assignment`]
    /// before falling through to the ordinary conditional-expression
    /// chain. Detecting the parenthesized-parameter-list form requires
    /// one token of lookahead past the matching `)` (to see whether
    /// `=>` follows) -- exactly what [`Parser::matching_close_paren`]
    /// exists for, since the token stream is fully materialized up
    /// front rather than a lazy/streaming lexer.
    fn try_parse_arrow_function(&mut self) -> Result<Option<Expr>, ParseError> {
        if self.async_arrow_follows() {
            self.advance();
            return self.try_parse_arrow_function_with_async(true);
        }
        self.try_parse_arrow_function_with_async(false)
    }

    fn try_parse_arrow_function_with_async(
        &mut self,
        is_async: bool,
    ) -> Result<Option<Expr>, ParseError> {
        if let Token::Identifier(name) = self.peek().clone() {
            if matches!(self.peek_at(1), Token::Punct(Punct::Arrow)) {
                self.advance();
                self.advance();
                let params = vec![Param {
                    pattern: Pattern::Identifier(name),
                    default: None,
                    rest: false,
                }];
                let body = self.parse_arrow_body(is_async)?;
                return Ok(Some(Expr::Arrow {
                    params,
                    body,
                    is_async,
                }));
            }
        }
        if self.check_punct(Punct::LParen) {
            if let Some(close_idx) = self.matching_close_paren(self.pos) {
                if matches!(
                    self.tokens.get(close_idx + 1).map(|t| &t.token),
                    Some(Token::Punct(Punct::Arrow))
                ) {
                    let params = self.parse_params()?;
                    self.expect_punct(Punct::Arrow)?;
                    let body = self.parse_arrow_body(is_async)?;
                    return Ok(Some(Expr::Arrow {
                        params,
                        body,
                        is_async,
                    }));
                }
            }
        }
        Ok(None)
    }

    // ---- Expressions ----

    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        // `Expression` is a left-associative sequence of assignment
        // expressions. Callers which require AssignmentExpression
        // (arguments, initializers, patterns, and conditional arms) stay on
        // `parse_assignment`, because commas delimit those productions.
        let first = self.parse_assignment()?;
        let mut expressions = vec![first];
        while self.eat_punct(Punct::Comma) {
            expressions.push(self.parse_assignment()?);
        }
        if expressions.len() == 1 {
            Ok(expressions.pop().unwrap())
        } else {
            Ok(Expr::Sequence(expressions))
        }
    }

    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        if let Some(arrow) = self.try_parse_arrow_function()? {
            return Ok(arrow);
        }
        if self.destructuring_assignment_ahead() {
            let pattern = self.parse_assignment_pattern()?;
            self.expect_punct(Punct::Assign)?;
            let value = self.parse_assignment()?;
            return Ok(Expr::DestructureAssign {
                pattern,
                value: Box::new(value),
            });
        }
        let left = self.parse_conditional()?;
        let op = match self.peek() {
            Token::Punct(Punct::Assign) => Some(AssignOp::Assign),
            Token::Punct(Punct::PlusAssign) => Some(AssignOp::AddAssign),
            Token::Punct(Punct::MinusAssign) => Some(AssignOp::SubAssign),
            Token::Punct(Punct::StarAssign) => Some(AssignOp::MulAssign),
            Token::Punct(Punct::SlashAssign) => Some(AssignOp::DivAssign),
            Token::Punct(Punct::PercentAssign) => Some(AssignOp::ModAssign),
            _ => None,
        };
        let Some(op) = op else {
            return Ok(left);
        };
        if !is_valid_ref_target(&left) {
            return Err(self.error("invalid assignment target"));
        }
        self.advance();
        let value = self.parse_assignment()?;
        Ok(Expr::Assign {
            op,
            target: Box::new(left),
            value: Box::new(value),
        })
    }

    /// A leading array/object cover grammar is a destructuring target only
    /// when its matching delimiter is immediately followed by `=`. This
    /// keeps ordinary literals on the regular expression path and lets
    /// parenthesized object assignment (`({x} = source)`) parse correctly.
    fn destructuring_assignment_ahead(&self) -> bool {
        if !matches!(self.peek(), Token::Punct(Punct::LBracket | Punct::LBrace)) {
            return false;
        }
        let mut delimiters = Vec::new();
        let mut index = self.pos;
        loop {
            match self.tokens[index].token {
                Token::Punct(Punct::LParen) => delimiters.push(Punct::RParen),
                Token::Punct(Punct::LBracket) => delimiters.push(Punct::RBracket),
                Token::Punct(Punct::LBrace) => delimiters.push(Punct::RBrace),
                Token::Punct(punct) if delimiters.last() == Some(&punct) => {
                    delimiters.pop();
                    if delimiters.is_empty() {
                        return matches!(
                            self.tokens.get(index + 1).map(|token| &token.token),
                            Some(Token::Punct(Punct::Assign))
                        );
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
            index += 1;
        }
    }

    fn parse_assignment_pattern(&mut self) -> Result<AssignmentPattern, ParseError> {
        match self.peek() {
            Token::Punct(Punct::LBracket) => self.parse_array_assignment_pattern(),
            Token::Punct(Punct::LBrace) => self.parse_object_assignment_pattern(),
            _ => {
                let target = self.parse_lhs_expression()?;
                if !is_valid_ref_target(&target) {
                    return Err(self.error("invalid destructuring assignment target"));
                }
                Ok(AssignmentPattern::Target(Box::new(target)))
            }
        }
    }

    fn parse_array_assignment_pattern(&mut self) -> Result<AssignmentPattern, ParseError> {
        self.expect_punct(Punct::LBracket)?;
        let mut elements = Vec::new();
        while !self.check_punct(Punct::RBracket) {
            if self.eat_punct(Punct::Comma) {
                elements.push(None);
                continue;
            }
            let rest = self.eat_punct(Punct::Ellipsis);
            let pattern = self.parse_assignment_pattern()?;
            let default = if rest {
                None
            } else if self.eat_punct(Punct::Assign) {
                Some(self.parse_assignment()?)
            } else {
                None
            };
            elements.push(Some(AssignmentPatternElement {
                pattern,
                default,
                rest,
            }));
            if rest && !self.check_punct(Punct::RBracket) {
                return Err(
                    self.error("a rest element must be last in a destructuring assignment pattern")
                );
            }
            if !self.check_punct(Punct::RBracket) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBracket)?;
        Ok(AssignmentPattern::Array(elements))
    }

    fn parse_object_assignment_pattern(&mut self) -> Result<AssignmentPattern, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let mut properties = Vec::new();
        while !self.check_punct(Punct::RBrace) {
            if self.eat_punct(Punct::Ellipsis) {
                properties.push(AssignmentPatternProp::Rest(
                    self.parse_assignment_pattern()?,
                ));
                if !self.check_punct(Punct::RBrace) {
                    return Err(self.error(
                        "a rest property must be last in a destructuring assignment pattern",
                    ));
                }
            } else {
                let key = self.parse_property_key()?;
                let (value, default) = if self.eat_punct(Punct::Colon) {
                    let value = self.parse_assignment_pattern()?;
                    let default = if self.eat_punct(Punct::Assign) {
                        Some(self.parse_assignment()?)
                    } else {
                        None
                    };
                    (value, default)
                } else {
                    let PropertyKey::Identifier(name) = &key else {
                        return Err(self.error("expected ':' in destructuring assignment pattern"));
                    };
                    let default = if self.eat_punct(Punct::Assign) {
                        Some(self.parse_assignment()?)
                    } else {
                        None
                    };
                    (
                        AssignmentPattern::Target(Box::new(Expr::Identifier(name.clone()))),
                        default,
                    )
                };
                properties.push(AssignmentPatternProp::KeyValue {
                    key,
                    value,
                    default,
                });
            }
            if !self.check_punct(Punct::RBrace) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(AssignmentPattern::Object(properties))
    }

    fn parse_conditional(&mut self) -> Result<Expr, ParseError> {
        let test = self.parse_nullish()?;
        if self.eat_punct(Punct::Question) {
            // `in` is allowed again between `?` and `:` even inside a
            // no-in `for`-head context (matches real ECMAScript
            // grammar: the no-in restriction doesn't propagate through
            // a parenthesized-equivalent sub-expression).
            let saved_no_in = self.no_in;
            self.no_in = false;
            let consequent = self.parse_assignment();
            self.no_in = saved_no_in;
            let consequent = consequent?;
            self.expect_punct(Punct::Colon)?;
            let alternate = self.parse_assignment()?;
            return Ok(Expr::Conditional {
                test: Box::new(test),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            });
        }
        Ok(test)
    }

    fn parse_nullish(&mut self) -> Result<Expr, ParseError> {
        let (mut left, logical) = self.parse_logical_or()?;
        while self.eat_punct(Punct::QuestionQuestion) {
            if logical {
                return Err(self.error("parentheses required when mixing ?? with && or ||"));
            }
            let (right, logical_right) = self.parse_logical_or()?;
            if logical_right {
                return Err(self.error("parentheses required when mixing ?? with && or ||"));
            }
            left = Expr::Logical {
                op: LogicalOp::Nullish,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_logical_or(&mut self) -> Result<(Expr, bool), ParseError> {
        let (mut left, mut logical) = self.parse_logical_and()?;
        while self.eat_punct(Punct::OrOr) {
            let (right, _) = self.parse_logical_and()?;
            left = Expr::Logical {
                op: LogicalOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
            logical = true;
        }
        Ok((left, logical))
    }

    fn parse_logical_and(&mut self) -> Result<(Expr, bool), ParseError> {
        let mut left = self.parse_equality()?;
        let mut logical = false;
        while self.eat_punct(Punct::AndAnd) {
            let right = self.parse_equality()?;
            left = Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
            logical = true;
        }
        Ok((left, logical))
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_relational()?;
        loop {
            let op = if self.check_punct(Punct::EqEq) {
                BinaryOp::Eq
            } else if self.check_punct(Punct::NotEq) {
                BinaryOp::NotEq
            } else if self.check_punct(Punct::EqEqEq) {
                BinaryOp::StrictEq
            } else if self.check_punct(Punct::NotEqEq) {
                BinaryOp::StrictNotEq
            } else {
                break;
            };
            self.advance();
            let right = self.parse_relational()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_relational(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive()?;
        loop {
            let op = if self.check_punct(Punct::Lt) {
                BinaryOp::Lt
            } else if self.check_punct(Punct::Gt) {
                BinaryOp::Gt
            } else if self.check_punct(Punct::LtEq) {
                BinaryOp::LtEq
            } else if self.check_punct(Punct::GtEq) {
                BinaryOp::GtEq
            } else if self.check_keyword(Keyword::Instanceof) {
                BinaryOp::Instanceof
            } else if self.check_keyword(Keyword::In) && !self.no_in {
                BinaryOp::In
            } else {
                break;
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = if self.check_punct(Punct::Plus) {
                BinaryOp::Add
            } else if self.check_punct(Punct::Minus) {
                BinaryOp::Sub
            } else {
                break;
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = if self.check_punct(Punct::Star) {
                BinaryOp::Mul
            } else if self.check_punct(Punct::Slash) {
                BinaryOp::Div
            } else if self.check_punct(Punct::Percent) {
                BinaryOp::Mod
            } else {
                break;
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.eat_punct(Punct::Bang) {
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                arg: Box::new(self.parse_unary()?),
            });
        }
        if self.eat_punct(Punct::Minus) {
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                arg: Box::new(self.parse_unary()?),
            });
        }
        if self.eat_punct(Punct::Plus) {
            return Ok(Expr::Unary {
                op: UnaryOp::Plus,
                arg: Box::new(self.parse_unary()?),
            });
        }
        if self.eat_keyword(Keyword::Typeof) {
            return Ok(Expr::Unary {
                op: UnaryOp::Typeof,
                arg: Box::new(self.parse_unary()?),
            });
        }
        if self.eat_keyword(Keyword::Void) {
            return Ok(Expr::Unary {
                op: UnaryOp::Void,
                arg: Box::new(self.parse_unary()?),
            });
        }
        if self.eat_keyword(Keyword::Delete) {
            return Ok(Expr::Unary {
                op: UnaryOp::Delete,
                arg: Box::new(self.parse_unary()?),
            });
        }
        if self.async_depth != 0
            && matches!(self.peek(), Token::Identifier(name) if name == "await")
        {
            self.advance();
            return Ok(Expr::Await(Box::new(self.parse_unary()?)));
        }
        if self.eat_punct(Punct::PlusPlus) {
            let arg = self.parse_unary()?;
            if !is_valid_ref_target(&arg) {
                return Err(self.error("invalid '++' operand"));
            }
            return Ok(Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(arg),
                prefix: true,
            });
        }
        if self.eat_punct(Punct::MinusMinus) {
            let arg = self.parse_unary()?;
            if !is_valid_ref_target(&arg) {
                return Err(self.error("invalid '--' operand"));
            }
            return Ok(Expr::Update {
                op: UpdateOp::Dec,
                arg: Box::new(arg),
                prefix: true,
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_lhs_expression()?;
        // Postfix `++`/`--` is forbidden across a line terminator
        // (ASI); see `token.rs`'s `SpannedToken` doc comment.
        if !self.newline_before() {
            if self.check_punct(Punct::PlusPlus) {
                if !is_valid_ref_target(&expr) {
                    return Err(self.error("invalid '++' operand"));
                }
                self.advance();
                return Ok(Expr::Update {
                    op: UpdateOp::Inc,
                    arg: Box::new(expr),
                    prefix: false,
                });
            }
            if self.check_punct(Punct::MinusMinus) {
                if !is_valid_ref_target(&expr) {
                    return Err(self.error("invalid '--' operand"));
                }
                self.advance();
                return Ok(Expr::Update {
                    op: UpdateOp::Dec,
                    arg: Box::new(expr),
                    prefix: false,
                });
            }
        }
        Ok(expr)
    }

    fn parse_lhs_expression(&mut self) -> Result<Expr, ParseError> {
        let mut expr = if self.check_keyword(Keyword::New)
            && matches!(self.peek_at(1), Token::Punct(Punct::Dot))
            && matches!(self.peek_at(2), Token::Identifier(name) if name == "target")
        {
            self.advance();
            self.advance();
            self.advance();
            Expr::NewTarget
        } else if self.eat_keyword(Keyword::New) {
            self.parse_new_expression()?
        } else {
            self.parse_primary()?
        };
        loop {
            if self.eat_punct(Punct::Dot) {
                let name = self.expect_identifier_name()?;
                expr = Expr::Member {
                    object: Box::new(expr),
                    property: Box::new(Expr::Identifier(name)),
                    computed: false,
                };
            } else if self.eat_punct(Punct::LBracket) {
                let prop = self.parse_expression()?;
                self.expect_punct(Punct::RBracket)?;
                expr = Expr::Member {
                    object: Box::new(expr),
                    property: Box::new(prop),
                    computed: true,
                };
            } else if self.check_punct(Punct::LParen) {
                let args = self.parse_arguments()?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.tokenizer.at_template(self.positions[self.pos]) {
                let (raw, cooked, sources) = self
                    .tokenizer
                    .tagged_template_at(self.positions[self.pos])?;
                self.rescan_suffix();
                let expressions = sources
                    .iter()
                    .map(|source| parse_expression_from_source(source))
                    .collect::<Result<Vec<_>, _>>()?;
                expr = Expr::TaggedTemplate {
                    tag: Box::new(expr),
                    raw,
                    cooked,
                    expressions,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    /// `new` already consumed by the caller. Real ECMAScript's
    /// `MemberExpression`/`NewExpression` mutual recursion is
    /// approximated here rather than reproduced exactly: the callee is
    /// a primary expression plus member accesses (`new a.b.c(...)`,
    /// `new (a.b)(...)`) but not a call (`new a()()` parses `a()` as
    /// the callee only via the outer loop picking the trailing `()` up
    /// afterward, at [`Parser::parse_lhs_expression`]'s level) --
    /// covers every realistic `new` usage a hand-written DOM script
    /// makes without needing the spec's full grammar distinction.
    fn parse_new_expression(&mut self) -> Result<Expr, ParseError> {
        let mut callee = if self.eat_keyword(Keyword::New) {
            self.parse_new_expression()?
        } else {
            self.parse_primary()?
        };
        loop {
            if self.eat_punct(Punct::Dot) {
                let name = self.expect_identifier_name()?;
                callee = Expr::Member {
                    object: Box::new(callee),
                    property: Box::new(Expr::Identifier(name)),
                    computed: false,
                };
            } else if self.eat_punct(Punct::LBracket) {
                let prop = self.parse_expression()?;
                self.expect_punct(Punct::RBracket)?;
                callee = Expr::Member {
                    object: Box::new(callee),
                    property: Box::new(prop),
                    computed: true,
                };
            } else {
                break;
            }
        }
        let args = if self.check_punct(Punct::LParen) {
            self.parse_arguments()?
        } else {
            Vec::new()
        };
        Ok(Expr::New {
            callee: Box::new(callee),
            args,
        })
    }

    fn parse_arguments(&mut self) -> Result<Vec<Argument>, ParseError> {
        self.expect_punct(Punct::LParen)?;
        let mut args = Vec::new();
        while !self.check_punct(Punct::RParen) {
            if self.eat_punct(Punct::Ellipsis) {
                args.push(Argument::Spread(self.parse_assignment()?));
            } else {
                args.push(Argument::Normal(self.parse_assignment()?));
            }
            if !self.check_punct(Punct::RParen) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RParen)?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            Token::Punct(Punct::Slash | Punct::SlashAssign) => {
                let (pattern, flags) = self.tokenizer.regexp_at(self.positions[self.pos])?;
                crate::regexp::RegExp::compile(pattern.clone(), &flags).map_err(|error| {
                    let resource = if matches!(error, crate::RuntimeError::SyntaxError(_)) {
                        None
                    } else {
                        Some(error.clone())
                    };
                    ParseError {
                        message: error.to_string(),
                        resource,
                        known_syntax: matches!(error, crate::RuntimeError::SyntaxError(_)),
                    }
                })?;
                self.rescan_suffix();
                Ok(Expr::RegExp { pattern, flags })
            }
            Token::Invalid(message) => Err(self.error(&message)),
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::String(s) => {
                self.advance();
                Ok(Expr::String(s))
            }
            Token::Template {
                quasis,
                raw_expressions,
            } => {
                self.advance();
                parse_template(quasis, raw_expressions)
            }
            Token::Keyword(Keyword::True) => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::Keyword(Keyword::False) => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Keyword(Keyword::Null) => {
                self.advance();
                Ok(Expr::Null)
            }
            Token::Keyword(Keyword::This) => {
                self.advance();
                Ok(Expr::This)
            }
            Token::Keyword(Keyword::Function) => {
                self.advance();
                Ok(Expr::Function(self.parse_function()?))
            }
            Token::Identifier(name) if name == "async" && self.async_function_follows() => {
                self.advance();
                self.expect_keyword(Keyword::Function)?;
                Ok(Expr::Function(self.parse_function_with_async(true)?))
            }
            Token::Identifier(name) if name == "class" => {
                self.advance();
                Ok(Expr::Class(self.parse_class()?))
            }
            Token::Identifier(name) if name == "super" => {
                self.advance();
                Ok(Expr::Super)
            }
            Token::Identifier(name) if name == "yield" && self.generator_depth != 0 => {
                self.advance();
                let delegate = self.eat_punct(Punct::Star);
                let value = if !delegate
                    && matches!(
                        self.peek(),
                        Token::Punct(Punct::Semicolon | Punct::RBrace) | Token::Eof
                    ) {
                    None
                } else {
                    Some(Box::new(self.parse_assignment()?))
                };
                Ok(Expr::Yield { value, delegate })
            }
            Token::Identifier(name) => {
                self.advance();
                Ok(Expr::Identifier(name))
            }
            Token::Punct(Punct::LParen) => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect_punct(Punct::RParen)?;
                Ok(expr)
            }
            Token::Punct(Punct::LBracket) => self.parse_array_literal(),
            Token::Punct(Punct::LBrace) => self.parse_object_literal(),
            _ => Err(self.error("expected an expression")),
        }
    }

    fn parse_array_literal(&mut self) -> Result<Expr, ParseError> {
        self.expect_punct(Punct::LBracket)?;
        let mut elements = Vec::new();
        while !self.check_punct(Punct::RBracket) {
            if self.check_punct(Punct::Comma) {
                self.advance();
                elements.push(None);
                continue;
            }
            if self.eat_punct(Punct::Ellipsis) {
                elements.push(Some(ArrayElement::Spread(self.parse_assignment()?)));
            } else {
                elements.push(Some(ArrayElement::Normal(self.parse_assignment()?)));
            }
            if !self.check_punct(Punct::RBracket) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBracket)?;
        Ok(Expr::Array(elements))
    }

    fn parse_object_literal(&mut self) -> Result<Expr, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let mut props = Vec::new();
        while !self.check_punct(Punct::RBrace) {
            if self.eat_punct(Punct::Ellipsis) {
                props.push(ObjectProp::Spread(self.parse_assignment()?));
            } else {
                let key = self.parse_property_key()?;
                if self.eat_punct(Punct::Colon) {
                    let value = self.parse_assignment()?;
                    props.push(ObjectProp::KeyValue {
                        key,
                        value,
                        shorthand: false,
                    });
                } else if self.check_punct(Punct::LParen) {
                    let params = self.parse_params()?;
                    let body = self.parse_block()?;
                    let name = match &key {
                        PropertyKey::Identifier(name) => name.clone(),
                        PropertyKey::String(name) => name.to_utf8().unwrap_or_default(),
                        PropertyKey::Number(number) => number.to_string(),
                        PropertyKey::Computed(_) => String::new(),
                    };
                    props.push(ObjectProp::Method {
                        key,
                        function: Function {
                            name: Some(name),
                            params,
                            body,
                            generator: false,
                            is_async: false,
                        },
                    });
                } else if matches!(&key, PropertyKey::Identifier(name) if name == "get" || name == "set")
                    && !self.check_punct(Punct::Comma)
                    && !self.check_punct(Punct::RBrace)
                {
                    let getter = matches!(&key, PropertyKey::Identifier(name) if name == "get");
                    let key = self.parse_property_key()?;
                    let params = self.parse_params()?;
                    if (getter && !params.is_empty())
                        || (!getter && (params.len() != 1 || params[0].rest))
                    {
                        return Err(self.error("invalid accessor parameter list"));
                    }
                    let body = self.parse_block()?;
                    let name = match &key {
                        PropertyKey::Identifier(name) => name.clone(),
                        PropertyKey::String(name) => name.to_utf8().unwrap_or_default(),
                        PropertyKey::Number(number) => number.to_string(),
                        PropertyKey::Computed(_) => String::new(),
                    };
                    let name = format!("{} {}", if getter { "get" } else { "set" }, name);
                    props.push(ObjectProp::Accessor {
                        key,
                        function: Function {
                            name: Some(name),
                            params,
                            body,
                            generator: false,
                            is_async: false,
                        },
                        getter,
                    });
                } else {
                    let name = match &key {
                        PropertyKey::Identifier(n) => n.clone(),
                        _ => return Err(self.error("expected ':' after object property key")),
                    };
                    props.push(ObjectProp::KeyValue {
                        key: PropertyKey::Identifier(name.clone()),
                        value: Expr::Identifier(name),
                        shorthand: true,
                    });
                }
            }
            if !self.check_punct(Punct::RBrace) {
                self.expect_punct(Punct::Comma)?;
            }
        }
        self.expect_punct(Punct::RBrace)?;
        Ok(Expr::Object(props))
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

#[cfg(test)]
mod tests {
    #[test]
    fn regexp_lexical_goals_are_visible_in_the_public_ast() {
        use crate::{parse, Expr, Stmt};
        let program = parse("/a/g").unwrap();
        assert!(
            matches!(&program.body[0],Stmt::Expr(Expr::RegExp {pattern,flags}) if pattern == "a" && flags == "g")
        );
        assert!(parse("delete object.x").is_ok());
        for source in ["/(/", "/a\n/", "String.raw`unterminated", "'\\u{110000}'"] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn advance_stops_at_the_terminal_token() {
        let mut parser = Parser::new("value");
        assert_eq!(parser.advance(), Token::Identifier("value".into()));
        assert!(parser.at_eof());
        assert_eq!(parser.advance(), Token::Eof);
        assert!(parser.at_eof());
    }

    #[test]
    fn contextual_async_and_class_lookahead_accept_the_supported_forms() {
        let mut with_statement = Parser::new("with({value:1})value");
        assert!(with_statement.parse_with_stmt().is_ok());

        let mut class = Parser::new("class C{async method(){} static async *items(){}}");
        class.advance();
        assert!(class.parse_class().is_ok());

        assert!(Parser::new("async function named(){}").async_function_follows());
        assert!(!Parser::new("async\nfunction named(){}").async_function_follows());
        assert!(Parser::new("async value=>value").async_arrow_follows());
        assert!(Parser::new("async (value)=>value").async_arrow_follows());
        assert!(!Parser::new("async value").async_arrow_follows());
        assert!(!Parser::new("async (value)").async_arrow_follows());
        assert!(!Parser::new("async (value").async_arrow_follows());
        assert!(!Parser::new("async").async_arrow_follows());
        assert!(!Parser::new("value").async_arrow_follows());
        assert!(!Parser::new("async\n(value)=>value").async_arrow_follows());
        assert_eq!(
            class_element_name(&PropertyKey::Computed(Box::new(Expr::Identifier(
                "key".into()
            )))),
            ""
        );
    }

    use super::*;

    fn program(src: &str) -> Program {
        parse(src).expect(src)
    }

    fn expr(src: &str) -> Expr {
        parse_expression_from_source(src).expect(src)
    }

    fn only_stmt(src: &str) -> Stmt {
        let p = program(src);
        assert_eq!(
            p.body.len(),
            1,
            "expected exactly one statement in {src:?}, got {:?}",
            p.body
        );
        p.body.into_iter().next().unwrap()
    }

    #[test]
    fn parses_literals() {
        assert_eq!(expr("42"), Expr::Number(42.0));
        assert_eq!(expr("\"hi\""), Expr::String("hi".into()));
        assert_eq!(expr("true"), Expr::Bool(true));
        assert_eq!(expr("false"), Expr::Bool(false));
        assert_eq!(expr("null"), Expr::Null);
        assert_eq!(expr("this"), Expr::This);
        assert_eq!(expr("undefined"), Expr::Identifier("undefined".to_string()));
        assert_eq!(expr("x"), Expr::Identifier("x".to_string()));
    }

    #[test]
    fn parses_template_literal_with_expressions() {
        assert_eq!(
            expr("`sum: ${a + b}!`"),
            Expr::Template {
                quasis: vec!["sum: ".into(), "!".into()],
                expressions: vec![Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::Identifier("a".to_string())),
                    right: Box::new(Expr::Identifier("b".to_string()))
                }]
            }
        );
        assert!(
            matches!(expr(r"tag`value: ${1}`"), Expr::TaggedTemplate { expressions, .. } if expressions == vec![Expr::Number(1.0)])
        );
    }

    #[test]
    fn operator_precedence_multiplicative_over_additive() {
        assert_eq!(
            expr("1 + 2 * 3"),
            Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Number(1.0)),
                right: Box::new(Expr::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(Expr::Number(2.0)),
                    right: Box::new(Expr::Number(3.0))
                })
            }
        );
    }

    #[test]
    fn operator_precedence_relational_over_equality() {
        assert_eq!(
            expr("a < b === c"),
            Expr::Binary {
                op: BinaryOp::StrictEq,
                left: Box::new(Expr::Binary {
                    op: BinaryOp::Lt,
                    left: Box::new(Expr::Identifier("a".to_string())),
                    right: Box::new(Expr::Identifier("b".to_string()))
                }),
                right: Box::new(Expr::Identifier("c".to_string())),
            }
        );
    }

    #[test]
    fn logical_and_binds_tighter_than_logical_or() {
        assert_eq!(
            expr("a || b && c"),
            Expr::Logical {
                op: LogicalOp::Or,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Logical {
                    op: LogicalOp::And,
                    left: Box::new(Expr::Identifier("b".to_string())),
                    right: Box::new(Expr::Identifier("c".to_string()))
                })
            }
        );
    }

    #[test]
    fn parses_nullish_coalescing_typeof_instanceof_in() {
        assert_eq!(
            expr("a ?? b"),
            Expr::Logical {
                op: LogicalOp::Nullish,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
        assert_eq!(
            expr("typeof x"),
            Expr::Unary {
                op: UnaryOp::Typeof,
                arg: Box::new(Expr::Identifier("x".to_string()))
            }
        );
        assert_eq!(
            expr("x instanceof Foo"),
            Expr::Binary {
                op: BinaryOp::Instanceof,
                left: Box::new(Expr::Identifier("x".to_string())),
                right: Box::new(Expr::Identifier("Foo".to_string()))
            }
        );
        assert_eq!(
            expr("'k' in obj"),
            Expr::Binary {
                op: BinaryOp::In,
                left: Box::new(Expr::String("k".into())),
                right: Box::new(Expr::Identifier("obj".to_string()))
            }
        );
    }

    #[test]
    fn parses_ternary_right_associative() {
        assert_eq!(
            expr("a ? b : c ? d : e"),
            Expr::Conditional {
                test: Box::new(Expr::Identifier("a".to_string())),
                consequent: Box::new(Expr::Identifier("b".to_string())),
                alternate: Box::new(Expr::Conditional {
                    test: Box::new(Expr::Identifier("c".to_string())),
                    consequent: Box::new(Expr::Identifier("d".to_string())),
                    alternate: Box::new(Expr::Identifier("e".to_string())),
                }),
            }
        );
    }

    #[test]
    fn parses_assignment_and_compound_assignment() {
        assert_eq!(
            expr("x = 1"),
            Expr::Assign {
                op: AssignOp::Assign,
                target: Box::new(Expr::Identifier("x".to_string())),
                value: Box::new(Expr::Number(1.0))
            }
        );
        assert_eq!(
            expr("x += 1"),
            Expr::Assign {
                op: AssignOp::AddAssign,
                target: Box::new(Expr::Identifier("x".to_string())),
                value: Box::new(Expr::Number(1.0))
            }
        );
        assert_eq!(
            expr("x -= 1"),
            Expr::Assign {
                op: AssignOp::SubAssign,
                target: Box::new(Expr::Identifier("x".to_string())),
                value: Box::new(Expr::Number(1.0))
            }
        );
        assert_eq!(
            expr("x *= 2"),
            Expr::Assign {
                op: AssignOp::MulAssign,
                target: Box::new(Expr::Identifier("x".to_string())),
                value: Box::new(Expr::Number(2.0))
            }
        );
        assert_eq!(
            expr("x /= 2"),
            Expr::Assign {
                op: AssignOp::DivAssign,
                target: Box::new(Expr::Identifier("x".to_string())),
                value: Box::new(Expr::Number(2.0))
            }
        );
        assert_eq!(
            expr("x %= 2"),
            Expr::Assign {
                op: AssignOp::ModAssign,
                target: Box::new(Expr::Identifier("x".to_string())),
                value: Box::new(Expr::Number(2.0))
            }
        );
        assert_eq!(
            expr("x = y = 1"),
            Expr::Assign {
                op: AssignOp::Assign,
                target: Box::new(Expr::Identifier("x".to_string())),
                value: Box::new(Expr::Assign {
                    op: AssignOp::Assign,
                    target: Box::new(Expr::Identifier("y".to_string())),
                    value: Box::new(Expr::Number(1.0))
                })
            }
        );
    }

    #[test]
    fn invalid_assignment_target_is_an_error() {
        assert!(parse_expression_from_source("1 = 2").is_err());
        assert!(parse_expression_from_source("(a + b) = 2").is_err());
        assert!(parse_expression_from_source("([1] = source)").is_err());
        assert!(parse_expression_from_source("[").is_err());
        assert!(parse_expression_from_source("([a, ...b, c] = source)").is_err());
        assert!(parse_expression_from_source("({a, ...rest, b} = source)").is_err());
    }

    #[test]
    fn parses_prefix_and_postfix_update_expressions() {
        assert_eq!(
            expr("++x"),
            Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(Expr::Identifier("x".to_string())),
                prefix: true
            }
        );
        assert_eq!(
            expr("x++"),
            Expr::Update {
                op: UpdateOp::Inc,
                arg: Box::new(Expr::Identifier("x".to_string())),
                prefix: false
            }
        );
        assert_eq!(
            expr("--x"),
            Expr::Update {
                op: UpdateOp::Dec,
                arg: Box::new(Expr::Identifier("x".to_string())),
                prefix: true
            }
        );
        assert_eq!(
            expr("x--"),
            Expr::Update {
                op: UpdateOp::Dec,
                arg: Box::new(Expr::Identifier("x".to_string())),
                prefix: false
            }
        );
    }

    #[test]
    fn postfix_update_is_suppressed_across_a_newline_asi() {
        // `x\n++y` is `x; ++y`, not `x++; y` -- ASI's restricted-token rule.
        let p = program("x\n++y");
        assert_eq!(
            p.body,
            vec![
                Stmt::Expr(Expr::Identifier("x".to_string())),
                Stmt::Expr(Expr::Update {
                    op: UpdateOp::Inc,
                    arg: Box::new(Expr::Identifier("y".to_string())),
                    prefix: true
                })
            ]
        );
    }

    #[test]
    fn invalid_update_operand_is_an_error() {
        assert!(parse_expression_from_source("1++").is_err());
        assert!(parse_expression_from_source("++1").is_err());
    }

    #[test]
    fn parses_member_and_computed_member_and_call_chains() {
        assert_eq!(
            expr("a.b[c](d)"),
            Expr::Call {
                callee: Box::new(Expr::Member {
                    object: Box::new(Expr::Member {
                        object: Box::new(Expr::Identifier("a".to_string())),
                        property: Box::new(Expr::Identifier("b".to_string())),
                        computed: false
                    }),
                    property: Box::new(Expr::Identifier("c".to_string())),
                    computed: true,
                }),
                args: vec![Argument::Normal(Expr::Identifier("d".to_string()))],
            }
        );
    }

    #[test]
    fn parses_new_expression_with_a_computed_member_callee() {
        assert_eq!(
            expr("new a[b]()"),
            Expr::New {
                callee: Box::new(Expr::Member {
                    object: Box::new(Expr::Identifier("a".to_string())),
                    property: Box::new(Expr::Identifier("b".to_string())),
                    computed: true
                }),
                args: vec![]
            }
        );
    }

    #[test]
    fn parses_new_expressions() {
        assert_eq!(
            expr("new Error(\"boom\")"),
            Expr::New {
                callee: Box::new(Expr::Identifier("Error".to_string())),
                args: vec![Argument::Normal(Expr::String("boom".into()))]
            }
        );
        assert_eq!(
            expr("new Foo"),
            Expr::New {
                callee: Box::new(Expr::Identifier("Foo".to_string())),
                args: vec![]
            }
        );
        assert_eq!(
            expr("new a.b.C()"),
            Expr::New {
                callee: Box::new(Expr::Member {
                    object: Box::new(Expr::Member {
                        object: Box::new(Expr::Identifier("a".to_string())),
                        property: Box::new(Expr::Identifier("b".to_string())),
                        computed: false
                    }),
                    property: Box::new(Expr::Identifier("C".to_string())),
                    computed: false,
                }),
                args: vec![],
            }
        );
        // A call immediately after `new Foo()` attaches to the `New`
        // node via the outer left-hand-side loop, not `parse_new_expression` itself.
        assert_eq!(
            expr("new Foo()()"),
            Expr::Call {
                callee: Box::new(Expr::New {
                    callee: Box::new(Expr::Identifier("Foo".to_string())),
                    args: vec![]
                }),
                args: vec![]
            }
        );
    }

    #[test]
    fn parses_call_with_spread_argument() {
        assert_eq!(
            expr("f(1, ...xs, 2)"),
            Expr::Call {
                callee: Box::new(Expr::Identifier("f".to_string())),
                args: vec![
                    Argument::Normal(Expr::Number(1.0)),
                    Argument::Spread(Expr::Identifier("xs".to_string())),
                    Argument::Normal(Expr::Number(2.0))
                ],
            }
        );
    }

    #[test]
    fn parses_array_literal_with_holes_and_spread_and_trailing_comma() {
        assert_eq!(
            expr("[1, 2, 3]"),
            Expr::Array(vec![
                Some(ArrayElement::Normal(Expr::Number(1.0))),
                Some(ArrayElement::Normal(Expr::Number(2.0))),
                Some(ArrayElement::Normal(Expr::Number(3.0)))
            ])
        );
        assert_eq!(
            expr("[1,,3]"),
            Expr::Array(vec![
                Some(ArrayElement::Normal(Expr::Number(1.0))),
                None,
                Some(ArrayElement::Normal(Expr::Number(3.0)))
            ])
        );
        assert_eq!(
            expr("[1, 2,]"),
            Expr::Array(vec![
                Some(ArrayElement::Normal(Expr::Number(1.0))),
                Some(ArrayElement::Normal(Expr::Number(2.0)))
            ])
        );
        assert_eq!(
            expr("[...xs]"),
            Expr::Array(vec![Some(ArrayElement::Spread(Expr::Identifier(
                "xs".to_string()
            )))])
        );
    }

    #[test]
    fn parses_object_literal_with_shorthand_computed_and_spread() {
        assert_eq!(
            expr("{a: 1, b, [c]: 2, ...rest}"),
            Expr::Object(vec![
                ObjectProp::KeyValue {
                    key: PropertyKey::Identifier("a".to_string()),
                    value: Expr::Number(1.0),
                    shorthand: false
                },
                ObjectProp::KeyValue {
                    key: PropertyKey::Identifier("b".to_string()),
                    value: Expr::Identifier("b".to_string()),
                    shorthand: true
                },
                ObjectProp::KeyValue {
                    key: PropertyKey::Computed(Box::new(Expr::Identifier("c".to_string()))),
                    value: Expr::Number(2.0),
                    shorthand: false
                },
                ObjectProp::Spread(Expr::Identifier("rest".to_string())),
            ])
        );
        assert!(matches!(
            expr("{get 'quoted'(){return 1},set 3(value){}}"),
            Expr::Object(properties)
                if matches!(
                    &properties[..],
                    [
                        ObjectProp::Accessor { function, getter: true, .. },
                        ObjectProp::Accessor { function: setter, getter: false, .. },
                    ] if function.name.as_deref() == Some("get quoted") && setter.name.as_deref() == Some("set 3")
                )
        ));
        assert!(matches!(
            only_stmt("({get 'quoted'(){return 1}})"),
            Stmt::Expr(Expr::Object(properties))
                if matches!(
                    &properties[..],
                    [ObjectProp::Accessor { function, getter: true, .. }]
                        if function.name.as_deref() == Some("get quoted")
                )
        ));
    }

    #[test]
    fn object_literal_used_as_a_statement_needs_parens_or_context() {
        // `{a: 1}` alone at statement position is a block containing a
        // labeled-looking statement in real JS; this parser doesn't
        // support labels, so bare `{a:1}` as a *statement* parses as a
        // block (consistent with the grammar ambiguity real engines
        // resolve the same way at statement position) -- confirming
        // object literals are unambiguous only in expression position,
        // e.g. wrapped in parens.
        assert_eq!(
            expr("({a: 1})"),
            Expr::Object(vec![ObjectProp::KeyValue {
                key: PropertyKey::Identifier("a".to_string()),
                value: Expr::Number(1.0),
                shorthand: false
            }])
        );
    }

    #[test]
    fn parses_function_declaration() {
        assert_eq!(
            only_stmt("function add(a, b) { return a + b; }"),
            Stmt::FunctionDecl(Function {
                name: Some("add".to_string()),
                params: vec![
                    Param {
                        pattern: Pattern::Identifier("a".to_string()),
                        default: None,
                        rest: false
                    },
                    Param {
                        pattern: Pattern::Identifier("b".to_string()),
                        default: None,
                        rest: false
                    }
                ],
                body: vec![Stmt::Return(Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::Identifier("a".to_string())),
                    right: Box::new(Expr::Identifier("b".to_string()))
                }))],
                generator: false,
                is_async: false,
            })
        );
    }

    #[test]
    fn anonymous_function_declaration_is_an_error() {
        assert!(parse("function (a) { return a; }").is_err());
    }

    #[test]
    fn parses_function_with_default_and_rest_params() {
        assert_eq!(
            expr("function f(a, b = 1, ...rest) {}"),
            Expr::Function(Function {
                name: Some("f".to_string()),
                params: vec![
                    Param {
                        pattern: Pattern::Identifier("a".to_string()),
                        default: None,
                        rest: false
                    },
                    Param {
                        pattern: Pattern::Identifier("b".to_string()),
                        default: Some(Expr::Number(1.0)),
                        rest: false
                    },
                    Param {
                        pattern: Pattern::Identifier("rest".to_string()),
                        default: None,
                        rest: true
                    },
                ],
                body: vec![],
                generator: false,
                is_async: false,
            })
        );
    }

    #[test]
    fn parses_arrow_functions_all_shapes() {
        assert_eq!(
            expr("x => x + 1"),
            Expr::Arrow {
                params: vec![Param {
                    pattern: Pattern::Identifier("x".to_string()),
                    default: None,
                    rest: false
                }],
                body: ArrowBody::Expr(Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::Identifier("x".to_string())),
                    right: Box::new(Expr::Number(1.0))
                })),
                is_async: false,
            }
        );
        assert_eq!(
            expr("() => {}"),
            Expr::Arrow {
                params: vec![],
                body: ArrowBody::Block(vec![]),
                is_async: false
            }
        );
        assert_eq!(
            expr("(a, b) => { return a + b; }"),
            Expr::Arrow {
                params: vec![
                    Param {
                        pattern: Pattern::Identifier("a".to_string()),
                        default: None,
                        rest: false
                    },
                    Param {
                        pattern: Pattern::Identifier("b".to_string()),
                        default: None,
                        rest: false
                    }
                ],
                body: ArrowBody::Block(vec![Stmt::Return(Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::Identifier("a".to_string())),
                    right: Box::new(Expr::Identifier("b".to_string()))
                }))]),
                is_async: false,
            }
        );
        assert!(matches!(
            expr("async value => await value"),
            Expr::Arrow { is_async: true, .. }
        ));
    }

    #[test]
    fn nested_parentheses_in_arrow_defaults_preserve_the_parameter_boundary() {
        // Drive the public parser; an inner ')' must not terminate arrow
        // lookahead before the actual parameter list's ')' and '=>'.
        assert_eq!(
            parse("(x=((1+2)*3))=>x").unwrap(),
            Program {
                body: vec![Stmt::Expr(Expr::Arrow {
                    params: vec![Param {
                        pattern: Pattern::Identifier("x".into()),
                        default: Some(Expr::Binary {
                            op: BinaryOp::Mul,
                            left: Box::new(Expr::Binary {
                                op: BinaryOp::Add,
                                left: Box::new(Expr::Number(1.0)),
                                right: Box::new(Expr::Number(2.0))
                            }),
                            right: Box::new(Expr::Number(3.0)),
                        }),
                        rest: false,
                    }],
                    body: ArrowBody::Expr(Box::new(Expr::Identifier("x".into()))),
                    is_async: false,
                })],
            }
        );
        assert!(parse("(x=((1+2)*3)=>x").is_err());
    }

    #[test]
    fn arrow_function_closes_over_outer_scope_syntactically() {
        // No runtime scoping to test here (that's the interpreter's
        // job, a separate checklist item) -- just that nested function
        // bodies parse as ordinary nested statement lists referencing
        // outer identifiers by name, which is all a closure needs
        // *syntactically*.
        assert_eq!(
            expr("(x) => () => x"),
            Expr::Arrow {
                params: vec![Param {
                    pattern: Pattern::Identifier("x".to_string()),
                    default: None,
                    rest: false
                }],
                body: ArrowBody::Expr(Box::new(Expr::Arrow {
                    params: vec![],
                    body: ArrowBody::Expr(Box::new(Expr::Identifier("x".to_string()))),
                    is_async: false
                })),
                is_async: false,
            }
        );
    }

    #[test]
    fn parses_destructuring_in_declarations_and_params() {
        assert_eq!(
            only_stmt("let [a, , b, ...rest] = arr;"),
            Stmt::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: Pattern::Array(vec![
                        Some(ArrayPatternElement {
                            pattern: Pattern::Identifier("a".to_string()),
                            default: None,
                            rest: false
                        }),
                        None,
                        Some(ArrayPatternElement {
                            pattern: Pattern::Identifier("b".to_string()),
                            default: None,
                            rest: false
                        }),
                        Some(ArrayPatternElement {
                            pattern: Pattern::Identifier("rest".to_string()),
                            default: None,
                            rest: true
                        }),
                    ]),
                    init: Some(Expr::Identifier("arr".to_string())),
                }],
            )
        );
        assert_eq!(
            only_stmt("const {a, b: renamed = 1, ...rest} = obj;"),
            Stmt::VarDecl(
                DeclKind::Const,
                vec![VarDeclarator {
                    pattern: Pattern::Object(vec![
                        ObjectPatternProp::KeyValue {
                            key: PropertyKey::Identifier("a".to_string()),
                            value: Pattern::Identifier("a".to_string()),
                            default: None
                        },
                        ObjectPatternProp::KeyValue {
                            key: PropertyKey::Identifier("b".to_string()),
                            value: Pattern::Identifier("renamed".to_string()),
                            default: Some(Expr::Number(1.0))
                        },
                        ObjectPatternProp::Rest(Pattern::Identifier("rest".to_string())),
                    ]),
                    init: Some(Expr::Identifier("obj".to_string())),
                }],
            )
        );
        assert_eq!(
            expr("function f([a, b]) {}"),
            Expr::Function(Function {
                name: Some("f".to_string()),
                params: vec![Param {
                    pattern: Pattern::Array(vec![
                        Some(ArrayPatternElement {
                            pattern: Pattern::Identifier("a".to_string()),
                            default: None,
                            rest: false
                        }),
                        Some(ArrayPatternElement {
                            pattern: Pattern::Identifier("b".to_string()),
                            default: None,
                            rest: false
                        }),
                    ]),
                    default: None,
                    rest: false,
                }],
                body: vec![],
                generator: false,
                is_async: false,
            })
        );
        assert!(matches!(
            expr("([a,,b=3,...rest]=source)"),
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Array(_),
                ..
            }
        ));
        assert!(matches!(
            expr("({a,b:c=2,...rest}=source)"),
            Expr::DestructureAssign {
                pattern: AssignmentPattern::Object(_),
                ..
            }
        ));
    }

    #[test]
    fn parses_var_let_const_with_multiple_declarators() {
        assert_eq!(
            only_stmt("var a = 1, b = 2;"),
            Stmt::VarDecl(
                DeclKind::Var,
                vec![
                    VarDeclarator {
                        pattern: Pattern::Identifier("a".to_string()),
                        init: Some(Expr::Number(1.0))
                    },
                    VarDeclarator {
                        pattern: Pattern::Identifier("b".to_string()),
                        init: Some(Expr::Number(2.0))
                    }
                ]
            )
        );
        assert_eq!(
            only_stmt("let x;"),
            Stmt::VarDecl(
                DeclKind::Let,
                vec![VarDeclarator {
                    pattern: Pattern::Identifier("x".to_string()),
                    init: None
                }]
            )
        );
    }

    #[test]
    fn parses_if_else() {
        assert_eq!(
            only_stmt("if (a) b; else c;"),
            Stmt::If {
                test: Expr::Identifier("a".to_string()),
                consequent: Box::new(Stmt::Expr(Expr::Identifier("b".to_string()))),
                alternate: Some(Box::new(Stmt::Expr(Expr::Identifier("c".to_string())))),
            }
        );
    }

    #[test]
    fn parses_while_and_do_while() {
        assert_eq!(
            only_stmt("while (a) b;"),
            Stmt::While {
                test: Expr::Identifier("a".to_string()),
                body: Box::new(Stmt::Expr(Expr::Identifier("b".to_string())))
            }
        );
        assert_eq!(
            only_stmt("do a; while (b);"),
            Stmt::DoWhile {
                body: Box::new(Stmt::Expr(Expr::Identifier("a".to_string()))),
                test: Expr::Identifier("b".to_string())
            }
        );
    }

    #[test]
    fn parses_classic_for_loop() {
        assert_eq!(
            only_stmt("for (let i = 0; i < 10; i++) {}"),
            Stmt::For {
                init: Some(ForInit::VarDecl(
                    DeclKind::Let,
                    vec![VarDeclarator {
                        pattern: Pattern::Identifier("i".to_string()),
                        init: Some(Expr::Number(0.0))
                    }]
                )),
                test: Some(Expr::Binary {
                    op: BinaryOp::Lt,
                    left: Box::new(Expr::Identifier("i".to_string())),
                    right: Box::new(Expr::Number(10.0))
                }),
                update: Some(Expr::Update {
                    op: UpdateOp::Inc,
                    arg: Box::new(Expr::Identifier("i".to_string())),
                    prefix: false
                }),
                body: Box::new(Stmt::Block(vec![])),
            }
        );
        // All three clauses empty.
        assert_eq!(
            only_stmt("for (;;) {}"),
            Stmt::For {
                init: None,
                test: None,
                update: None,
                body: Box::new(Stmt::Block(vec![]))
            }
        );
    }

    #[test]
    fn for_loop_with_existing_variable_does_not_misparse_in_as_a_binary_operator() {
        assert_eq!(
            only_stmt("for (i = 0; i < 10; i++) {}"),
            Stmt::For {
                init: Some(ForInit::Expr(Expr::Assign {
                    op: AssignOp::Assign,
                    target: Box::new(Expr::Identifier("i".to_string())),
                    value: Box::new(Expr::Number(0.0))
                })),
                test: Some(Expr::Binary {
                    op: BinaryOp::Lt,
                    left: Box::new(Expr::Identifier("i".to_string())),
                    right: Box::new(Expr::Number(10.0))
                }),
                update: Some(Expr::Update {
                    op: UpdateOp::Inc,
                    arg: Box::new(Expr::Identifier("i".to_string())),
                    prefix: false
                }),
                body: Box::new(Stmt::Block(vec![])),
            }
        );
    }

    #[test]
    fn parses_for_in_and_for_of() {
        assert_eq!(
            only_stmt("for (let k in obj) {}"),
            Stmt::ForIn {
                left: ForHead::Decl(DeclKind::Let, Pattern::Identifier("k".to_string())),
                right: Expr::Identifier("obj".to_string()),
                body: Box::new(Stmt::Block(vec![]))
            }
        );
        assert_eq!(
            only_stmt("for (const item of items) {}"),
            Stmt::ForOf {
                left: ForHead::Decl(DeclKind::Const, Pattern::Identifier("item".to_string())),
                right: Expr::Identifier("items".to_string()),
                body: Box::new(Stmt::Block(vec![]))
            }
        );
        assert_eq!(
            only_stmt("for (x of items) {}"),
            Stmt::ForOf {
                left: ForHead::Pattern(Pattern::Identifier("x".to_string())),
                right: Expr::Identifier("items".to_string()),
                body: Box::new(Stmt::Block(vec![]))
            }
        );
    }

    #[test]
    fn parses_switch_with_default() {
        assert_eq!(
            only_stmt("switch (x) { case 1: a; break; default: b; }"),
            Stmt::Switch {
                discriminant: Expr::Identifier("x".to_string()),
                cases: vec![
                    SwitchCase {
                        test: Some(Expr::Number(1.0)),
                        consequent: vec![
                            Stmt::Expr(Expr::Identifier("a".to_string())),
                            Stmt::Break(None)
                        ]
                    },
                    SwitchCase {
                        test: None,
                        consequent: vec![Stmt::Expr(Expr::Identifier("b".to_string()))]
                    },
                ],
            }
        );
    }

    #[test]
    fn duplicate_switch_default_is_a_known_syntax_error() {
        let error = parse("switch (value) { default: first; default: second; }").unwrap_err();
        assert!(error.known_syntax);
        assert!(error
            .message
            .starts_with("a switch statement can contain only one default clause"));
    }

    #[test]
    fn parses_try_catch_finally_and_requires_at_least_one() {
        assert_eq!(
            only_stmt("try { a; } catch (e) { b; } finally { c; }"),
            Stmt::Try {
                block: vec![Stmt::Expr(Expr::Identifier("a".to_string()))],
                handler: Some(CatchClause {
                    param: Some(Pattern::Identifier("e".to_string())),
                    body: vec![Stmt::Expr(Expr::Identifier("b".to_string()))]
                }),
                finalizer: Some(vec![Stmt::Expr(Expr::Identifier("c".to_string()))]),
            }
        );
        assert_eq!(
            only_stmt("try { a; } catch { b; }"),
            Stmt::Try {
                block: vec![Stmt::Expr(Expr::Identifier("a".to_string()))],
                handler: Some(CatchClause {
                    param: None,
                    body: vec![Stmt::Expr(Expr::Identifier("b".to_string()))]
                }),
                finalizer: None
            }
        );
        assert!(parse("try { a; }").is_err());
    }

    #[test]
    fn parses_throw_and_forbids_newline_before_its_expression() {
        assert_eq!(
            only_stmt("throw new Error(\"x\");"),
            Stmt::Throw(Expr::New {
                callee: Box::new(Expr::Identifier("Error".to_string())),
                args: vec![Argument::Normal(Expr::String("x".into()))]
            })
        );
        assert!(parse("throw\nnew Error(\"x\");").is_err());
    }

    #[test]
    fn automatic_semicolon_insertion_covers_the_common_cases() {
        let p = program("let a = 1\nlet b = 2");
        assert_eq!(p.body.len(), 2);
        // return with a newline before the value returns nothing, and
        // the value becomes its own separate expression statement.
        assert_eq!(
            only_stmt("function f() { return\n1; }"),
            Stmt::FunctionDecl(Function {
                name: Some("f".to_string()),
                params: vec![],
                body: vec![Stmt::Return(None), Stmt::Expr(Expr::Number(1.0))],
                generator: false,
                is_async: false
            })
        );
    }

    #[test]
    fn missing_semicolon_with_no_asi_opportunity_is_an_error() {
        assert!(parse("let a = 1 let b = 2").is_err());
    }

    #[test]
    fn dom_script_acceptance_bar_shaped_program_parses() {
        // Mirrors `phase-2-mvp-scope/PLAN.md`'s "Interactive-JS
        // acceptance bar" shape closely enough to prove this parser
        // covers what that bar needs syntactically (semantics are the
        // interpreter's job, not this crate's).
        let src = r#"
            var items = ["a", "b", "c"];
            for (var i = 0; i < items.length; i++) {
                var li = document.createElement("li");
                li.textContent = items[i];
                list.appendChild(li);
            }
            button.addEventListener("click", function () {
                el.style.display = el.style.display === "none" ? "block" : "none";
            });
        "#;
        let p = program(src);
        assert_eq!(p.body.len(), 3);
    }

    #[test]
    fn unexpected_token_in_expression_is_a_typed_error_not_a_panic() {
        assert!(parse("let x = ;").is_err());
        assert!(parse(")").is_err());
        assert!(parse("function").is_err());
    }

    #[test]
    fn bitwise_operators_are_rejected_as_out_of_scope_end_to_end() {
        assert!(parse("let x = a & b;").is_err());
    }

    #[test]
    fn parses_bare_semicolon_and_continue_statements() {
        assert_eq!(only_stmt(";"), Stmt::Empty);
        assert_eq!(only_stmt("continue;"), Stmt::Continue(None));
    }

    #[test]
    fn keywords_are_accepted_as_member_names_and_object_property_keys() {
        // `.default`/`.in`/etc. are ordinary property accesses in real
        // ECMAScript (keywords are only reserved as *identifiers*, not
        // as property names) -- exercises `keyword_as_str` broadly.
        assert_eq!(
            expr("obj.default"),
            Expr::Member {
                object: Box::new(Expr::Identifier("obj".to_string())),
                property: Box::new(Expr::Identifier("default".to_string())),
                computed: false
            }
        );
        assert_eq!(
            expr("obj.in"),
            Expr::Member {
                object: Box::new(Expr::Identifier("obj".to_string())),
                property: Box::new(Expr::Identifier("in".to_string())),
                computed: false
            }
        );
        assert_eq!(
            expr("obj.function"),
            Expr::Member {
                object: Box::new(Expr::Identifier("obj".to_string())),
                property: Box::new(Expr::Identifier("function".to_string())),
                computed: false
            }
        );
        assert_eq!(
            expr("{default: 1, case: 2, new: 3}"),
            Expr::Object(vec![
                ObjectProp::KeyValue {
                    key: PropertyKey::Identifier("default".to_string()),
                    value: Expr::Number(1.0),
                    shorthand: false
                },
                ObjectProp::KeyValue {
                    key: PropertyKey::Identifier("case".to_string()),
                    value: Expr::Number(2.0),
                    shorthand: false
                },
                ObjectProp::KeyValue {
                    key: PropertyKey::Identifier("new".to_string()),
                    value: Expr::Number(3.0),
                    shorthand: false
                },
            ])
        );
    }

    #[test]
    fn member_access_with_a_non_identifier_property_name_is_an_error() {
        assert!(parse_expression_from_source("a.1").is_err());
        assert!(parse_expression_from_source("a.").is_err());
    }

    #[test]
    fn for_of_target_that_is_not_a_plain_identifier_is_an_error_without_a_declaration_keyword() {
        assert!(parse("for (a.b of items) {}").is_err());
        assert!(parse("for (a.b in obj) {}").is_err());
    }

    #[test]
    fn do_while_missing_the_while_keyword_is_an_error() {
        assert!(parse("do a;").is_err());
    }

    #[test]
    fn unterminated_switch_statement_is_an_error() {
        assert!(parse("switch (x) { case 1: a;").is_err());
    }

    #[test]
    fn unterminated_block_is_an_error() {
        assert!(parse("function f() { let x = 1;").is_err());
    }

    #[test]
    fn object_literal_accepts_a_string_key() {
        assert_eq!(
            expr(r#"{"a-b": 1}"#),
            Expr::Object(vec![ObjectProp::KeyValue {
                key: PropertyKey::String("a-b".into()),
                value: Expr::Number(1.0),
                shorthand: false
            }])
        );
    }

    #[test]
    fn invalid_property_key_token_is_an_error() {
        assert!(parse_expression_from_source("({+: 1})").is_err());
    }

    #[test]
    fn parses_every_equality_relational_additive_and_multiplicative_operator() {
        assert_eq!(
            expr("a != b"),
            Expr::Binary {
                op: BinaryOp::NotEq,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
        assert_eq!(
            expr("a !== b"),
            Expr::Binary {
                op: BinaryOp::StrictNotEq,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
        assert_eq!(
            expr("a > b"),
            Expr::Binary {
                op: BinaryOp::Gt,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
        assert_eq!(
            expr("a <= b"),
            Expr::Binary {
                op: BinaryOp::LtEq,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
        assert_eq!(
            expr("a >= b"),
            Expr::Binary {
                op: BinaryOp::GtEq,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
        assert_eq!(
            expr("a - b"),
            Expr::Binary {
                op: BinaryOp::Sub,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
        assert_eq!(
            expr("a / b"),
            Expr::Binary {
                op: BinaryOp::Div,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
        assert_eq!(
            expr("a % b"),
            Expr::Binary {
                op: BinaryOp::Mod,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
    }

    #[test]
    fn parses_unary_not_neg_and_plus() {
        assert_eq!(
            expr("!a"),
            Expr::Unary {
                op: UnaryOp::Not,
                arg: Box::new(Expr::Identifier("a".to_string()))
            }
        );
        assert_eq!(
            expr("-a"),
            Expr::Unary {
                op: UnaryOp::Neg,
                arg: Box::new(Expr::Identifier("a".to_string()))
            }
        );
        assert_eq!(
            expr("+a"),
            Expr::Unary {
                op: UnaryOp::Plus,
                arg: Box::new(Expr::Identifier("a".to_string()))
            }
        );
        assert_eq!(
            expr("void a"),
            Expr::Unary {
                op: UnaryOp::Void,
                arg: Box::new(Expr::Identifier("a".to_string()))
            }
        );
    }

    #[test]
    fn invalid_decrement_operand_is_an_error_prefix_and_postfix() {
        assert!(parse_expression_from_source("--1").is_err());
        assert!(parse_expression_from_source("1--").is_err());
    }

    #[test]
    fn parses_loose_equality() {
        assert_eq!(
            expr("a == b"),
            Expr::Binary {
                op: BinaryOp::Eq,
                left: Box::new(Expr::Identifier("a".to_string())),
                right: Box::new(Expr::Identifier("b".to_string()))
            }
        );
    }

    #[test]
    fn an_unclosed_grouping_paren_that_runs_out_of_input_is_an_error() {
        // Exercises `matching_close_paren` hitting EOF before a match,
        // not just its happy-path return.
        assert!(parse_expression_from_source("(1 + 2").is_err());
    }

    #[test]
    fn invalid_binding_pattern_target_is_an_error() {
        assert!(parse("let 5 = x;").is_err());
    }

    #[test]
    fn binding_rest_elements_and_properties_must_be_final() {
        for source in [
            "try {} catch ([...rest, next]) {}",
            "try {} catch ([...{value}, next]) {}",
            "try {} catch ([...rest,]) {}",
            "try {} catch ({...rest, next}) {}",
            "try {} catch ({...rest,}) {}",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn destructuring_object_pattern_with_a_computed_key_requires_a_colon() {
        // A computed key (`[expr]`) can never be a valid shorthand
        // binding on its own -- there's no identifier to shorthand to.
        assert!(parse("let {[k]} = obj;").is_err());
    }

    #[test]
    fn object_literal_shorthand_requires_an_identifier_shaped_key() {
        // A computed or numeric key with no value and no ':' has no
        // identifier to shorthand to either.
        assert!(parse_expression_from_source("({[k]})").is_err());
        assert!(parse_expression_from_source("({1})").is_err());
    }

    #[test]
    fn classic_for_loop_supports_multiple_declarators_and_an_omitted_initializer() {
        assert_eq!(
            only_stmt("for (let i = 0, j = 10; i < j; i++) {}"),
            Stmt::For {
                init: Some(ForInit::VarDecl(
                    DeclKind::Let,
                    vec![
                        VarDeclarator {
                            pattern: Pattern::Identifier("i".to_string()),
                            init: Some(Expr::Number(0.0))
                        },
                        VarDeclarator {
                            pattern: Pattern::Identifier("j".to_string()),
                            init: Some(Expr::Number(10.0))
                        },
                    ]
                )),
                test: Some(Expr::Binary {
                    op: BinaryOp::Lt,
                    left: Box::new(Expr::Identifier("i".to_string())),
                    right: Box::new(Expr::Identifier("j".to_string()))
                }),
                update: Some(Expr::Update {
                    op: UpdateOp::Inc,
                    arg: Box::new(Expr::Identifier("i".to_string())),
                    prefix: false
                }),
                body: Box::new(Stmt::Block(vec![])),
            }
        );
        // No initializer at all on the declarator.
        assert_eq!(
            only_stmt("for (let i; i < 10; i++) {}"),
            Stmt::For {
                init: Some(ForInit::VarDecl(
                    DeclKind::Let,
                    vec![VarDeclarator {
                        pattern: Pattern::Identifier("i".to_string()),
                        init: None
                    }]
                )),
                test: Some(Expr::Binary {
                    op: BinaryOp::Lt,
                    left: Box::new(Expr::Identifier("i".to_string())),
                    right: Box::new(Expr::Number(10.0))
                }),
                update: Some(Expr::Update {
                    op: UpdateOp::Inc,
                    arg: Box::new(Expr::Identifier("i".to_string())),
                    prefix: false
                }),
                body: Box::new(Stmt::Block(vec![])),
            }
        );
    }

    #[test]
    fn for_in_without_a_declaration_keyword_uses_the_existing_variable() {
        assert_eq!(
            only_stmt("for (k in obj) {}"),
            Stmt::ForIn {
                left: ForHead::Pattern(Pattern::Identifier("k".to_string())),
                right: Expr::Identifier("obj".to_string()),
                body: Box::new(Stmt::Block(vec![]))
            }
        );
    }

    #[test]
    fn template_placeholder_with_trailing_tokens_is_an_error() {
        // `${1 2}` has no valid single-expression parse: `1` consumes
        // the whole expression grammar, leaving `2` as unexpected
        // trailing input -- surfaced through `parse_expression_from_source`.
        assert!(parse_expression_from_source("`${1 2}`").is_err());
    }

    #[test]
    fn a_grouping_parenthesized_expression_is_not_mistaken_for_arrow_params() {
        // Exercises `matching_close_paren` finding a real match with no
        // trailing `=>`, so the parenthesized form falls through to an
        // ordinary grouped expression instead.
        assert_eq!(
            expr("(1 + 2) * 3"),
            Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::Number(1.0)),
                    right: Box::new(Expr::Number(2.0))
                }),
                right: Box::new(Expr::Number(3.0))
            }
        );
    }
}
