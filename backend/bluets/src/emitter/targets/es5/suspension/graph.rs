// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structured branch and exception regions compile to explicit continuations.

use super::*;
use crate::emitter::relocations;
use crate::parser::{FunctionBodyItem, FunctionDeclaration, FunctionElseBranch};
use std::collections::{BTreeMap, BTreeSet};

mod control;
mod expressions;
mod origins;
mod switch;

pub(super) struct Graph {
    pub(super) entry: usize,
    pub(super) declarations: String,
    pub(super) cases: String,
    pub(super) native: Option<NativeBlocks>,
}

pub(super) struct NativeBlocks {
    pub(super) blocks: Vec<String>,
    pub(super) exits: BTreeMap<usize, usize>,
    pub(super) finishes: BTreeSet<usize>,
    pub(super) delegation: bool,
}

pub(super) fn compile(
    (module, source_offset, previous): (&Module, usize, &EmittedJavaScript),
    function: &FunctionDeclaration,
    context: &str,
    arguments: &str,
    marker: Option<&str>,
    external_abi: bool,
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
        source_offset,
        previous,
        context,
        arguments,
        marker,
        asynchronous: function.async_function,
        external_abi,
        exits: BTreeMap::new(),
        finishes: BTreeSet::new(),
        delegation: false,
        cases: Vec::new(),
        locals: BTreeSet::new(),
        bindings: BTreeMap::new(),
        binding_initializers: BTreeMap::new(),
        case_punctuation: BTreeMap::new(),
        predicates: BTreeMap::new(),
        completion_site: None,
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
        .iter()
        .enumerate()
        .map(|(label, code)| format!("case {label}: {code}\n"))
        .collect();
    Some(Graph {
        entry,
        declarations,
        cases,
        native: external_abi.then_some(NativeBlocks {
            blocks: builder.cases,
            exits: builder.exits,
            finishes: builder.finishes,
            delegation: builder.delegation,
        }),
    })
}

struct Builder<'a> {
    module: &'a Module,
    source_offset: usize,
    previous: &'a EmittedJavaScript,
    context: &'a str,
    arguments: &'a str,
    marker: Option<&'a str>,
    asynchronous: bool,
    external_abi: bool,
    exits: BTreeMap<usize, usize>,
    finishes: BTreeSet<usize>,
    delegation: bool,
    cases: Vec<String>,
    locals: BTreeSet<String>,
    bindings: BTreeMap<String, (usize, usize)>,
    binding_initializers: BTreeMap<String, origins::InitializerOrigin>,
    case_punctuation: BTreeMap<String, usize>,
    predicates: BTreeMap<String, origins::PredicateOrigin>,
    completion_site: Option<(usize, usize, usize)>,
    functions: String,
    depth: usize,
    loops: Vec<JumpTargets>,
    regions: usize,
    tokens: Vec<Token>,
}

