// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a namespace body reads through its object.
//!
//! TypeScript reads an exported variable of a namespace as a property of the
//! namespace object, so a reference to it inside the body has to change, and a
//! function, class, enum or namespace another block of the namespace exported is
//! not a local of this block either. Which names those are, and whether a local
//! binding could shadow one of them (which would make a rewrite by name wrong),
//! is decided here, for both the emitter and the direct bridge.

use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{
    BindingPattern, Declaration, FunctionBodyItem, FunctionDeclaration, FunctionElseBranch, Module,
    NamespaceDeclaration, NestedFunctionBody, Parameter,
};
use crate::syntax::{Token, TokenKind};

fn unsupported(span: SourceSpan, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::UnsupportedSyntax, span, message)
}

pub(crate) fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// Whether a body declares anything that exists at run time.
pub fn has_runtime_values(body: &[Declaration]) -> bool {
    body.iter().any(|declaration| match declaration {
        Declaration::Variable(variable) => !variable.declared,
        Declaration::Function(function) => !function.declared && !function.overload,
        Declaration::Class(_) | Declaration::Raw(_) => true,
        Declaration::Enum(declaration) => !declaration.declared,
        Declaration::Namespace(inner) => !inner.declared && has_runtime_values(&inner.body),
        _ => false,
    })
}

/// What each namespace exports at run time, over every block of it.
#[derive(Default)]
pub struct Exports {
    /// Exported variables: always read through the namespace object.
    variables: BTreeSet<String>,
    /// Exported functions, classes, enums and namespaces with the block that
    /// declared each. They are local names inside their own block and read
    /// through the namespace object everywhere else.
    declarations: Vec<(String, usize)>,
}

fn collect_exports(path: &str, declarations: &[Declaration], into: &mut BTreeMap<String, Exports>) {
    for declaration in declarations {
        let Declaration::Namespace(namespace) = declaration else {
            continue;
        };
        let own = join(path, &namespace.name);
        let exports = into.entry(own.clone()).or_default();
        let block = namespace.span.start;
        for inner in &namespace.body {
            match inner {
                Declaration::Variable(variable) if variable.exported && !variable.declared => {
                    exports.variables.insert(variable.name.clone());
                }
                Declaration::Function(function)
                    if function.exported && !function.declared && !function.overload =>
                {
                    exports.declarations.push((function.name.clone(), block));
                }
                Declaration::Class(class) if class.exported => {
                    exports.declarations.push((class.name.clone(), block));
                }
                Declaration::Enum(item) if item.exported && !item.declared => {
                    exports.declarations.push((item.name.clone(), block));
                }
                Declaration::Namespace(item)
                    if item.exported && !item.declared && has_runtime_values(&item.body) =>
                {
                    exports.declarations.push((item.name.clone(), block));
                }
                _ => {}
            }
        }
        collect_exports(&own, &namespace.body, into);
    }
}

fn pattern_names(pattern: &BindingPattern, into: &mut BTreeSet<String>) {
    match pattern {
        BindingPattern::Object(bindings) => {
            for binding in bindings {
                into.insert(binding.name.clone());
            }
        }
        BindingPattern::Array(elements) => {
            for element in elements.iter().flatten() {
                into.insert(element.name.clone());
            }
        }
    }
}

fn parameter_names(parameters: &[Parameter], into: &mut BTreeSet<String>) {
    for parameter in parameters {
        into.insert(parameter.name.clone());
        if let Some(pattern) = &parameter.pattern {
            pattern_names(pattern, into);
        }
    }
}

