// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX lowering (`jsx: react`, `react-jsx`, `react-jsxdev`) for `.tsx` modules.
//!
//! The lowering follows TypeScript 5.9.3's JSX transform (read from the pinned
//! compiler): classic mode calls the factory with the tag, the props (or `null`)
//! and the children; the automatic runtime calls `jsx`/`jsxs`/`jsxDEV` imported
//! from `<importSource>/jsx-runtime` with the children in the props and the `key`
//! as a third argument, falling back to `createElement` when a `key` follows a
//! non-literal spread. Text children are trimmed and entity-decoded by TypeScript's
//! rules.
//!
//! Only the JSX *syntax* is replaced. Each embedded expression stays where it is
//! in the source (an `Item::Keep`), so every other pass reaches it: erased
//! annotations, CommonJS name rewriting, namespace references. Newlines of a
//! replaced region are kept, so every emitted line still maps to its source line.

use std::collections::BTreeMap;

use super::namespaces::Replacement;
use super::{Module, TextEdit};
use crate::compiler::{CompilerOptions, JsxMode};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::jsx::{self, JsxAttribute, JsxChild, JsxElement, JsxName, JsxValue, Pragmas};
use crate::{Token, TokenKind};

/// A piece of the output: new text, or a source range that stays as written.
enum Item {
    Text(String),
    Keep(usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Runtime {
    Classic,
    Automatic,
}

/// An import the automatic runtime needs, in order of first use.
#[derive(Default)]
struct RuntimeImports {
    /// `(module, imported name)` in order of first use.
    used: Vec<(String, &'static str)>,
}

struct Lowerer<'a> {
    module: &'a Module,
    tokens: &'a [Token],
    runtime: Runtime,
    dev: bool,
    commonjs: bool,
    import_source: String,
    factory: String,
    fragment: String,
    references: &'a BTreeMap<String, Replacement>,
    imports: RuntimeImports,
    uses_file_name: bool,
    items: Vec<Item>,
}

pub(super) fn lower_jsx(
    module: &Module,
    options: &CompilerOptions,
    references: &BTreeMap<String, Replacement>,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let Some(mode) = options.jsx.filter(|mode| mode.lowers()) else {
        return Ok(());
    };
    if !module.id.ends_with(".tsx") {
        return Ok(());
    }
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return Ok(());
    };
    if !tokens
        .iter()
        .any(|token| token.kind == TokenKind::JsxElement)
    {
        return Ok(());
    }
    let pragmas = Pragmas::of(&module.source);
    let runtime = match pragmas.runtime.as_deref() {
        Some("classic") => Runtime::Classic,
        Some("automatic") => Runtime::Automatic,
        _ if matches!(mode, JsxMode::ReactJsx | JsxMode::ReactJsxDev)
            || pragmas.import_source.is_some() =>
        {
            Runtime::Automatic
        }
        _ => Runtime::Classic,
    };
    let mut lowerer = Lowerer {
        module,
        tokens: &tokens,
        runtime,
        dev: mode == JsxMode::ReactJsxDev,
        commonjs: options.module_kind == crate::compiler::ModuleKind::CommonJs,
        import_source: pragmas
            .import_source
            .clone()
            .or_else(|| options.jsx_import_source.clone())
            .unwrap_or_else(|| "react".to_string()),
        factory: pragmas
            .factory
            .clone()
            .or_else(|| options.jsx_factory.clone())
            .unwrap_or_else(|| "React.createElement".to_string()),
        fragment: pragmas
            .fragment
            .clone()
            .or_else(|| options.jsx_fragment_factory.clone())
            .unwrap_or_else(|| "React.Fragment".to_string()),
        references,
        imports: RuntimeImports::default(),
        uses_file_name: false,
        items: Vec::new(),
    };
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::JsxElement {
            continue;
        }
        let element =
            crate::syntax::parse_jsx(&module.id, &module.source, token.start).map_err(|error| {
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    SourceSpan::new(&module.id, error.offset, error.offset),
                    error.message,
                )
            })?;
        // TypeScript reports a top-level element's position from the end of the
        // token before it (its leading trivia belongs to the element).
        let position = tokens[..index]
            .iter()
            .rev()
            .find(|previous| previous.end > previous.start)
            .map_or(0, |previous| previous.end);
        lowerer.items.clear();
        lowerer.element(&element, position)?;
        let items = std::mem::take(&mut lowerer.items);
        push_edits(module, &element, items, edits);
    }
    lowerer.runtime_prologue(edits)?;
    Ok(())
}

