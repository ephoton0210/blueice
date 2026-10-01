// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace emit.
//!
//! A namespace becomes the function-and-object form TypeScript builds, so that
//! blocks of one name merge and each runs in its own scope:
//!
//! ```js
//! var N;
//! (function (N) {
//!     N.a = 1;
//!     function f() { return N.a; }
//!     N.f = f;
//! })(N || (N = {}));
//! ```
//!
//! The text between the braces stays where it was. Only the header and the
//! closing brace are replaced, an exported variable's `export const x` becomes
//! `N.x`, an exported function or class loses `export` and gains `N.f = f;`
//! after it, an enum or inner namespace is rebuilt in place, and every
//! reference to an exported variable inside the body is rewritten to `N.x`, as
//! it reads the property, not a local. Everything keeps its line, so source
//! lines and maps do not move. A body with nothing at run time, and an ambient
//! one, is removed.
//!
//! A reference is rewritten from the tokens. That is only safe when no local
//! shadows the variable, so a parameter, local, catch binding or pattern with
//! the name of an exported variable it could see is refused instead.

use std::collections::{BTreeMap, BTreeSet};

use super::enums::{enum_statement, spread_lines, template_substitutions, EnumPlacement};
use super::{Module, TextEdit};
use crate::compiler::CompilerOptions;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::enum_eval::evaluate_enums_in;
use crate::parser::{
    BindingPattern, Declaration, FunctionBodyItem, FunctionDeclaration, FunctionElseBranch,
    NamespaceDeclaration, NestedFunctionBody, Parameter,
};
use crate::{Token, TokenKind};

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// Whether a body declares anything that exists at run time.
fn has_runtime_values(body: &[Declaration]) -> bool {
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
struct Exports {
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

/// Where a namespace's variable is declared, and what its function receives.
enum Scope {
    Module,
    Namespace { parent: String },
}

struct Walk<'a> {
    module: &'a Module,
    tokens: &'a [Token],
    exports: &'a BTreeMap<String, Exports>,
    edits: &'a mut Vec<TextEdit>,
    inline_const_enums: bool,
    preserve_const_enums: bool,
}

pub(super) fn lower_namespaces(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    if !module
        .declarations
        .iter()
        .any(|declaration| matches!(declaration, Declaration::Namespace(_)))
    {
        return Ok(());
    }
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return Ok(());
    };
    let mut exports = BTreeMap::new();
    collect_exports("", &module.declarations, &mut exports);
    let exported_by_name: BTreeSet<&str> = module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::ValueExport(export) => Some(export.bindings.iter()),
            _ => None,
        })
        .flatten()
        .map(|binding| binding.local.as_str())
        .collect();
    let mut walk = Walk {
        module,
        tokens: &tokens,
        exports: &exports,
        edits,
        inline_const_enums: options.inlines_const_enums(),
        preserve_const_enums: options.preserve_const_enums,
    };
    // The names a module-level namespace may merge into.
    let mut declared: BTreeSet<String> = BTreeSet::new();
    for declaration in &module.declarations {
        match declaration {
            Declaration::Function(function) if !function.declared && !function.overload => {
                declared.insert(function.name.clone());
            }
            Declaration::Class(class) => {
                declared.insert(class.name.clone());
            }
            Declaration::Enum(enum_declaration) => {
                let erased = enum_declaration.declared
                    || (enum_declaration.is_const
                        && walk.inline_const_enums
                        && !walk.preserve_const_enums
                        && !enum_declaration.exported
                        && !exported_by_name.contains(enum_declaration.name.as_str()));
                if !erased {
                    declared.insert(enum_declaration.name.clone());
                }
            }
            Declaration::Namespace(namespace) => {
                walk.namespace(
                    namespace,
                    &Scope::Module,
                    &mut declared,
                    "",
                    &BTreeMap::new(),
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn unsupported(span: SourceSpan, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::UnsupportedSyntax, span, message)
}

impl Walk<'_> {
    fn edit(&mut self, start: usize, end: usize, replacement: impl Into<String>) {
        self.edits.push(TextEdit {
            start,
            end,
            replacement: replacement.into(),
        });
    }

    fn lines_in(&self, start: usize, end: usize) -> usize {
        self.module.source[start..end].matches('\n').count()
    }

    /// The index of the first token at or after `offset`.
    fn token_at(&self, offset: usize) -> usize {
        self.tokens.partition_point(|token| token.start < offset)
    }

    /// Replaces `start..end` with `text`, followed by the line breaks it held.
    fn replace_keeping_lines(&mut self, start: usize, end: usize, text: String) {
        let lines = self.lines_in(start, end);
        self.edit(start, end, format!("{text}{}", "\n".repeat(lines)));
    }

    fn namespace(
        &mut self,
        namespace: &NamespaceDeclaration,
        scope: &Scope,
        declared: &mut BTreeSet<String>,
        parent_path: &str,
        outer: &BTreeMap<String, String>,
    ) -> Result<(), Diagnostic> {
        if namespace.declared || !has_runtime_values(&namespace.body) {
            self.edit(namespace.span.start, namespace.span.end, "");
            return Ok(());
        }
        let mut chain = vec![namespace];
        while let [Declaration::Namespace(inner)] = chain[chain.len() - 1].body.as_slice() {
            if !inner.implicit {
                break;
            }
            chain.push(inner);
        }
        let innermost = chain[chain.len() - 1];
        let first = declared.insert(namespace.name.clone());

        // The header opens one function per segment of the dotted name.
        let mut header = String::new();
        let mut closing: Vec<String> = Vec::new();
        for (index, segment) in chain.iter().enumerate() {
            let name = &segment.name;
            let argument = if index == 0 {
                match scope {
                    Scope::Namespace { parent } if namespace.exported => {
                        format!("{name} = {parent}.{name} || ({parent}.{name} = {{}})")
                    }
                    _ => format!("{name} || ({name} = {{}})"),
                }
            } else {
                let parent = &chain[index - 1].name;
                format!("{name} = {parent}.{name} || ({parent}.{name} = {{}})")
            };
            if index == 0 {
                if first {
                    let keyword = match scope {
                        Scope::Module => {
                            if namespace.exported {
                                "export var"
                            } else {
                                "var"
                            }
                        }
                        Scope::Namespace { .. } => "let",
                    };
                    header.push_str(&format!("{keyword} {name}; "));
                }
            } else {
                header.push_str(&format!("var {name}; "));
            }
            header.push_str(&format!("(function ({name}) {{ "));
            closing.push(format!("}})({argument});"));
        }
        closing.reverse();
        let header = header.trim_end().to_string();
        self.replace_keeping_lines(
            namespace.header_span.start,
            namespace.header_span.end,
            header,
        );
        self.replace_keeping_lines(
            namespace.closing_span.start,
            namespace.closing_span.end,
            closing.join(" "),
        );

        let param = innermost.name.clone();
        let own_path = chain.iter().fold(parent_path.to_string(), |path, segment| {
            join(&path, &segment.name)
        });
        let range = (innermost.header_span.end, innermost.closing_span.start);
        let innermost_block = innermost.span.start;
        let body = &innermost.body;

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
        if let Some(exports) = self.exports.get(&own_path) {
            for variable in &exports.variables {
                references.insert(variable.clone(), param.clone());
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
                    references.insert(name.clone(), param.clone());
                }
            }
        }
        let nested_spans: Vec<(usize, usize)> = body
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Namespace(inner) => Some((inner.span.start, inner.span.end)),
                Declaration::Enum(inner) => Some((inner.span.start, inner.span.end)),
                _ => None,
            })
            .collect();
        self.refuse_shadowing(body, range, &nested_spans, &references, namespace)?;

        let evaluations = evaluate_enums_in(body);
        let mut evaluations = evaluations.iter();
        let mut declared_here: BTreeSet<String> = BTreeSet::new();
        let mut nested: Vec<(usize, usize)> = Vec::new();
        for declaration in body {
            match declaration {
                Declaration::Variable(variable) if variable.exported && !variable.declared => {
                    self.exported_variable(variable, &param)?;
                }
                Declaration::Function(function) if !function.declared && !function.overload => {
                    declared_here.insert(function.name.clone());
                    if function.exported {
                        self.exported_member(&function.span, &function.name, &param);
                    }
                }
                Declaration::Class(class) => {
                    declared_here.insert(class.name.clone());
                    if class.exported {
                        self.exported_member(&class.span, &class.name, &param);
                    }
                }
                Declaration::Enum(enum_declaration) => {
                    let evaluation = evaluations.next().expect("every enum was evaluated");
                    let text = if enum_declaration.declared {
                        String::new()
                    } else {
                        let first = declared_here.insert(enum_declaration.name.clone());
                        let placement = EnumPlacement {
                            keyword: "let",
                            export_modifier: false,
                            namespace: Some((&param, enum_declaration.exported)),
                        };
                        enum_statement(
                            self.module,
                            enum_declaration,
                            evaluation,
                            first,
                            &placement,
                            self.edits,
                        )
                    };
                    let lines =
                        self.lines_in(enum_declaration.span.start, enum_declaration.span.end);
                    self.edits.retain(|edit| {
                        !(edit.start >= enum_declaration.span.start
                            && edit.end <= enum_declaration.span.end)
                    });
                    self.edit(
                        enum_declaration.span.start,
                        enum_declaration.span.end,
                        spread_lines(text, lines),
                    );
                    nested.push((enum_declaration.span.start, enum_declaration.span.end));
                }
                Declaration::Namespace(inner) => {
                    nested.push((inner.span.start, inner.span.end));
                    self.namespace(
                        inner,
                        &Scope::Namespace {
                            parent: param.clone(),
                        },
                        &mut declared_here,
                        &own_path,
                        &references,
                    )?;
                }
                _ => {}
            }
        }
        if !references.is_empty() {
            self.rewrite_references(range, &nested, &references, body);
        }
        Ok(())
    }

    /// `export const x = v;` becomes `N.x = v;`; with no value it is removed.
    fn exported_variable(
        &mut self,
        variable: &crate::parser::VariableDeclaration,
        param: &str,
    ) -> Result<(), Diagnostic> {
        if variable.initializer.is_empty() {
            self.edit(variable.span.start, variable.span.end, "");
            return Ok(());
        }
        let mut depth = 0usize;
        for token in &variable.initializer {
            match token.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                "," if depth == 0 => {
                    return Err(unsupported(
                        variable.span.clone(),
                        "an exported namespace variable with several declarators is not supported yet",
                    ));
                }
                _ => {}
            }
        }
        let from = self.token_at(variable.span.start);
        let name_token = self.tokens[from..]
            .iter()
            .take_while(|token| token.start < variable.span.end)
            .find(|token| token.kind == TokenKind::Identifier && token.text == variable.name)
            .cloned();
        let Some(name_token) = name_token else {
            return Err(unsupported(
                variable.span.clone(),
                "an exported namespace variable's name could not be located",
            ));
        };
        self.edit(
            variable.span.start,
            name_token.end,
            format!("{param}.{}", variable.name),
        );
        Ok(())
    }

    /// An exported function or class keeps its declaration, loses `export`, and
    /// is stored on the namespace right after it.
    fn exported_member(&mut self, span: &SourceSpan, name: &str, param: &str) {
        let from = self.token_at(span.start);
        if let (Some(export), Some(next)) = (self.tokens.get(from), self.tokens.get(from + 1)) {
            if export.is("export") {
                let (start, end) = (export.start, next.start);
                self.edit(start, end, "");
            }
        }
        self.edit(span.end, span.end, format!(" {param}.{name} = {name};"));
    }

    /// Refuses a body in which a local could shadow an exported variable that
    /// the body's text refers to.
    fn refuse_shadowing(
        &self,
        body: &[Declaration],
        range: (usize, usize),
        nested: &[(usize, usize)],
        references: &BTreeMap<String, String>,
        namespace: &NamespaceDeclaration,
    ) -> Result<(), Diagnostic> {
        if references.is_empty() {
            return Ok(());
        }
        let structural = local_bindings(self.module, body, range);
        // Declarations in text the structure does not cover: `for (const x ..)`,
        // blocks, catch clauses and patterns, counted per name.
        let start = self.token_at(range.0);
        let end = self.token_at(range.1);
        let tokens = &self.tokens[start..end];
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
                    if let (Some(open), Some(name)) = (tokens.get(index + 1), tokens.get(index + 2))
                    {
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

    /// Rewrites each reference to an exported variable in `range`, outside the
    /// text that is erased or replaced, to a property of its namespace.
    fn rewrite_references(
        &mut self,
        range: (usize, usize),
        nested: &[(usize, usize)],
        references: &BTreeMap<String, String>,
        body: &[Declaration],
    ) {
        let mut covered: Vec<(usize, usize)> = self
            .edits
            .iter()
            .filter(|edit| edit.end > edit.start)
            .map(|edit| (edit.start, edit.end))
            .collect();
        covered.extend_from_slice(nested);
        let members = class_member_names(self.module, body);
        let start = self.token_at(range.0);
        let end = self.token_at(range.1);
        let mut pending: Vec<TextEdit> = Vec::new();
        scan(
            self.module,
            &self.tokens[start..end],
            0,
            references,
            &covered,
            &members,
            &mut pending,
        );
        self.edits.extend(pending);
    }
}

/// The offsets of the identifier tokens that name a class member.
fn class_member_names(module: &Module, body: &[Declaration]) -> BTreeSet<usize> {
    let mut offsets = BTreeSet::new();
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return offsets;
    };
    for declaration in body {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        for member in &class.members {
            let Some(name) = &member.name else {
                continue;
            };
            let from = tokens.partition_point(|token| token.start < member.span.start);
            if let Some(token) = tokens[from..]
                .iter()
                .take_while(|token| token.start < member.span.end)
                .find(|token| token.text == *name)
            {
                offsets.insert(token.start);
            }
        }
    }
    offsets
}