fn item_bindings(items: &[FunctionBodyItem], into: &mut BTreeSet<String>) {
    for item in items {
        match item {
            FunctionBodyItem::Variable(variable) => {
                into.insert(variable.name.clone());
            }
            FunctionBodyItem::If(statement) => {
                let mut current = Some(statement);
                while let Some(statement) = current {
                    item_bindings(&statement.consequent, into);
                    current = match &statement.alternate {
                        Some(FunctionElseBranch::Braced(items)) => {
                            item_bindings(items, into);
                            None
                        }
                        Some(FunctionElseBranch::ElseIf(next)) => Some(next),
                        None => None,
                    };
                }
            }
            FunctionBodyItem::While(statement) => item_bindings(&statement.body, into),
            FunctionBodyItem::Try(statement) => {
                item_bindings(&statement.block, into);
                if let Some(handler) = &statement.handler {
                    into.insert(handler.binding.clone());
                    item_bindings(&handler.body, into);
                }
                if let Some(finalizer) = &statement.finalizer {
                    item_bindings(finalizer, into);
                }
            }
            FunctionBodyItem::Function(function) => function_bindings(function, into),
            _ => {}
        }
    }
}

fn function_bindings(function: &FunctionDeclaration, into: &mut BTreeSet<String>) {
    into.insert(function.name.clone());
    parameter_names(&function.parameters, into);
    for local in &function.locals {
        into.insert(local.name.clone());
    }
    item_bindings(&function.body, into);
}

/// The names a body's functions, methods and expressions bind locally.
fn local_bindings(
    module: &Module,
    body: &[Declaration],
    range: (usize, usize),
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for declaration in body {
        match declaration {
            Declaration::Function(function) => function_bindings(function, &mut names),
            Declaration::Class(class) => {
                for member in &class.members {
                    if let Some(constructor) = &member.constructor {
                        parameter_names(&constructor.parameters, &mut names);
                        item_bindings(constructor.body.as_deref().unwrap_or(&[]), &mut names);
                    }
                    if let Some(method) = &member.method {
                        parameter_names(&method.parameters, &mut names);
                        item_bindings(method.body.as_deref().unwrap_or(&[]), &mut names);
                    }
                    if let Some(accessor) = &member.accessor {
                        parameter_names(&accessor.parameters, &mut names);
                        item_bindings(&accessor.body, &mut names);
                    }
                }
            }
            _ => {}
        }
    }
    for nested in module.nested_functions.values() {
        if nested.span.start >= range.0 && nested.span.end <= range.1 {
            if let Some(name) = &nested.name {
                names.insert(name.clone());
            }
            parameter_names(&nested.parameters, &mut names);
            if let NestedFunctionBody::Block { items, locals, .. } = &nested.body {
                for local in locals {
                    names.insert(local.name.clone());
                }
                item_bindings(items, &mut names);
            }
        }
    }
    names
}

/// The exports of every namespace of a module, by namespace path.
pub struct NamespaceExports(BTreeMap<String, Exports>);

/// Where in the nesting a body sits.
pub struct BodyInput<'a> {
    /// The namespace's dotted path from the module, `A.B.C` for a body in `C`.
    pub own_path: &'a str,
    /// The name of the function parameter that is the namespace object.
    pub param: &'a str,
    /// The declaration written in the source (the outermost of a dotted name).
    pub namespace: &'a NamespaceDeclaration,
    /// The declaration whose body this is (the innermost of a dotted name).
    pub innermost: &'a NamespaceDeclaration,
    /// What enclosing namespaces make references to.
    pub outer: &'a BTreeMap<String, String>,
}

/// The byte ranges of the inner namespaces and enums of a body, which are
/// rebuilt or analysed on their own.
pub fn nested_spans(body: &[Declaration]) -> Vec<(usize, usize)> {
    body.iter()
        .filter_map(|declaration| match declaration {
            Declaration::Namespace(inner) => Some((inner.span.start, inner.span.end)),
            Declaration::Enum(inner) => Some((inner.span.start, inner.span.end)),
            _ => None,
        })
        .collect()
}

impl NamespaceExports {
    pub fn of(module: &Module) -> Self {
        let mut exports = BTreeMap::new();
        collect_exports("", &module.declarations, &mut exports);
        Self(exports)
    }