/// Turns an element's items into edits: each gap between kept ranges is replaced,
/// keeping its line breaks.
fn push_edits(module: &Module, element: &JsxElement, items: Vec<Item>, edits: &mut Vec<TextEdit>) {
    let mut cursor = element.start;
    let mut text = String::new();
    let flush = |text: &mut String, from: usize, to: usize, edits: &mut Vec<TextEdit>| {
        let lines = module.source[from..to].matches('\n').count();
        text.push_str(&"\n".repeat(lines));
        if from != to || !text.is_empty() {
            edits.push(TextEdit {
                start: from,
                end: to,
                replacement: std::mem::take(text),
            });
        }
    };
    for item in items {
        match item {
            Item::Text(more) => text.push_str(&more),
            Item::Keep(start, end) => {
                flush(&mut text, cursor, start, edits);
                cursor = end;
            }
        }
    }
    flush(&mut text, cursor, element.end, edits);
}

fn string_literal(text: &str, single_quote: bool) -> String {
    let quote = if single_quote { '\'' } else { '"' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            character if character == quote => {
                out.push('\\');
                out.push(character);
            }
            character => out.push(character),
        }
    }
    out.push(quote);
    out
}

/// TypeScript writes an attribute name bare when it matches `[A-Z_]\w*`.
fn attribute_name(name: &str) -> String {
    let mut characters = name.chars();
    let bare = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if bare {
        name.to_string()
    } else {
        string_literal(name, false)
    }
}