/// Whether the `{` before `index` (found by walking back to the nearest
/// unmatched opener) begins an object literal rather than a block.
fn in_object_literal(tokens: &[Token], index: usize) -> bool {
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
                return matches!(
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

fn scan(
    module: &Module,
    tokens: &[Token],
    base: usize,
    references: &BTreeMap<String, String>,
    covered: &[(usize, usize)],
    members: &BTreeSet<usize>,
    edits: &mut Vec<TextEdit>,
) {
    for (index, token) in tokens.iter().enumerate() {
        let start = base + token.start;
        if covered
            .iter()
            .any(|(from, to)| *from <= start && start < *to)
        {
            continue;
        }
        if token.kind == TokenKind::Template && token.text.contains("${") {
            for (from, to) in template_substitutions(&token.text) {
                if let Ok(inner) = crate::lex(&module.id, &token.text[from..to]) {
                    scan(
                        module,
                        &inner,
                        start + from,
                        references,
                        covered,
                        &BTreeSet::new(),
                        edits,
                    );
                }
            }
            continue;
        }
        let Some(parent) = references
            .get(&token.text)
            .filter(|_| token.kind == TokenKind::Identifier)
        else {
            continue;
        };
        let before = index
            .checked_sub(1)
            .map(|before| tokens[before].text.as_str());
        let after = tokens.get(index + 1).map(|after| after.text.as_str());
        if matches!(before, Some("." | "?.")) || members.contains(&start) {
            continue;
        }
        let mut replacement = format!("{parent}.{}", token.text);
        if matches!(before, Some("{" | ",")) && in_object_literal(tokens, index) {
            match after {
                Some(":" | "(") => continue,
                Some("," | "}") => replacement = format!("{}: {replacement}", token.text),
                _ => {}
            }
        } else if before == Some("{") && after == Some(":") {
            // A label.
            continue;
        }
        edits.push(TextEdit {
            start,
            end: base + token.end,
            replacement,
        });
    }
}

/// The declaration file text of a namespace: `declare namespace N { .. }` with
/// the members it exports, plus the types they mention that it does not.
///
/// Members are printed as a module's exports are, then stripped of `export
/// declare`, because inside an ambient namespace every member is exported
/// unless the namespace says otherwise. TypeScript says otherwise (an explicit
/// `export` on each exported member and a closing `export {};`) exactly when a
/// member that is not exported has to be printed because an exported one names
/// it, so that is when this does.
pub(super) fn emit_namespace_declaration(
    module: &Module,
    namespace: &NamespaceDeclaration,
    prefix: &str,
) -> Result<String, Diagnostic> {
    // `A.B.C` is one declaration with a dotted name.
    let mut name = namespace.name.clone();
    let mut current = namespace;
    while let [Declaration::Namespace(inner)] = current.body.as_slice() {
        if !inner.implicit {
            break;
        }
        name.push('.');
        name.push_str(&inner.name);
        current = inner;
    }
    let body = render_namespace_body(module, current)?;
    let mut output = format!("{prefix}namespace {name} {{");
    if body.is_empty() {
        output.push_str(" }\n");
    } else {
        output.push('\n');
        output.push_str(&body);
        output.push_str("}\n");
    }
    Ok(output)
}

fn is_exported_member(namespace: &NamespaceDeclaration, declaration: &Declaration) -> bool {
    let explicit = match declaration {
        Declaration::Variable(item) => item.exported,
        Declaration::Function(item) => item.exported,
        Declaration::Class(item) => item.exported,
        Declaration::Enum(item) => item.exported,
        Declaration::Interface(item) => item.exported,
        Declaration::TypeAlias(item) => item.exported,
        Declaration::Namespace(item) => item.exported,
        _ => false,
    };
    explicit || (namespace.exports_every_member() && declared_name(declaration).is_some())
}

fn declared_name(declaration: &Declaration) -> Option<&str> {
    match declaration {
        Declaration::Variable(item) => Some(&item.name),
        Declaration::Function(item) => Some(&item.name),
        Declaration::Class(item) => Some(&item.name),
        Declaration::Enum(item) => Some(&item.name),
        Declaration::Interface(item) => Some(&item.name),
        Declaration::TypeAlias(item) => Some(&item.name),
        Declaration::Namespace(item) => Some(&item.name),
        _ => None,
    }
}

/// A member as a module-level export, so the declaration printer prints it.
fn as_export(declaration: &Declaration) -> Declaration {
    let mut declaration = declaration.clone();
    match &mut declaration {
        Declaration::Variable(item) => {
            item.exported = true;
            item.declared = false;
        }
        Declaration::Function(item) => {
            item.exported = true;
            item.declared = false;
        }
        Declaration::Class(item) => item.exported = true,
        Declaration::Enum(item) => {
            item.exported = true;
        }
        Declaration::Interface(item) => item.exported = true,
        Declaration::TypeAlias(item) => item.exported = true,
        _ => {}
    }
    declaration
}

/// The identifier-like words of some text.
fn words(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !(character.is_alphanumeric() || matches!(character, '_' | '$')))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

fn render_namespace_body(
    module: &Module,
    namespace: &NamespaceDeclaration,
) -> Result<String, Diagnostic> {
    let body = &namespace.body;
    let mut included: Vec<bool> = body
        .iter()
        .map(|declaration| is_exported_member(namespace, declaration))
        .collect();
    let mut hidden: BTreeSet<usize> = BTreeSet::new();
    // A member that is not exported is printed when a printed member names it.
    loop {
        let text = render_members(module, namespace, &included, &hidden, false)?;
        let mentioned = words(&text);
        let mut changed = false;
        for (index, declaration) in body.iter().enumerate() {
            if included[index] {
                continue;
            }
            let typelike = matches!(
                declaration,
                Declaration::Interface(_)
                    | Declaration::TypeAlias(_)
                    | Declaration::Class(_)
                    | Declaration::Enum(_)
                    | Declaration::Namespace(_)
            );
            let named = declared_name(declaration).is_some_and(|name| mentioned.contains(name));
            if typelike && named {
                included[index] = true;
                hidden.insert(index);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let explicit = !hidden.is_empty();
    let mut text = render_members(module, namespace, &included, &hidden, explicit)?;
    if explicit {
        text.push_str("    export {};\n");
    }
    Ok(text)
}

/// The printed members, indented one level. In `explicit` mode each exported
/// member keeps its `export`; otherwise none shows it.
fn render_members(
    module: &Module,
    namespace: &NamespaceDeclaration,
    included: &[bool],
    hidden: &BTreeSet<usize>,
    explicit: bool,
) -> Result<String, Diagnostic> {
    let mut output = String::new();
    let mut chunk: Vec<(usize, Declaration)> = Vec::new();
    let flush =
        |chunk: &mut Vec<(usize, Declaration)>, output: &mut String| -> Result<(), Diagnostic> {
            if chunk.is_empty() {
                return Ok(());
            }
            let synthetic = Module {
                id: module.id.clone(),
                source: module.source.clone(),
                declarations: chunk
                    .iter()
                    .map(|(_, declaration)| declaration.clone())
                    .collect(),
                edits: Vec::new(),
                generic_call_type_arguments: BTreeMap::new(),
                nested_functions: BTreeMap::new(),
            };
            let text = super::emit_declaration(&synthetic)?;
            let hidden_names: BTreeSet<&str> = chunk
                .iter()
                .filter(|(index, _)| hidden.contains(index))
                .filter_map(|(_, declaration)| declared_name(declaration))
                .collect();
            for line in text.lines() {
                let mut line = line.to_string();
                if !line.starts_with(' ') && !line.starts_with('}') {
                    let stripped = line
                        .strip_prefix("export declare ")
                        .or_else(|| line.strip_prefix("export "))
                        .unwrap_or(&line)
                        .to_string();
                    let name = [
                        "const enum ",
                        "enum ",
                        "const ",
                        "let ",
                        "var ",
                        "function ",
                        "class ",
                        "interface ",
                        "type ",
                    ]
                    .iter()
                    .find_map(|keyword| stripped.strip_prefix(keyword))
                    .map(|rest| {
                        rest.split(|character: char| {
                            !(character.is_alphanumeric() || matches!(character, '_' | '$'))
                        })
                        .next()
                        .unwrap_or("")
                        .to_string()
                    })
                    .unwrap_or_default();
                    let keep_export = explicit && !hidden_names.contains(name.as_str());
                    line = format!("{}{stripped}", if keep_export { "export " } else { "" });
                }
                output.push_str(&format!("    {line}\n"));
            }
            chunk.clear();
            Ok(())
        };
    for (index, declaration) in namespace.body.iter().enumerate() {
        if !included[index] {
            continue;
        }
        match declaration {
            Declaration::Namespace(inner) => {
                flush(&mut chunk, &mut output)?;
                let prefix = if explicit && !hidden.contains(&index) {
                    "export "
                } else {
                    ""
                };
                let text = emit_namespace_declaration(module, inner, prefix)?;
                for line in text.lines() {
                    output.push_str(&format!("    {line}\n"));
                }
            }
            other => chunk.push((index, as_export(other))),
        }
    }
    flush(&mut chunk, &mut output)?;
    Ok(output)
}