    /// The names a body reads through a namespace object, mapped to the
    /// parameter that holds it; an error when a local could shadow one.
    pub fn references(
        &self,
        module: &Module,
        tokens: &[Token],
        input: &BodyInput<'_>,
    ) -> Result<BTreeMap<String, String>, Diagnostic> {
        let BodyInput {
            own_path,
            param,
            namespace,
            innermost,
            outer,
        } = *input;
        let body = &innermost.body;
        let range = (innermost.header_span.end, innermost.closing_span.start);
        let innermost_block = innermost.span.start;
        // References to an exported variable go through the namespace object.
        let mut references = outer.clone();
        for declaration in body {
            let name = match declaration {
                Declaration::Variable(item) => Some(&item.name),
                Declaration::Function(item) => Some(&item.name),
                Declaration::Class(item) => Some(&item.name),
                Declaration::Enum(item) => Some(&item.name),
                Declaration::Namespace(item) => Some(&item.name),
                _ => None,
            };
            if let Some(name) = name {
                references.remove(name);
            }
        }
        if let Some(exports) = self.0.get(own_path) {
            for variable in &exports.variables {
                references.insert(variable.clone(), param.to_string());
            }
            // A member another block declared is not a local of this one.
            for (name, block) in &exports.declarations {
                let local = body.iter().any(|declaration| match declaration {
                    Declaration::Function(item) => item.name == *name,
                    Declaration::Class(item) => item.name == *name,
                    Declaration::Enum(item) => item.name == *name,
                    Declaration::Namespace(item) => item.name == *name,
                    Declaration::Variable(item) => item.name == *name,
                    _ => false,
                });
                if *block != innermost_block && !local {
                    references.insert(name.clone(), param.to_string());
                }
            }
        }
        let nested = nested_spans(body);
        refuse_shadowing(module, tokens, body, range, &nested, &references, namespace)?;
        Ok(references)
    }
}

/// Refuses a body in which a local could shadow an exported variable that
/// the body's text refers to.
fn refuse_shadowing(
    module: &Module,
    all_tokens: &[Token],
    body: &[Declaration],
    range: (usize, usize),
    nested: &[(usize, usize)],
    references: &BTreeMap<String, String>,
    namespace: &NamespaceDeclaration,
) -> Result<(), Diagnostic> {
    if references.is_empty() {
        return Ok(());
    }
    let structural = local_bindings(module, body, range);
    // Declarations in text the structure does not cover: `for (const x ..)`,
    // blocks, catch clauses and patterns, counted per name.
    let start = all_tokens.partition_point(|token| token.start < range.0);
    let end = all_tokens.partition_point(|token| token.start < range.1);
    let tokens = &all_tokens[start..end];
    let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, token) in tokens.iter().enumerate() {
        if nested
            .iter()
            .any(|(from, to)| *from <= token.start && token.start < *to)
        {
            continue;
        }
        if token.kind == TokenKind::Keyword || token.kind == TokenKind::Identifier {
            if matches!(
                token.text.as_str(),
                "const" | "let" | "var" | "function" | "class"
            ) {
                match tokens.get(index + 1) {
                    Some(next) if next.is("{") || next.is("[") => {
                        let mut depth = 0usize;
                        for inner in &tokens[index + 1..] {
                            match inner.text.as_str() {
                                "{" | "[" => depth += 1,
                                "}" | "]" => {
                                    depth = depth.saturating_sub(1);
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                _ if inner.kind == TokenKind::Identifier => {
                                    *counted.entry(inner.text.as_str()).or_default() += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(next) if next.kind == TokenKind::Identifier => {
                        *counted.entry(next.text.as_str()).or_default() += 1;
                    }
                    _ => {}
                }
            }
            if token.is("catch") {
                if let (Some(open), Some(name)) = (tokens.get(index + 1), tokens.get(index + 2)) {
                    if open.is("(") {
                        *counted.entry(name.text.as_str()).or_default() += 1;
                    }
                }
            }
        }
    }
    // An exported variable of this body is declared once, by its own
    // declaration; any further binding of the name is a shadow.
    let own: BTreeSet<&str> = body
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Variable(item) if item.exported => Some(item.name.as_str()),
            _ => None,
        })
        .collect();
    for name in references.keys() {
        let allowed = usize::from(own.contains(name.as_str()));
        let found = counted.get(name.as_str()).copied().unwrap_or(0);
        if structural.contains(name) || found > allowed {
            return Err(unsupported(
                namespace.name_span.clone(),
                format!(
                    "a local named `{name}` shadows an exported variable of the namespace \
                     it is used in, which is not supported yet"
                ),
            ));
        }
    }
    Ok(())
}

/// Whether the `{` before `index` (found by walking back to the nearest
/// unmatched opener) begins an object literal rather than a block.
pub(crate) fn in_object_literal(tokens: &[Token], index: usize) -> bool {
    let mut depth = 0usize;
    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        match tokens[cursor].text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            "{" => {
                if depth > 0 {
                    depth -= 1;
                    continue;
                }
                let before = cursor
                    .checked_sub(1)
                    .map(|before| tokens[before].text.as_str());
                // A `{` that begins the tokens of an expression is an object.
                return before.is_none()
                    || matches!(
                        before,
                        Some(
                            "=" | "("
                                | ","
                                | ":"
                                | "["
                                | "return"
                                | "?"
                                | "||"
                                | "&&"
                                | "??"
                                | "..."
                                | "+"
                                | "-"
                                | "!"
                                | "typeof"
                                | "void"
                                | "await"
                                | "yield"
                                | "in"
                                | "of"
                                | "+="
                                | "-="
                                | "||="
                                | "&&="
                                | "??="
                        )
                    );
            }
            _ => {}
        }
    }
    false
}

