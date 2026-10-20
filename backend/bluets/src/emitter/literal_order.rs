// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned library allocation facts order proven fresh-literal return unions.
//! This changes declaration presentation only, never bindings or runtime grants.

use super::*;
use crate::parser::{
    FunctionBodyItem, FunctionDeclaration, NestedFunctionBody, VariableDeclaration,
};
use crate::syntax::{Token, TokenKind};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::OnceLock;

pub(crate) const VERSION: &str = "bluets-declaration-literal-order/1/ts5.9.3";
const SOURCE: &str = include_str!("literal_order/native-5.9.3.json");

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Text(String),
    Number(u64),
}

#[derive(Deserialize)]
struct Catalog {
    schema: u8,
    typescript: String,
    profiles: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    target: String,
    mode: String,
    values: Vec<serde_json::Value>,
}

type Ranks = BTreeMap<Key, usize>;
static PROFILES: OnceLock<BTreeMap<String, Ranks>> = OnceLock::new();

fn profiles() -> &'static BTreeMap<String, Ranks> {
    PROFILES.get_or_init(|| {
        let source: Catalog =
            serde_json::from_str(SOURCE).expect("versioned literal-order catalog");
        assert_eq!(source.schema, 1);
        assert_eq!(source.typescript, "5.9.3");
        source
            .profiles
            .into_iter()
            .map(|profile| {
                let values = profile
                    .values
                    .into_iter()
                    .enumerate()
                    .map(|(rank, value)| {
                        let key = match value {
                            serde_json::Value::String(text) => Key::Text(text),
                            serde_json::Value::Number(value) => {
                                number(value.as_f64().expect("finite native literal"))
                            }
                            _ => panic!("invalid versioned literal-order entry"),
                        };
                        (key, rank)
                    })
                    .collect();
                (format!("{}:{}", profile.target, profile.mode), values)
            })
            .collect()
    })
}

fn number(value: f64) -> Key {
    Key::Number(if value == 0.0 {
        0.0f64.to_bits()
    } else {
        value.to_bits()
    })
}
fn literal(text: &str) -> Option<Key> {
    if text.starts_with(['\'', '"']) {
        return crate::enum_eval::decode_plain_string(text).map(Key::Text);
    }
    let value = text.parse::<f64>().ok()?;
    value.is_finite().then(|| number(value))
}

pub(super) struct Order {
    ranks: Ranks,
    tokens: Vec<Token>,
    erased: Vec<(usize, usize)>,
    nested: BTreeMap<usize, usize>,
}

impl Order {
    pub(super) fn new(module: &Module, options: &CompilerOptions) -> Result<Self, Diagnostic> {
        let mode = match &options.libraries {
            None => "default",
            Some(libs) if libs.is_empty() => "none",
            Some(_) => "explicit",
        };
        let target = options.library_target().unwrap_or(options.target);
        let mut ranks = profiles()
            .get(&format!("{}:{mode}", target.as_str()))
            .expect("owned target literal profile")
            .clone();
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &module.source,
            options.limits.parser.max_source_bytes,
            options.limits.parser.max_tokens,
        )
        .map_err(|errors| {
            errors
                .into_iter()
                .next()
                .expect("nonempty lexer diagnostics")
        })?;
        let mut erased: Vec<_> = module
            .edits
            .iter()
            .filter(|edit| edit.replacement.is_empty())
            .map(|edit| (edit.start, edit.end))
            .collect();
        collect_static_specifiers(&module.declarations, &mut erased);
        erased.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, end) in erased {
            if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        for (index, token) in tokens.iter().enumerate() {
            if covered(&merged, token.start, token.end) {
                continue;
            }
            let property = tokens
                .get(index + 1)
                .is_some_and(|next| matches!(next.text.as_str(), ":" | "?" | "(" | "="))
                && index
                    .checked_sub(1)
                    .and_then(|index| tokens.get(index))
                    .is_some_and(|previous| {
                        matches!(
                            previous.text.as_str(),
                            "{" | "," | ";" | "static" | "get" | "set"
                        )
                    });
            if !property && matches!(token.kind, TokenKind::String | TokenKind::Number) {
                if let Some(key) = literal(&token.text) {
                    let next = ranks.len();
                    ranks.entry(key).or_insert(next);
                }
            }
        }
        let mut nested: BTreeMap<_, _> = module
            .nested_functions
            .values()
            .map(|function| (function.span.start, function.span.end))
            .chain(
                module
                    .class_expressions
                    .values()
                    .map(|class| (class.class.span.start, class.class.span.end)),
            )
            .collect();
        collect_functions(&module.declarations, &mut nested);
        Ok(Self {
            ranks,
            tokens,
            erased: merged,
            nested,
        })
    }

    pub(super) fn function(&self, function: &FunctionDeclaration, value: &Type) -> Type {
        self.returned(
            function.body.first().map(item_start),
            function.span.end,
            value,
        )
    }

    pub(super) fn variable(
        &self,
        module: &Module,
        variable: &VariableDeclaration,
        value: &Type,
    ) -> Type {
        let mut value = super::inferred_returns::canonical(value);
        let Some(first) = variable.initializer.first() else {
            return value;
        };
        let Some(last) = variable.initializer.last() else {
            return value;
        };
        let Some(function) = module.nested_functions.values().find(|function| {
            function.return_type.is_none()
                && first.start <= function.span.start
                && function.span.end <= last.end
                && variable
                    .initializer
                    .iter()
                    .filter(|token| {
                        token.start < function.span.start || token.start >= function.span.end
                    })
                    .all(|token| token.is("(") || token.is(")"))
        }) else {
            return value;
        };
        let NestedFunctionBody::Block { items, .. } = &function.body else {
            return value;
        };
        if let Type::Function { result, .. } | Type::GenericFunction { result, .. } = &mut value {
            **result = self.returned(items.first().map(item_start), function.span.end, result);
        }
        value
    }

    fn returned(&self, start: Option<usize>, end: usize, value: &Type) -> Type {
        let mut value = super::inferred_returns::canonical(value);
        let Type::Union(parts) = &mut value else {
            return value;
        };
        let Some(keys): Option<Vec<_>> = parts
            .iter()
            .map(|part| {
                if let Type::Literal(text) = part {
                    literal(text)
                } else {
                    None
                }
            })
            .collect()
        else {
            return value;
        };
        let Some(start) = start else {
            return value;
        };
        let Some(returned) = self.direct_returns(start, end) else {
            return value;
        };
        let expected: BTreeSet<_> = keys.iter().cloned().collect();
        if returned != expected || keys.iter().any(|key| !self.ranks.contains_key(key)) {
            return value;
        }
        parts.sort_by_key(|part| {
            if let Type::Literal(text) = part {
                self.ranks[&literal(text).expect("proved literal")]
            } else {
                unreachable!()
            }
        });
        value
    }

    fn direct_returns(&self, start: usize, end: usize) -> Option<BTreeSet<Key>> {
        let mut returned = BTreeSet::new();
        let mut index = self.tokens.partition_point(|token| token.start < start);
        while let Some(token) = self.tokens.get(index).filter(|token| token.start < end) {
            if let Some(nested_end) = self
                .nested
                .get(&token.start)
                .filter(|nested_end| **nested_end <= end)
            {
                index = self
                    .tokens
                    .partition_point(|token| token.start < *nested_end);
                continue;
            }
            if token.is("class") || token.is("function") {
                return None;
            }
            if token.is("return")
                && !covered(&self.erased, token.start, token.end)
                && !index
                    .checked_sub(1)
                    .and_then(|index| self.tokens.get(index))
                    .is_some_and(|previous| previous.is(".") || previous.is("?."))
            {
                let operand = self.tokens.get(index + 1)?;
                let next = self.tokens.get(index + 2)?;
                if !matches!(operand.kind, TokenKind::String | TokenKind::Number)
                    || !matches!(next.text.as_str(), ";" | "}")
                {
                    return None;
                }
                returned.insert(literal(&operand.text)?);
            }
            index += 1;
        }
        Some(returned)
    }
}

