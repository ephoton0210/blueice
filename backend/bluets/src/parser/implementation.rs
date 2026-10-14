// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private parser state and its focused parsing submodules.

use super::*;

pub(super) struct Parser {
    id: String,
    source: String,
    tokens: Vec<Token>,
    index: usize,
    declarations: Vec<Declaration>,
    edits: Vec<TextEdit>,
    generic_call_type_arguments: BTreeMap<usize, Vec<Type>>,
    nested_functions: BTreeMap<usize, NestedFunction>,
    class_expressions: BTreeMap<usize, ClassExpression>,
    type_references: Vec<TypeReference>,
    expression_variable_types: BTreeMap<usize, Type>,
    type_assertions: BTreeMap<usize, Type>,
    diagnostics: Vec<Diagnostic>,
    max_type_depth: usize,
    type_depth: usize,
    infer_depth: usize,
    /// Set while a class constructor's parameters are parsed, where an
    /// accessibility modifier or `readonly` declares a property.
    parameter_property_mode: bool,
    parameter_properties: Vec<ParameterProperty>,
    /// How many namespace bodies the cursor is inside, and how many of them are
    /// ambient (`declare namespace`).
    namespace_depth: usize,
    ambient_depth: usize,
    /// How many `export {};` markers the body being parsed has had.
    namespace_export_markers: usize,
    /// Decorators read before a declaration, for the class that follows them.
    pending_decorators: Vec<Decorator>,
    /// Internal emitter input has already passed TypeScript subset checks.
    emitted_runtime: bool,
    /// Exclusive token bound when the emitter parses one owned statement.
    emitted_body_end: Option<usize>,
}

#[path = "declarations.rs"]
mod declarations;
pub(crate) use declarations::parse_variable_pattern;
#[path = "import_attributes.rs"]
mod import_attributes;
#[path = "runtime_syntax.rs"]
mod runtime_syntax;
#[path = "type_syntax.rs"]
mod type_syntax;

/// Parse an owned JSX tag's erased type arguments with the ordinary type grammar.
pub(crate) fn parse_jsx_type_arguments(
    module: &Module,
    range: std::ops::Range<usize>,
    depth: usize,
) -> Result<(Vec<Type>, Vec<TypeReference>), Vec<Diagnostic>> {
    let mut tokens = crate::syntax::lex(&module.id, &module.source[range.clone()])?;
    for token in &mut tokens {
        token.start += range.start;
        token.end += range.start;
    }
    let mut parser = Parser::new(module.id.clone(), module.source.clone(), tokens, depth);
    let mut arguments = Vec::new();
    while !parser.at_eof() {
        let before = parser.index;
        arguments.push(parser.parse_type_until(&[","]));
        if !parser.consume(",") {
            break;
        }
        if parser.index <= before {
            break;
        }
    }
    if arguments.is_empty() {
        parser.error_here(DiagnosticCode::ParseError, "expected a type");
    } else if !parser.at_eof() {
        parser.error_here(
            DiagnosticCode::ParseError,
            "expected `,` between JSX type arguments",
        );
    }
    if parser.diagnostics.is_empty() {
        Ok((arguments, parser.type_references))
    } else {
        Err(parser.diagnostics)
    }
}

pub(crate) fn parse_emitted_module(
    id: &str,
    source: &str,
    limits: &ParserLimits,
) -> Result<Module, Vec<Diagnostic>> {
    let tokens = lex_with_limits(id, source, limits.max_source_bytes, limits.max_tokens)?;
    let mut parser = Parser::new(id.into(), source.into(), tokens, limits.max_type_depth);
    parser.emitted_runtime = true;
    parser.parse_module()
}

/// Reuses the ordinary function-body parser for an already lexed, braced
/// emitted block. Source offsets stay in the owning module's coordinate space.
pub(crate) fn parse_emitted_block(
    module: &Module,
    tokens: Vec<Token>,
    open: usize,
    depth: usize,
) -> Option<Vec<FunctionBodyItem>> {
    if !tokens.get(open).is_some_and(|token| token.is("{")) {
        return None;
    }
    let start = tokens[open].start;
    let mut parser = Parser::new(module.id.clone(), module.source.clone(), tokens, depth);
    parser.emitted_runtime = true;
    parser.index = open + 1;
    let mut body = Vec::new();
    parser.parse_function_body(start, &mut body, &mut Vec::new(), &mut Vec::new());
    parser.diagnostics.is_empty().then_some(body)
}

pub(crate) fn parse_emitted_statement(
    module: &Module,
    tokens: Vec<Token>,
    start: usize,
    end: usize,
    depth: usize,
) -> Option<Vec<FunctionBodyItem>> {
    if start >= end || end >= tokens.len() {
        return None;
    }
    let offset = tokens[start].start;
    let mut parser = Parser::new(module.id.clone(), module.source.clone(), tokens, depth);
    parser.emitted_runtime = true;
    parser.emitted_body_end = Some(end);
    parser.index = start;
    let mut body = Vec::new();
    parser.parse_function_body(offset, &mut body, &mut Vec::new(), &mut Vec::new());
    parser.diagnostics.is_empty().then_some(body)
}