/// `tokens` with each reference to a name in `references` written as a property
/// of the namespace parameter that holds it (`x` as `N . x`, a shorthand
/// property `{ x }` as `{ x : N . x }`), and each qualified name BlueTS merged
/// into one token (`N.f`) written out as the names and dots it was. The new
/// tokens keep the spans of the ones they replace.
pub fn rewrite_tokens(
    tokens: &[Token],
    references: &BTreeMap<String, String>,
) -> Result<Vec<Token>, Diagnostic> {
    let mut output = Vec::with_capacity(tokens.len());
    for (index, token) in tokens.iter().enumerate() {
        // A qualified name BlueTS merged into one token is written out again.
        if token.kind == TokenKind::Identifier && token.text.contains('.') {
            // A chain that starts with a name read through a namespace object
            // starts from that object.
            let head = token.text.split('.').next().unwrap_or("");
            if let Some(parent) = references.get(head) {
                output.push(Token {
                    kind: TokenKind::Identifier,
                    text: parent.clone(),
                    start: token.start,
                    end: token.end,
                });
                output.push(Token {
                    kind: TokenKind::Punct,
                    text: ".".to_string(),
                    start: token.start,
                    end: token.end,
                });
            }
            for (position, name) in token.text.split('.').enumerate() {
                if position > 0 {
                    output.push(Token {
                        kind: TokenKind::Punct,
                        text: ".".to_string(),
                        start: token.start,
                        end: token.end,
                    });
                }
                output.push(Token {
                    kind: TokenKind::Identifier,
                    text: name.to_string(),
                    start: token.start,
                    end: token.end,
                });
            }
            continue;
        }
        if token.kind == TokenKind::Template && token.text.contains("${") {
            let reads = references.keys().any(|name| {
                token
                    .text
                    .split(|character: char| {
                        !(character.is_alphanumeric() || matches!(character, '_' | '$'))
                    })
                    .any(|word| word == name)
            });
            if reads {
                return Err(unsupported(
                    SourceSpan::new("", token.start, token.end),
                    "a template literal that reads an exported namespace variable is not lowered directly yet",
                ));
            }
            output.push(token.clone());
            continue;
        }
        let Some(parent) = references
            .get(&token.text)
            .filter(|_| token.kind == TokenKind::Identifier)
        else {
            output.push(token.clone());
            continue;
        };
        let before = index
            .checked_sub(1)
            .map(|before| tokens[before].text.as_str());
        let after = tokens.get(index + 1).map(|after| after.text.as_str());
        if matches!(before, Some("." | "?.")) {
            output.push(token.clone());
            continue;
        }
        let mut shorthand = false;
        if matches!(before, Some("{" | ",")) && in_object_literal(tokens, index) {
            match after {
                Some(":" | "(") => {
                    output.push(token.clone());
                    continue;
                }
                Some("," | "}") => shorthand = true,
                _ => {}
            }
        } else if before == Some("{") && after == Some(":") {
            // A label.
            output.push(token.clone());
            continue;
        }
        let piece = |kind: TokenKind, text: &str| Token {
            kind,
            text: text.to_string(),
            start: token.start,
            end: token.end,
        };
        if shorthand {
            output.push(piece(TokenKind::Identifier, &token.text));
            output.push(piece(TokenKind::Punct, ":"));
        }
        output.push(piece(TokenKind::Identifier, parent));
        output.push(piece(TokenKind::Punct, "."));
        output.push(piece(TokenKind::Identifier, &token.text));
    }
    Ok(output)
}