impl Lowerer<'_> {
    fn text(&mut self, text: impl Into<String>) {
        self.items.push(Item::Text(text.into()));
    }

    fn error(&self, offset: usize, message: &str) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UnsupportedSyntax,
            SourceSpan::new(&self.module.id, offset, offset),
            message,
        )
    }

    /// A kept expression, parenthesized when it has a top-level comma.
    fn keep(&mut self, start: usize, end: usize, tokens: &[Token]) {
        let mut depth = 0usize;
        let sequence = tokens.iter().any(|token| match token.text.as_str() {
            "(" | "[" | "{" => {
                depth += 1;
                false
            }
            ")" | "]" | "}" => {
                depth = depth.saturating_sub(1);
                false
            }
            "," if depth == 0 && token.end > token.start => true,
            _ => false,
        });
        if sequence {
            self.text("(");
        }
        self.items.push(Item::Keep(start, end));
        if sequence {
            self.text(")");
        }
    }

    /// The expression for a dotted entity (`React.createElement`), with its root
    /// name rewritten the way the module's other references are (`react_1.default`),
    /// called as `(0, f)` when that is needed to not bind `this`.
    fn entity(&self, entity: &str, called: bool) -> String {
        let root = entity.split('.').next().unwrap_or(entity);
        let tail = &entity[root.len()..];
        match self.references.get(root) {
            Some(replacement) => {
                let text = format!("{}{tail}", replacement.text);
                if called && tail.is_empty() && replacement.wrap_calls {
                    format!("(0, {text})")
                } else {
                    text
                }
            }
            None => entity.to_string(),
        }
    }

    /// The name an automatic-runtime import is used by.
    fn runtime_name(&mut self, module: &str, name: &'static str, called: bool) -> String {
        if !self
            .imports
            .used
            .iter()
            .any(|(used_module, used_name)| used_module == module && *used_name == name)
        {
            self.imports.used.push((module.to_string(), name));
        }
        if self.commonjs {
            let variable = runtime_variable(module);
            if called {
                format!("(0, {variable}.{name})")
            } else {
                format!("{variable}.{name}")
            }
        } else {
            format!("_{name}")
        }
    }

    fn jsx_module(&self) -> String {
        format!(
            "{}/{}",
            self.import_source,
            if self.dev {
                "jsx-dev-runtime"
            } else {
                "jsx-runtime"
            }
        )
    }

    fn tag(&mut self, name: &JsxName) {
        if jsx::is_intrinsic_name(&name.text) {
            self.text(string_literal(&name.text, false));
        } else {
            let text = self.entity(&name.text, false);
            self.text(text);
        }
    }

    fn element(&mut self, element: &JsxElement, position: usize) -> Result<(), Diagnostic> {
        let key = self.key_attribute(element);
        let use_create_element = self.runtime == Runtime::Classic
            || key_follows_spread(element, self.tokens_of_spreads(element));
        let semantic = semantic_children(self.module, &element.children);
        if self.runtime == Runtime::Automatic && !use_create_element {
            return self.automatic(element, position, key, &semantic);
        }
        // Classic (or the `createElement` fallback): factory(tag, props, ...children).
        let callee = if self.runtime == Runtime::Classic {
            let factory = self.factory.clone();
            self.entity(&factory, true)
        } else {
            let source = self.import_source.clone();
            self.runtime_name(&source, "createElement", true)
        };
        self.text(format!("{callee}("));
        match &element.name {
            None => {
                let fragment = if self.runtime == Runtime::Classic {
                    let fragment = self.fragment.clone();
                    self.entity(&fragment, false)
                } else {
                    let module = self.jsx_module();
                    self.runtime_name(&module, "Fragment", false)
                };
                self.text(fragment);
            }
            Some(name) => self.tag(name),
        }
        self.text(", ");
        self.classic_props(element)?;
        for child in &element.children {
            self.classic_child(child)?;
        }
        self.text(")");
        Ok(())
    }

    fn tokens_of_spreads<'e>(&self, element: &'e JsxElement) -> Vec<&'e [Token]> {
        element
            .attributes
            .iter()
            .filter_map(|attribute| match attribute {
                JsxAttribute::Spread { tokens, .. } => Some(tokens.as_slice()),
                _ => None,
            })
            .collect()
    }

    fn key_attribute<'e>(&self, element: &'e JsxElement) -> Option<&'e JsxAttribute> {
        element.attributes.iter().find(
            |attribute| matches!(attribute, JsxAttribute::Named { name, .. } if name.text == "key"),
        )
    }

    fn attribute_value(&mut self, value: &Option<JsxValue>, at: usize) -> Result<(), Diagnostic> {
        match value {
            None => self.text("true"),
            Some(JsxValue::String { start, end }) => {
                let raw = &self.module.source[*start..*end];
                let single = raw.starts_with('\'');
                let inner = &raw[1..raw.len() - 1];
                self.text(string_literal(&jsx::decode_entities(inner), single));
            }
            Some(JsxValue::Expression {
                start, end, tokens, ..
            }) => self.keep(*start, *end, tokens),
            Some(JsxValue::Element(element)) => self.element(element, element.start)?,
        }
        let _ = at;
        Ok(())
    }

    fn classic_props(&mut self, element: &JsxElement) -> Result<(), Diagnostic> {
        if element.attributes.is_empty() {
            self.text("null");
            return Ok(());
        }
        self.props_object(&element.attributes, None)
    }

    /// `{ a: 1, ...b, c: 2 }`, optionally followed by one more entry.
    fn props_object(
        &mut self,
        attributes: &[JsxAttribute],
        children: Option<&[&JsxChild]>,
    ) -> Result<(), Diagnostic> {
        self.text("{ ");
        let mut first = true;
        for attribute in attributes {
            if !first {
                self.text(", ");
            }
            first = false;
            match attribute {
                JsxAttribute::Spread {
                    start, end, tokens, ..
                } => {
                    self.text("...");
                    self.keep(*start, *end, tokens);
                }
                JsxAttribute::Named {
                    name, value, start, ..
                } => {
                    self.text(format!("{}: ", attribute_name(&name.text)));
                    self.attribute_value(value, *start)?;
                }
            }
        }
        if let Some(children) = children {
            if !first {
                self.text(", ");
            }
            self.automatic_children(children)?;
        }
        self.text(" }");
        Ok(())
    }

    fn classic_child(&mut self, child: &JsxChild) -> Result<(), Diagnostic> {
        match child {
            JsxChild::Text { start, end } => {
                if let Some(value) = jsx::text_value(&self.module.source[*start..*end]) {
                    self.text(format!(", {}", string_literal(&value, false)));
                }
            }
            JsxChild::Expression {
                start,
                end,
                tokens,
                spread,
                ..
            } => {
                if tokens.is_empty() && !*spread {
                    return Ok(());
                }
                self.text(if *spread { ", ..." } else { ", " });
                self.keep(*start, *end, tokens);
            }
            JsxChild::Element(element) => {
                self.text(", ");
                self.element(element, element.start)?;
            }
        }
        Ok(())
    }

    fn automatic(
        &mut self,
        element: &JsxElement,
        position: usize,
        key: Option<&JsxAttribute>,
        semantic: &[&JsxChild],
    ) -> Result<(), Diagnostic> {
        let is_static = semantic.len() > 1
            || semantic
                .first()
                .is_some_and(|child| matches!(child, JsxChild::Expression { spread: true, .. }));
        let name = if self.dev {
            "jsxDEV"
        } else if is_static {
            "jsxs"
        } else {
            "jsx"
        };
        let module = self.jsx_module();
        let callee = self.runtime_name(&module, name, true);
        self.text(format!("{callee}("));
        match &element.name {
            None => {
                let fragment = self.runtime_name(&module, "Fragment", false);
                self.text(fragment);
            }
            Some(tag) => self.tag(tag),
        }
        self.text(", ");
        let attributes: Vec<JsxAttribute> = element
            .attributes
            .iter()
            .filter(|attribute| {
                !matches!(attribute, JsxAttribute::Named { name, .. } if name.text == "key")
            })
            .cloned()
            .collect();
        if attributes.is_empty() && semantic.is_empty() {
            self.text("{}");
        } else {
            let children = (!semantic.is_empty()).then_some(semantic);
            self.props_object(&attributes, children)?;
        }
        let mut wrote_key = false;
        if let Some(JsxAttribute::Named { value, .. }) = key {
            self.text(", ");
            // Moved after the props, so it is written out rather than kept in place.
            self.copy_value(value);
            wrote_key = true;
        }
        if self.dev {
            if !wrote_key {
                self.text(", void 0");
            }
            self.text(format!(", {}", if is_static { "true" } else { "false" }));
            let (line, column) = self.line_and_column(position);
            self.uses_file_name = true;
            self.text(format!(
                ", {{ fileName: _jsxFileName, lineNumber: {line}, columnNumber: {column} }}, this"
            ));
        }
        self.text(")");
        Ok(())
    }

    /// An attribute value written as text, for the places that move it.
    fn copy_value(&mut self, value: &Option<JsxValue>) {
        match value {
            None => self.text("true"),
            Some(JsxValue::String { start, end }) => {
                let raw = &self.module.source[*start..*end];
                let single = raw.starts_with('\'');
                let inner = &raw[1..raw.len() - 1];
                self.text(string_literal(&jsx::decode_entities(inner), single));
            }
            Some(JsxValue::Expression { start, end, .. }) => {
                let text = self.module.source[*start..*end].trim().to_string();
                let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
                self.text(compact);
            }
            Some(JsxValue::Element(element)) => {
                let _ = element;
            }
        }
    }

    fn automatic_children(&mut self, semantic: &[&JsxChild]) -> Result<(), Diagnostic> {
        self.text("children: ");
        if let [only] = semantic {
            if !matches!(only, JsxChild::Expression { spread: true, .. }) {
                return self.automatic_child(only);
            }
        }
        self.text("[");
        for (index, child) in semantic.iter().enumerate() {
            if index > 0 {
                self.text(", ");
            }
            if matches!(child, JsxChild::Expression { spread: true, .. }) {
                self.text("...");
            }
            self.automatic_child(child)?;
        }
        self.text("]");
        Ok(())
    }

    fn automatic_child(&mut self, child: &JsxChild) -> Result<(), Diagnostic> {
        match child {
            JsxChild::Text { start, end } => {
                let value = jsx::text_value(&self.module.source[*start..*end]).unwrap_or_default();
                self.text(string_literal(&value, false));
            }
            JsxChild::Expression {
                start, end, tokens, ..
            } => self.keep(*start, *end, tokens),
            JsxChild::Element(element) => self.element(element, element.start)?,
        }
        Ok(())
    }

    fn line_and_column(&self, offset: usize) -> (usize, usize) {
        let before = &self.module.source[..offset.min(self.module.source.len())];
        let line = before.matches('\n').count() + 1;
        let start = before.rfind('\n').map_or(0, |at| at + 1);
        let column = before[start..].encode_utf16().count() + 1;
        (line, column)
    }

    /// The imports (or `require`s) and the dev file-name constant, inserted after
    /// the file's directive prologue.
    fn runtime_prologue(&mut self, edits: &mut Vec<TextEdit>) -> Result<(), Diagnostic> {
        if self.imports.used.is_empty() && !self.uses_file_name {
            return Ok(());
        }
        let at = prologue_end(self.module, self.tokens);
        let mut text = String::new();
        let mut modules: Vec<&str> = Vec::new();
        for (module, _) in &self.imports.used {
            if !modules.contains(&module.as_str()) {
                modules.push(module);
            }
        }
        for module in modules {
            if self.commonjs {
                text.push_str(&format!(
                    "const {} = require({}); ",
                    runtime_variable(module),
                    string_literal(module, false)
                ));
            } else {
                let names: Vec<String> = self
                    .imports
                    .used
                    .iter()
                    .filter(|(used, _)| used == module)
                    .map(|(_, name)| format!("{name} as _{name}"))
                    .collect();
                text.push_str(&format!(
                    "import {{ {} }} from {}; ",
                    names.join(", "),
                    string_literal(module, false)
                ));
            }
        }
        if self.uses_file_name {
            text.push_str(&format!(
                "const _jsxFileName = {}; ",
                string_literal(&self.module.id, false)
            ));
        }
        for reserved in [
            "_jsx",
            "_jsxs",
            "_jsxDEV",
            "_Fragment",
            "_createElement",
            "_jsxFileName",
        ] {
            if self
                .tokens
                .iter()
                .any(|token| token.kind == TokenKind::Identifier && token.text == reserved)
            {
                return Err(self.error(
                    0,
                    "a name the JSX runtime imports (such as `_jsx`) is already used in this module",
                ));
            }
        }
        edits.push(TextEdit {
            start: at,
            end: at,
            replacement: text,
        });
        Ok(())
    }
}

