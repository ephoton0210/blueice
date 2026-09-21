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
//! `token.rs`'s own doc comment for why), and [`Parser::parse_template`]
//! below re-tokenizes/re-parses each one independently via
//! [`Parser::parse_template_placeholder`] -- a fresh [`Parser`] over just that
//! substring (inheriting the enclosing function's generator/async/strict
//! context), required to consume it entirely as one expression.

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
    let mut parser = Parser::new_script(source);
    let mut body = Vec::new();
    let mut prologue = DirectivePrologue::default();
    while !parser.at_eof() {
        body.push(parser.parse_prologue_statement(&mut prologue)?);
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
/// but a direct eval inherits the calling method's `[[HomeObject]]`. Eval
/// code is strict when the calling code is (`strict`) or when its own
/// directive prologue says so.
pub(crate) fn parse_eval(source: &str, strict: bool) -> Result<Program, ParseError> {
    let mut parser = Parser::new_script(source);
    parser.strict = strict;
    let mut body = Vec::new();
    let mut prologue = DirectivePrologue::default();
    while !parser.at_eof() {
        body.push(parser.parse_prologue_statement(&mut prologue)?);
    }
    Ok(Program { body })
}

/// State of the Directive Prologue (§11.2.1) while a Script, eval code or a
/// function body is parsed statement by statement.
struct DirectivePrologue {
    /// Every statement so far has been an ExpressionStatement consisting
    /// solely of a string literal.
    open: bool,
    /// A directive so far used a legacy octal or non-octal decimal escape,
    /// which a later Use Strict Directive makes an early error.
    legacy_octal: bool,
}

impl Default for DirectivePrologue {
    fn default() -> Self {
        DirectivePrologue {
            open: true,
            legacy_octal: false,
        }
    }
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
                    legacy_octal_escape: false,
                    string_escaped: false,
                });
                positions.push(tokenizer.position());
                tokens.push(SpannedToken {
                    token: Token::Eof,
                    newline_before: false,
                    identifier_escaped: false,
                    legacy_octal_escape: false,
                    string_escaped: false,
                });
                return (tokens, positions);
            }
        }
    }
}

/// Parses `source` as one standalone expression, to exercise expression
/// parsing in tests without wrapping every fixture in a statement.
#[cfg(test)]
fn parse_expression_from_source(source: &str) -> Result<Expr, ParseError> {
    let mut parser = Parser::new(source);
    let expr = parser.parse_expression()?;
    if !parser.at_eof() {
        return Err(parser.error("unexpected trailing tokens after expression"));
    }
    Ok(expr)
}

pub(crate) fn closes_template_placeholder(source: &str) -> bool {
    // The placeholder is parsed again inside its enclosing function, where
    // `yield` and `await` may be operators. Its closing brace is the same in
    // every such context, so accept a candidate that parses in any of them.
    [(0, 0), (1, 0), (0, 1)]
        .into_iter()
        .any(|(generator_depth, async_depth)| {
            let mut parser = Parser::new(source);
            parser.generator_depth = generator_depth;
            parser.async_depth = async_depth;
            parser.parse_expression().is_ok() && parser.eat_punct(Punct::RBrace) && parser.at_eof()
        })
}

/// The words reserved only in strict mode code (§13.1.1), which the tokenizer
/// keeps as plain identifiers.
fn is_strict_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
    )
}