type Rewrite<'a> = &'a dyn Fn(&[Token]) -> Result<Vec<Token>, Diagnostic>;

fn rewrite_optional(
    tokens: &Option<Vec<Token>>,
    rewrite: Rewrite<'_>,
) -> Result<Option<Vec<Token>>, Diagnostic> {
    tokens.as_deref().map(rewrite).transpose()
}

fn rewrite_parameters(
    parameters: &[Parameter],
    rewrite: Rewrite<'_>,
) -> Result<Vec<Parameter>, Diagnostic> {
    parameters
        .iter()
        .map(|parameter| {
            Ok(Parameter {
                default: rewrite_optional(&parameter.default, rewrite)?,
                ..parameter.clone()
            })
        })
        .collect()
}

fn rewrite_variable(
    variable: &crate::parser::VariableDeclaration,
    rewrite: Rewrite<'_>,
) -> Result<crate::parser::VariableDeclaration, Diagnostic> {
    Ok(crate::parser::VariableDeclaration {
        initializer: rewrite(&variable.initializer)?,
        ..variable.clone()
    })
}

fn rewrite_items(
    items: &[FunctionBodyItem],
    rewrite: Rewrite<'_>,
) -> Result<Vec<FunctionBodyItem>, Diagnostic> {
    items
        .iter()
        .map(|item| rewrite_item(item, rewrite))
        .collect()
}

fn rewrite_if(
    statement: &crate::parser::FunctionIfStatement,
    rewrite: Rewrite<'_>,
) -> Result<crate::parser::FunctionIfStatement, Diagnostic> {
    Ok(crate::parser::FunctionIfStatement {
        test: rewrite(&statement.test)?,
        consequent: rewrite_items(&statement.consequent, rewrite)?,
        alternate: match &statement.alternate {
            None => None,
            Some(FunctionElseBranch::Braced(items)) => {
                Some(FunctionElseBranch::Braced(rewrite_items(items, rewrite)?))
            }
            Some(FunctionElseBranch::ElseIf(next)) => Some(FunctionElseBranch::ElseIf(Box::new(
                rewrite_if(next, rewrite)?,
            ))),
        },
        span: statement.span.clone(),
    })
}