/// `react/jsx-runtime` is required as `jsx_runtime_1`, `react` as `react_1`.
fn runtime_variable(module: &str) -> String {
    let last = module.rsplit('/').next().unwrap_or(module);
    let name: String = last
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("{name}_1")
}

/// The children that count: not whitespace-only text with a line break, not an
/// empty `{}` (a spread child counts).
fn semantic_children<'e>(module: &Module, children: &'e [JsxChild]) -> Vec<&'e JsxChild> {
    children
        .iter()
        .filter(|child| match child {
            JsxChild::Text { start, end } => {
                !jsx::is_blank_with_newline(&module.source[*start..*end])
            }
            JsxChild::Expression { tokens, spread, .. } => *spread || !tokens.is_empty(),
            JsxChild::Element(_) => true,
        })
        .collect()
}

/// A `key` after a spread that is not a plain object literal needs
/// `createElement`: the spread might itself carry a `key`.
fn key_follows_spread(element: &JsxElement, spreads: Vec<&[Token]>) -> bool {
    let mut spread_index = 0;
    let mut spread = false;
    for attribute in &element.attributes {
        match attribute {
            JsxAttribute::Spread { .. } => {
                let tokens = spreads[spread_index];
                spread_index += 1;
                let literal = tokens.first().is_some_and(|first| first.text == "{")
                    && tokens.last().is_some_and(|last| last.text == "}")
                    && !tokens.iter().any(|token| token.text == "...");
                if !literal {
                    spread = true;
                }
            }
            JsxAttribute::Named { name, .. } if spread && name.text == "key" => return true,
            _ => {}
        }
    }
    false
}

/// Where the runtime import goes: after the leading directive statements
/// (`"use strict";`), else at the start.
fn prologue_end(module: &Module, tokens: &[Token]) -> usize {
    let mut at = 0;
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        if token.kind != TokenKind::String {
            break;
        }
        match tokens.get(index + 1) {
            Some(next) if next.is(";") => {
                at = next.end;
                index += 2;
            }
            Some(next) if module.source[token.end..next.start].contains('\n') => {
                at = token.end;
                index += 1;
            }
            _ => break,
        }
    }
    at
}