#[derive(Clone, Copy)]
struct JumpTargets {
    broken: (usize, usize),
    continued: Option<(usize, usize)>,
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
            FunctionBodyItem::Return { tokens, span } => {
                self.scoped_completion(tokens, "return", span, next)
            }
            FunctionBodyItem::Throw { tokens, span } => {
                self.scoped_completion(tokens, "throw", span, next)
            }
            FunctionBodyItem::Variable(variable) if variable.pattern.is_none() => {
                self.locals.insert(variable.name.clone());
                if let Some(token) = self.tokens.iter().find(|token| {
                    token.start >= variable.span.start
                        && token.end <= variable.span.end
                        && token.text == variable.name
                }) {
                    self.bindings
                        .insert(variable.name.clone(), (token.start, token.end));
                }
                if variable.initializer.is_empty() {
                    Some(self.block(self.jump(next)))
                } else {
                    self.binding_initializers.insert(
                        variable.name.clone(),
                        origins::InitializerOrigin {
                            start: variable.initializer.first()?.start,
                            end: variable.initializer.last()?.end,
                            statement: true,
                        },
                    );
                    self.expression(&variable.initializer, Some(&variable.name), "", next)
                }
            }
            FunctionBodyItem::If(statement) => {
                let consequent = self.sequence(&statement.consequent, next)?;
                let alternate = match &statement.alternate {
                    Some(FunctionElseBranch::Braced(items)) => self.sequence(items, next)?,
                    Some(FunctionElseBranch::ElseIf(statement)) => {
                        self.statement(&FunctionBodyItem::If((**statement).clone()), next)?
                    }
                    None => next,
                };
                let test = self.temporary();
                self.predicate_origin(&test, statement.span.start, &statement.test);
                let branch = self.block(format!(
                    "{context}.label = {test} ? {consequent} : {alternate}; continue;"
                ));
                self.expression(&statement.test, Some(&test), "", branch)
            }
            FunctionBodyItem::Try(statement) => {
                self.regions += 1;
                let leave = self.block("return {kind:\"leave\"};".into());
                let finish = self.block("return {kind:\"finish\"};".into());
                if self.external_abi {
                    self.exits.insert(leave, next);
                    self.finishes.insert(finish);
                }
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
                self.functions.push_str(&relocations::relocated_verbatim(
                    self.source_offset + function.span.start,
                    self.source_offset + function.span.end,
                ));
                self.functions.push_str(relocations::unmapped());
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
            let previous = self.completion_site;
            // Native's implicit await(operand) retains the operand endpoint,
            // while its later return(sent()) stays on the return keyword.
            if let (Some(first), Some(last)) = (tokens.first(), tokens.last()) {
                self.binding_initializers.insert(
                    temporary.clone(),
                    origins::InitializerOrigin {
                        start: first.start,
                        end: last.end,
                        statement: false,
                    },
                );
            }
            self.completion_site = previous.map(|(start, _, end)| (start, start, end));
            let value = format!(
                "{}{temporary}{}",
                self.completion_origin(),
                relocations::unmapped(),
            );
            let finish = self.block(self.completion_code("return", &value)?);
            self.completion_site = previous;
            let resume = self.block(format!(
                "{temporary} = {context}.sent(); {}",
                self.jump(finish)
            ));
            let suspension = self.suspension(true, &temporary)?;
            let suspend = self.block(format!(
                "{context}.label = {resume}; return {{kind:\"yield\", value:{suspension}}};"
            ));
            return self.expression(tokens, Some(&temporary), "", suspend);
        }
        if !self.suspends(tokens) {
            let value = self.text(tokens).unwrap_or_else(|| "void 0".into());
            let code = if completion.is_empty() {
                let assignment = binding.map_or(String::new(), |name| self.binding_prefix(name));
                format!(
                    "{assignment}{value}{}{}",
                    if binding.is_none() && !tokens.is_empty() {
                        self.resume_terminator(tokens, true)
                    } else {
                        self.initializer_terminator(binding, tokens)
                    },
                    self.jump(next)
                )
            } else {
                self.completion_code(completion, &value)?
            };
            return Some(self.block(code));
        }
        let first = tokens.first()?;
        if first.is("await")
            && exponentiation::operand_end(tokens, 1).is_some_and(|end| end < tokens.len())
        {
            return self.compound_expression(tokens, binding, completion, next);
        }
        if !matches!(first.text.as_str(), "yield" | "await") {
            return self.compound_expression(tokens, binding, completion, next);
        }
        let delegate = tokens.get(1).is_some_and(|token| token.is("*"));
        let operand = &tokens[1 + usize::from(delegate)..];
        let resume = if completion.is_empty() {
            let assignment = binding.map_or(String::new(), |name| self.binding_prefix(name));
            format!(
                "{assignment}{}{context}.sent(){}{}",
                relocations::source_anchor(self.source_offset + first.start),
                self.resume_terminator(tokens, binding.is_none()),
                self.jump(next)
            )
        } else {
            self.completion_code(
                completion,
                &format!(
                    "{}{context}.sent(){}",
                    relocations::source_anchor(self.source_offset + first.start),
                    relocations::unmapped()
                ),
            )?
        };
        let resume = self.block(format!("{resume}{}", relocations::unmapped()));
        if delegate && self.marker.is_some() {
            return None;
        }
        self.delegation |= delegate;
        let temporary = self.temporary();
        if let (Some(first), Some(last)) = (operand.first(), operand.last()) {
            self.binding_initializers.insert(
                temporary.clone(),
                origins::InitializerOrigin {
                    start: first.start,
                    end: last.end,
                    statement: false,
                },
            );
        }
        let value = if self.marker.is_some() {
            self.suspension(first.is("await"), &temporary)?
        } else {
            temporary.clone()
        };
        let public_generator_yield = self.marker.is_some() && first.is("yield");
        let operation_end = if public_generator_yield {
            first.start
        } else {
            tokens.last()?.end
        };
        let transition = if self.marker.is_some() {
            format!(
                "{} ",
                relocations::source_anchor(self.source_offset + tokens.last()?.end)
            )
        } else {
            String::new()
        };
        let suspend = self.block(format!(
            "{context}.label = {resume}; {}return {{kind:{}, value:{}({value}{})}};{}{}",
            relocations::source_anchor(self.source_offset + first.start),
            if delegate {
                "\"delegate\""
            } else {
                "\"yield\""
            },
            relocations::unmapped(),
            relocations::source_anchor(self.source_offset + operation_end),
            transition,
            relocations::unmapped()
        ));
        self.expression(operand, Some(&temporary), "", suspend)
    }

    fn suspension(&self, awaiting: bool, value: &str) -> Option<String> {
        Some(if self.external_abi {
            format!("{{awaiting:{awaiting},value:{value}}}")
        } else {
            format!("{}({awaiting}, {value})", self.marker?)
        })
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
        let mut output = String::new();
        let mut cursor = first.start;
        for edit in edits {
            let start = first.start + edit.start;
            let end = first.start + edit.end;
            if cursor < start {
                output.push_str(&relocations::relocated_verbatim(
                    self.source_offset + cursor,
                    self.source_offset + start,
                ));
            }
            output.push_str(&relocations::source_anchor(self.source_offset + start));
            output.push_str(&edit.replacement);
            output.push_str(&relocations::source_anchor(self.source_offset + end));
            cursor = end;
        }
        if cursor < last.end {
            output.push_str(&relocations::relocated_verbatim(
                self.source_offset + cursor,
                self.source_offset + last.end,
            ));
        }
        output.push_str(relocations::unmapped());
        Some(output)
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