fn rewrite_item(
    item: &FunctionBodyItem,
    rewrite: Rewrite<'_>,
) -> Result<FunctionBodyItem, Diagnostic> {
    Ok(match item {
        FunctionBodyItem::Variable(variable) => {
            FunctionBodyItem::Variable(rewrite_variable(variable, rewrite)?)
        }
        FunctionBodyItem::Expression { tokens, span } => FunctionBodyItem::Expression {
            tokens: rewrite(tokens)?,
            span: span.clone(),
        },
        FunctionBodyItem::Throw { tokens, span } => FunctionBodyItem::Throw {
            tokens: rewrite(tokens)?,
            span: span.clone(),
        },
        FunctionBodyItem::Return { tokens, span } => FunctionBodyItem::Return {
            tokens: rewrite(tokens)?,
            span: span.clone(),
        },
        FunctionBodyItem::If(statement) => FunctionBodyItem::If(rewrite_if(statement, rewrite)?),
        FunctionBodyItem::While(statement) => {
            FunctionBodyItem::While(crate::parser::FunctionWhileStatement {
                test: rewrite(&statement.test)?,
                body: rewrite_items(&statement.body, rewrite)?,
                span: statement.span.clone(),
            })
        }
        FunctionBodyItem::Try(statement) => {
            FunctionBodyItem::Try(crate::parser::FunctionTryStatement {
                block: rewrite_items(&statement.block, rewrite)?,
                handler: match &statement.handler {
                    None => None,
                    Some(handler) => Some(crate::parser::FunctionCatchClause {
                        body: rewrite_items(&handler.body, rewrite)?,
                        ..handler.clone()
                    }),
                },
                finalizer: statement
                    .finalizer
                    .as_deref()
                    .map(|items| rewrite_items(items, rewrite))
                    .transpose()?,
                span: statement.span.clone(),
            })
        }
        FunctionBodyItem::Function(function) => {
            FunctionBodyItem::Function(Box::new(rewrite_function(function, rewrite)?))
        }
        FunctionBodyItem::Opaque(span) => FunctionBodyItem::Opaque(span.clone()),
    })
}

fn rewrite_function(
    function: &FunctionDeclaration,
    rewrite: Rewrite<'_>,
) -> Result<FunctionDeclaration, Diagnostic> {
    Ok(FunctionDeclaration {
        parameters: rewrite_parameters(&function.parameters, rewrite)?,
        body: rewrite_items(&function.body, rewrite)?,
        ..function.clone()
    })
}

/// A declaration with every runtime expression in it rewritten, for the direct
/// bridge, which lowers from the declaration's own tokens. An inner namespace is
/// left to its own analysis. With no references this only splits merged names.
pub fn rewrite_declaration(
    declaration: &Declaration,
    references: &BTreeMap<String, String>,
) -> Result<Declaration, Diagnostic> {
    let rewrite = |tokens: &[Token]| rewrite_tokens(tokens, references);
    let rewrite: Rewrite<'_> = &rewrite;
    Ok(match declaration {
        Declaration::Variable(variable) => {
            Declaration::Variable(rewrite_variable(variable, rewrite)?)
        }
        Declaration::Function(function) => {
            Declaration::Function(rewrite_function(function, rewrite)?)
        }
        Declaration::Raw(raw) => Declaration::Raw(crate::parser::RawDeclaration {
            tokens: rewrite(&raw.tokens)?,
            span: raw.span.clone(),
        }),
        Declaration::Enum(declaration) => {
            let mut declaration = declaration.clone();
            for member in &mut declaration.members {
                member.initializer = rewrite_optional(&member.initializer, rewrite)?;
            }
            Declaration::Enum(declaration)
        }
        Declaration::Class(class) => {
            let mut class = class.clone();
            for member in &mut class.members {
                if let Some(constructor) = &mut member.constructor {
                    constructor.parameters = rewrite_parameters(&constructor.parameters, rewrite)?;
                    constructor.body = constructor
                        .body
                        .as_deref()
                        .map(|items| rewrite_items(items, rewrite))
                        .transpose()?;
                }
                if let Some(method) = &mut member.method {
                    method.parameters = rewrite_parameters(&method.parameters, rewrite)?;
                    method.body = method
                        .body
                        .as_deref()
                        .map(|items| rewrite_items(items, rewrite))
                        .transpose()?;
                }
                if let Some(field) = &mut member.field {
                    field.initializer = rewrite_optional(&field.initializer, rewrite)?;
                }
                if let Some(accessor) = &mut member.accessor {
                    accessor.parameters = rewrite_parameters(&accessor.parameters, rewrite)?;
                    accessor.body = rewrite_items(&accessor.body, rewrite)?;
                }
                if let Some(block) = &mut member.static_block {
                    block.body = rewrite_items(&block.body, rewrite)?;
                }
            }
            Declaration::Class(class)
        }
        other => other.clone(),
    })
}