/// Whether a function body's Directive Prologue holds a Use Strict Directive.
/// Only a bare string statement is a directive: the parser wraps a
/// `use strict`-valued statement that is not one in `Expr::Parenthesized`.
fn function_body_has_use_strict(body: &[Stmt]) -> bool {
    body.iter()
        .take_while(|stmt| matches!(stmt, Stmt::Expr(Expr::String(_))))
        .any(|stmt| matches!(stmt, Stmt::Expr(Expr::String(value)) if value == "use strict"))
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

    /// A parser over complete Script source text, the only place a Hashbang
    /// comment (`#!...`) may begin. Template placeholders and other
    /// substring parses use [`Parser::new`], where `#!` stays an error.
    fn new_script(source: &str) -> Parser {
        let mut tokenizer = Tokenizer::new(source);
        tokenizer.skip_hashbang();
        Self::from_tokenizer(tokenizer)
    }

    fn new_module(source: &str) -> Parser {
        let mut tokenizer = Tokenizer::new_module(source);
        tokenizer.skip_hashbang();
        Self::from_tokenizer(tokenizer)
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
        match self.peek() {
            Token::Identifier(name) => self.identifier_reference_name_is_valid(name),
            // `let` stays a valid IdentifierReference in sloppy code.
            Token::Keyword(Keyword::Let) => self.identifier_reference_name_is_valid("let"),
            _ => false,
        }
    }

    /// Whether `name` may be an IdentifierReference here (§13.1.1). Besides
    /// the ReservedWords, the strict-mode reserved words and the `yield` /
    /// `await` context rules apply. The keywords the tokenizer keeps as
    /// [`Keyword`] tokens can only arrive here as a property-name string
    /// (`({ true })`) or as an escaped spelling, and are reserved too.
    fn identifier_reference_name_is_valid(&self, name: &str) -> bool {
        if Keyword::from_str(name).is_some_and(|keyword| keyword != Keyword::Let) {
            return false;
        }
        if matches!(
            name,
            "class" | "debugger" | "enum" | "export" | "extends" | "import" | "super" | "with"
        ) {
            return false;
        }
        if name == "yield" && (self.generator_depth != 0 || self.strict) {
            return false;
        }
        if name == "await"
            && (self.async_depth != 0
                || self.module_await
                || self.static_block_function_depths.last() == Some(&self.function_depth))
        {
            return false;
        }
        !(self.strict && is_strict_reserved_word(name))
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

    /// Consumes a contextual keyword (`as`, `from`, `with`, ...). It is a
    /// terminal symbol of its production, so a spelling containing a Unicode
    /// escape cannot stand in for it: that is a SyntaxError, except when a
    /// preceding line terminator lets ASI end the statement first (the escaped
    /// word then starts the next statement as an ordinary identifier).
    fn eat_contextual_keyword(&mut self, expected: &str) -> Result<bool, ParseError> {
        if !self.check_identifier(expected) {
            return Ok(false);
        }
        if self.current_identifier_escaped() {
            if self.newline_before() {
                return Ok(false);
            }
            return Err(
                self.syntax_error(format!("the {expected} keyword cannot contain an escape"))
            );
        }
        self.advance();
        Ok(true)
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

    /// A token the grammar cannot accept at this point. Every production the
    /// parser implements is complete for its goal, so a token that fails to
    /// match a mandatory position is a specified SyntaxError. Only a lexical
    /// placeholder for a construct this engine does not scan (see
    /// `Token::Invalid`) stays unclassified, because the source may be valid.
    fn error(&self, message: impl Into<String>) -> ParseError {
        let known_syntax = !matches!(self.peek(), Token::Invalid(message) if message.contains("not supported") || message.contains("unexpected character '#'"));
        ParseError {
            message: format!("{} (found {:?})", message.into(), self.peek()),
            resource: None,
            known_syntax,
        }
    }

    /// Legacy octal and non-octal decimal escapes are Annex B extensions.
    /// The tokenizer records their use because the surrounding syntactic goal
    /// determines whether they are permitted.
    fn reject_legacy_octal_escape(&self) -> Result<(), ParseError> {
        if self.strict && self.tokens[self.pos].legacy_octal_escape {
            return Err(self.syntax_error(
                "legacy octal and non-octal decimal literals and escapes are not valid in strict mode",
            ));
        }
        Ok(())
    }

    /// Parses one statement of a body that begins with a Directive
    /// Prologue, turning strict mode on at a Use Strict Directive. Only an
    /// unparenthesized string-literal statement spelled exactly `use strict`
    /// (no escape, no line continuation) is that directive.
    ///
    /// The compiler recognizes a directive by the cooked value of the leading
    /// string statements alone. A `use strict`-valued statement that is not
    /// a directive (`('use strict')`, `'use\u0020strict'`) is therefore kept
    /// in the AST wrapped in an `Expr::Parenthesized`, which evaluates the
    /// same but is no longer a bare string statement.
    fn parse_prologue_statement(
        &mut self,
        prologue: &mut DirectivePrologue,
    ) -> Result<Stmt, ParseError> {
        let start = self.pos;
        let statement = self.parse_statement()?;
        if !prologue.open {
            return Ok(statement);
        }
        let token = &self.tokens[start];
        let is_use_strict_valued =
            matches!(&statement, Stmt::Expr(Expr::String(value)) if value == "use strict");
        let is_directive = matches!(&statement, Stmt::Expr(Expr::String(_)))
            && matches!(token.token, Token::String(_))
            && (self.pos == start + 1
                || (self.pos == start + 2
                    && matches!(self.tokens[start + 1].token, Token::Punct(Punct::Semicolon))));
        if !is_directive {
            prologue.open = false;
        } else {
            prologue.legacy_octal |= token.legacy_octal_escape;
        }
        if !is_use_strict_valued {
            return Ok(statement);
        }
        if !is_directive || token.string_escaped {
            let Stmt::Expr(expression) = statement else {
                unreachable!("a use strict-valued statement is an expression statement")
            };
            return Ok(Stmt::Expr(Expr::Parenthesized(Box::new(expression))));
        }
        if prologue.legacy_octal {
            return Err(ParseError {
                message: "legacy octal and non-octal decimal escapes cannot precede a \
                          Use Strict Directive"
                    .to_string(),
                resource: None,
                known_syntax: true,
            });
        }
        self.strict = true;
        Ok(statement)
    }

    /// `{ FunctionBody }` of a function, method, accessor or arrow: a
    /// statement list with its own Directive Prologue, whose strictness ends
    /// with the body.
    pub(super) fn parse_function_body(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.with_in_allowed(Self::parse_function_body_in)
    }

    fn parse_function_body_in(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect_punct(Punct::LBrace)?;
        let outer_strict = self.strict;
        let mut prologue = DirectivePrologue::default();
        let mut statements = Vec::new();
        let result = loop {
            if self.check_punct(Punct::RBrace) {
                break self.expect_punct(Punct::RBrace);
            }
            if self.at_eof() {
                break Err(self.error("unterminated block, expected '}'"));
            }
            match self.parse_prologue_statement(&mut prologue) {
                Ok(statement) => statements.push(statement),
                Err(error) => break Err(error),
            }
        };
        self.strict = outer_strict;
        result.map(|()| statements)
    }

    /// Runs `parse` with the `in` operator enabled again. A `for` head parses
    /// its init with `in` disabled only at its own top level (`[~In]`);
    /// parentheses, brackets, argument lists, object literals and function
    /// bodies nested inside it restore `[+In]`.
    fn with_in_allowed<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let saved_no_in = std::mem::replace(&mut self.no_in, false);
        let result = parse(self);
        self.no_in = saved_no_in;
        result
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
        self.reject_legacy_octal_escape()?;
        match self.advance() {
            Token::String(name) => name
                .to_utf8()
                .map_err(|_| self.syntax_error("module specifier must be well-formed Unicode")),
            _ => Err(self.syntax_error("expected a module specifier string")),
        }
    }

    fn expect_module_export_name(&mut self) -> Result<String, ParseError> {
        self.reject_legacy_octal_escape()?;
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

    /// `of` written without an escape: the contextual keyword cannot be spelled
    /// `o\u0066`.
    fn is_contextual_of(&self) -> bool {
        matches!(self.peek(), Token::Identifier(name) if name == "of")
            && !self.current_identifier_escaped()
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

impl Parser {
    /// Parses a template placeholder's source text. It is a fresh parser over
    /// just that text, but it inherits the enclosing function's context so
    /// that `yield`, `await` and strict-only restrictions mean inside the
    /// placeholder what they mean around the template.
    fn parse_template_placeholder(&self, source: &str) -> Result<Expr, ParseError> {
        let mut parser = Parser::new(source);
        parser.generator_depth = self.generator_depth;
        parser.async_depth = self.async_depth;
        parser.module_await = self.module_await;
        parser.strict = self.strict;
        let expr = parser.parse_expression()?;
        if !parser.at_eof() {
            return Err(parser.error("unexpected trailing tokens after expression"));
        }
        Ok(expr)
    }

    fn parse_template(
        &self,
        quasis: Vec<crate::JsString>,
        raw_expressions: Vec<String>,
    ) -> Result<Expr, ParseError> {
        let expressions = raw_expressions
            .iter()
            .map(|src| self.parse_template_placeholder(src))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Expr::Template {
            quasis,
            expressions,
        })
    }
}