fn covered(ranges: &[(usize, usize)], start: usize, end: usize) -> bool {
    ranges
        .partition_point(|range| range.0 <= start)
        .checked_sub(1)
        .is_some_and(|index| end <= ranges[index].1)
}
fn item_start(item: &FunctionBodyItem) -> usize {
    match item {
        FunctionBodyItem::Variable(value) => value.span.start,
        FunctionBodyItem::Function(value) => value.span.start,
        FunctionBodyItem::If(value) => value.span.start,
        FunctionBodyItem::While(value) => value.span.start,
        FunctionBodyItem::Try(value) => value.span.start,
        FunctionBodyItem::Return { span, .. }
        | FunctionBodyItem::Throw { span, .. }
        | FunctionBodyItem::Expression { span, .. }
        | FunctionBodyItem::Opaque(span) => span.start,
    }
}
fn collect_static_specifiers(declarations: &[Declaration], ranges: &mut Vec<(usize, usize)>) {
    for declaration in declarations {
        match declaration {
            Declaration::Import(value) => ranges.push((value.span.start, value.span.end)),
            Declaration::ValueExport(value) if value.specifier.is_some() => {
                ranges.push((value.span.start, value.span.end))
            }
            Declaration::Namespace(value) => collect_static_specifiers(&value.body, ranges),
            Declaration::Ambient(value) => collect_static_specifiers(&value.body, ranges),
            _ => {}
        }
    }
}
fn collect_functions(declarations: &[Declaration], nested: &mut BTreeMap<usize, usize>) {
    for declaration in declarations {
        match declaration {
            Declaration::Function(function) => {
                nested.insert(function.span.start, function.span.end);
                collect_body(&function.body, nested);
            }
            Declaration::Class(class) => {
                nested.insert(class.span.start, class.span.end);
            }
            Declaration::Namespace(value) => collect_functions(&value.body, nested),
            Declaration::Ambient(value) => collect_functions(&value.body, nested),
            _ => {}
        }
    }
}
fn collect_body(items: &[FunctionBodyItem], nested: &mut BTreeMap<usize, usize>) {
    for item in items {
        match item {
            FunctionBodyItem::Function(function) => {
                nested.insert(function.span.start, function.span.end);
                collect_body(&function.body, nested);
            }
            FunctionBodyItem::If(value) => {
                collect_body(&value.consequent, nested);
                match &value.alternate {
                    Some(crate::parser::FunctionElseBranch::Braced(body)) => {
                        collect_body(body, nested)
                    }
                    Some(crate::parser::FunctionElseBranch::ElseIf(branch)) => {
                        collect_body(&[FunctionBodyItem::If((**branch).clone())], nested)
                    }
                    None => {}
                }
            }
            FunctionBodyItem::While(value) => collect_body(&value.body, nested),
            FunctionBodyItem::Try(value) => {
                collect_body(&value.block, nested);
                if let Some(handler) = &value.handler {
                    collect_body(&handler.body, nested);
                }
                if let Some(finalizer) = &value.finalizer {
                    collect_body(finalizer, nested);
                }
            }
            _ => {}
        }
    }
}
