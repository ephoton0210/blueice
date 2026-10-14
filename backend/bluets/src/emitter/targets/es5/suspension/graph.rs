// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structured branch and exception regions compile to explicit continuations.

use super::*;
use crate::parser::{FunctionBodyItem, FunctionDeclaration, FunctionElseBranch};
use std::collections::BTreeSet;

mod control;
mod expressions;

pub(super) struct Graph {
    pub(super) entry: usize,
    pub(super) declarations: String,
    pub(super) cases: String,
}

pub(super) fn compile(
    module: &Module,
    function: &FunctionDeclaration,
    context: &str,
    arguments: &str,
    marker: Option<&str>,
    limits: &crate::parser::ParserLimits,
) -> Option<Graph> {
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        limits.max_source_bytes,
        limits.max_tokens,
    )
    .ok()?;
    let mut builder = Builder {
        module,
        context,
        arguments,
        marker,
        cases: Vec::new(),
        locals: BTreeSet::new(),
        functions: String::new(),
        depth: limits.max_type_depth.min(128),
        tokens,
        loops: Vec::new(),
        regions: 0,
    };
    let terminal = builder.block("return {kind:\"return\", value:void 0};".into());
    let entry = builder.sequence(&function.body, terminal)?;
    let mut declarations = builder.functions;
    if !builder.locals.is_empty() {
        declarations.push_str(&format!(
            "var {};\n",
            builder.locals.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    let cases = builder
        .cases
        .into_iter()
        .enumerate()
        .map(|(label, code)| format!("case {label}: {code}\n"))
        .collect();
    Some(Graph {
        entry,
        declarations,
        cases,
    })
}

struct Builder<'a> {
    module: &'a Module,
    context: &'a str,
    arguments: &'a str,
    marker: Option<&'a str>,
    cases: Vec<String>,
    locals: BTreeSet<String>,
    functions: String,
    depth: usize,
    loops: Vec<(usize, usize, usize)>,
    regions: usize,
    tokens: Vec<Token>,
}

impl Builder<'_> {
    fn block(&mut self, code: String) -> usize {
        let label = self.cases.len();
        self.cases.push(code);
        label
    }

    fn jump(&self, label: usize) -> String {
        format!("{}.label = {label}; continue;", self.context)
    }

    fn sequence(&mut self, items: &[FunctionBodyItem], mut next: usize) -> Option<usize> {
        self.depth = self.depth.checked_sub(1)?;
        let mut retained = Vec::new();
        let mut index = 0;
        while index < items.len() {
            if let Some(end) = self.control_end(items, index)? {
                retained.push((index, end, true));
                index = end;
            } else {
                retained.push((index, index + 1, false));
                index += 1;
            }
        }
        for (start, end, control) in retained.into_iter().rev() {
            next = if control {
                self.control(items, start, end, next)?
            } else {
                self.statement(&items[start], next)?
            };
        }
        self.depth += 1;
        Some(next)
    }

    fn statement(&mut self, item: &FunctionBodyItem, next: usize) -> Option<usize> {
        let context = self.context;
        match item {
            FunctionBodyItem::Expression { tokens, .. } => self.expression(tokens, None, "", next),
            FunctionBodyItem::Return { tokens, .. } => {
                self.expression(tokens, None, "return", next)
            }
            FunctionBodyItem::Throw { tokens, .. } => self.expression(tokens, None, "throw", next),
            FunctionBodyItem::Variable(variable) if variable.pattern.is_none() => {
                self.locals.insert(variable.name.clone());
                if variable.initializer.is_empty() {
                    Some(self.block(self.jump(next)))
                } else {
                    self.expression(&variable.initializer, Some(&variable.name), "", next)
                }
            }
            FunctionBodyItem::If(statement) => {
                if self.suspends(&statement.test) {
                    return None;
                }
                let consequent = self.sequence(&statement.consequent, next)?;
                let alternate = match &statement.alternate {
                    Some(FunctionElseBranch::Braced(items)) => self.sequence(items, next)?,
                    Some(FunctionElseBranch::ElseIf(statement)) => {
                        self.statement(&FunctionBodyItem::If((**statement).clone()), next)?
                    }
                    None => next,
                };
                let test = self.text(&statement.test)?;
                Some(self.block(format!(
                    "{context}.label = ({test}) ? {consequent} : {alternate}; continue;"
                )))
            }
            FunctionBodyItem::Try(statement) => {
                self.regions += 1;
                let leave = self.block("return {kind:\"leave\"};".into());
                let finish = self.block("return {kind:\"finish\"};".into());
                let finalizer = statement
                    .finalizer
                    .as_ref()
                    .map(|items| self.sequence(items, finish))
                    .transpose_option()?;
                let handler = if let Some(handler) = &statement.handler {
                    self.locals.insert(handler.binding.clone());
                    let entry = self.sequence(&handler.body, leave)?;
                    Some(self.block(format!(
                        "{} = {context}.exception; {}",
                        handler.binding,
                        self.jump(entry)
                    )))
                } else {
                    None
                };
                let entry = self.sequence(&statement.block, leave)?;
                self.regions -= 1;
                Some(self.block(format!("return {{kind:\"enter\", entry:{entry}, caught:{}, finalizer:{}, after:{next}}};",
                    handler.map_or("-1".into(),|label|label.to_string()),finalizer.map_or("-1".into(),|label|label.to_string()))))
            }
            FunctionBodyItem::Function(function) => {
                self.functions
                    .push_str(&self.module.source[function.span.start..function.span.end]);
                self.functions.push('\n');
                Some(next)
            }
            FunctionBodyItem::While(statement) => {
                self.while_loop(&statement.test, &statement.body, next, None)
            }
            _ => None,
        }
    }

    fn expression(
        &mut self,
        tokens: &[Token],
        binding: Option<&str>,
        completion: &str,
        next: usize,
    ) -> Option<usize> {
        self.depth = self.depth.checked_sub(1)?;
        let result = self.expression_inner(tokens, binding, completion, next);
        self.depth += 1;
        result
    }

    fn expression_inner(
        &mut self,
        tokens: &[Token],
        binding: Option<&str>,
        completion: &str,
        next: usize,
    ) -> Option<usize> {
        let context = self.context;
        if completion == "return" && self.marker.is_some() && !tokens.is_empty() {
            let temporary = self.temporary();
            let finish = self.block(format!("return {{kind:\"return\", value:{temporary}}};"));
            let resume = self.block(format!(
                "{temporary} = {context}.sent(); {}",
                self.jump(finish)
            ));
            let marker = self.marker?;
            let suspend = self.block(format!("{context}.label = {resume}; return {{kind:\"yield\", value:{marker}(true, {temporary})}};"));
            return self.expression(tokens, Some(&temporary), "", suspend);
        }
        if !self.suspends(tokens) {
            let value = self.text(tokens).unwrap_or_else(|| "void 0".into());
            let code = if completion.is_empty() {
                let assignment = binding.map_or(String::new(), |name| format!("{name} = "));
                format!("{assignment}{value}; {}", self.jump(next))
            } else {
                format!(
                    "return {{kind:{}, value:({value})}};",
                    serde_json::to_string(completion).ok()?
                )
            };
            return Some(self.block(code));
        }
        let first = tokens.first()?;
        if !matches!(first.text.as_str(), "yield" | "await") {
            return self.compound_expression(tokens, binding, completion, next);
        }
        let delegate = tokens.get(1).is_some_and(|token| token.is("*"));
        let operand = &tokens[1 + usize::from(delegate)..];
        let resume = if completion.is_empty() {
            let assignment = binding.map_or(String::new(), |name| format!("{name} = "));
            format!("{assignment}{context}.sent(); {}", self.jump(next))
        } else {
            format!(
                "return {{kind:{}, value:{context}.sent()}};",
                serde_json::to_string(completion).ok()?
            )
        };
        let resume = self.block(resume);
        if delegate && self.marker.is_some() {
            return None;
        }
        let temporary = self.temporary();
        let value = self.marker.map_or_else(
            || temporary.clone(),
            |marker| format!("{marker}({}, {temporary})", first.is("await")),
        );
        let suspend = self.block(format!(
            "{context}.label = {resume}; return {{kind:{}, value:({value})}};",
            if delegate {
                "\"delegate\""
            } else {
                "\"yield\""
            }
        ));
        self.expression(operand, Some(&temporary), "", suspend)
    }

    fn temporary(&mut self) -> String {
        let mut index = self.locals.len();
        loop {
            let name = format!("{}_value_{index}", self.context);
            if !self.module.source.contains(&name) && self.locals.insert(name.clone()) {
                return name;
            }
            index += 1;
        }
    }

    fn suspends(&self, tokens: &[Token]) -> bool {
        tokens.iter().any(|token| {
            matches!(token.text.as_str(), "yield" | "await")
                && token.kind == TokenKind::Keyword
                && !self.module.nested_functions.values().any(|function| {
                    token.start >= function.span.start && token.end <= function.span.end
                })
        })
    }

    fn text(&mut self, tokens: &[Token]) -> Option<String> {
        let (first, last) = (tokens.first()?, tokens.last()?);
        let mut edits = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            if token.is("arguments")
                && !index
                    .checked_sub(1)
                    .is_some_and(|before| tokens[before].is("."))
                && !tokens.get(index + 1).is_some_and(|next| next.is(":"))
                && !self.module.nested_functions.values().any(|function| {
                    token.start >= function.span.start && token.end <= function.span.end
                })
            {
                edits.push(TextEdit {
                    start: token.start - first.start,
                    end: token.end - first.start,
                    replacement: self.arguments.into(),
                });
            }
        }
        Some(apply_edits(&self.module.source[first.start..last.end], edits).javascript)
    }
}

trait OptionalContinuation {
    fn transpose_option(self) -> Option<Option<usize>>;
}

impl OptionalContinuation for Option<Option<usize>> {
    fn transpose_option(self) -> Option<Option<usize>> {
        match self {
            Some(Some(value)) => Some(Some(value)),
            Some(None) => None,
            None => Some(None),
        }
    }
}
