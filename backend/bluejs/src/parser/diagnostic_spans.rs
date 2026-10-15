// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Temporary source provenance. Snapshots are matched structurally with exact
//! function source ranges and discarded after binding diagnostic node slots.
//! Cumulative cloned AST/span storage is bounded before each snapshot clone.

use super::*;
use crate::program_debug::ast_ids::{
    self, same_statement_source, visit_statement_tree, DiagnosticNode,
};

type Span = Option<(usize, usize)>;

pub(super) type CapturedStatement = CapturedNode<Stmt>;
pub(super) type CapturedExpression = CapturedNode<Expr>;
pub(super) type CapturedPattern = CapturedNode<Pattern>;
pub(super) type CapturedAssignmentPattern = CapturedNode<AssignmentPattern>;

pub(super) trait CapturedSyntax: Clone {
    fn visit(&self, visit: &mut dyn FnMut(&Self));
    fn same_source(&self, other: &Self) -> bool;
    fn clone_bytes(&self) -> Option<usize>;
}
impl CapturedSyntax for Stmt {
    fn visit(&self, visit: &mut dyn FnMut(&Self)) {
        visit_statement_tree(self, visit);
    }
    fn same_source(&self, other: &Self) -> bool {
        same_statement_source(self, other)
    }
    fn clone_bytes(&self) -> Option<usize> {
        self.diagnostic_clone_payload_bytes()
    }
}
impl CapturedSyntax for Expr {
    fn visit(&self, visit: &mut dyn FnMut(&Self)) {
        ast_ids::visit_expression_nodes(self, &mut |node, _| {
            if let DiagnosticNode::Expression(node) = node {
                visit(node)
            }
        });
    }
    fn same_source(&self, other: &Self) -> bool {
        ast_ids::same_expression_source(self, other)
    }
    fn clone_bytes(&self) -> Option<usize> {
        self.diagnostic_clone_payload_bytes()
    }
}
impl CapturedSyntax for Pattern {
    fn visit(&self, visit: &mut dyn FnMut(&Self)) {
        ast_ids::visit_pattern_nodes(self, &mut |node, _| {
            if let DiagnosticNode::Pattern(node) = node {
                visit(node)
            }
        });
    }
    fn same_source(&self, other: &Self) -> bool {
        ast_ids::same_pattern_source(self, other)
    }
    fn clone_bytes(&self) -> Option<usize> {
        self.diagnostic_clone_payload_bytes()
    }
}

impl CapturedSyntax for AssignmentPattern {
    fn visit(&self, visit: &mut dyn FnMut(&Self)) {
        ast_ids::visit_assignment_pattern_nodes(self, &mut |node, _| {
            if let DiagnosticNode::AssignmentPattern(node) = node {
                visit(node)
            }
        });
    }
    fn same_source(&self, other: &Self) -> bool {
        ast_ids::same_assignment_pattern_source(self, other)
    }
    fn clone_bytes(&self) -> Option<usize> {
        self.diagnostic_clone_payload_bytes()
    }
}

pub(super) struct CapturedNode<N> {
    statement: N,
    spans: Vec<Span>,
}

fn statement_count<N: CapturedSyntax>(statement: &N) -> usize {
    let mut count = 0;
    statement.visit(&mut |_| count += 1);
    count
}

struct Subtrees<N> {
    roots: Vec<CapturedNode<N>>,
    used: Vec<Vec<bool>>,
    cursor: usize,
}

impl<N: CapturedSyntax> Subtrees<N> {
    fn new(roots: Vec<CapturedNode<N>>) -> Self {
        let used = roots
            .iter()
            .map(|root| vec![false; root.spans.len()])
            .collect();
        Self {
            roots,
            used,
            cursor: 0,
        }
    }

    fn take(&mut self, statement: &N) -> Option<Vec<Span>> {
        while self.cursor < self.roots.len() && self.used[self.cursor].iter().all(|used| *used) {
            self.cursor += 1;
        }
        for index in self.cursor..self.roots.len() {
            let root = &self.roots[index];
            if !self.used[index].iter().any(|used| *used) && root.statement.same_source(statement) {
                self.used[index].fill(true);
                return Some(root.spans.clone());
            }
        }
        // Directive/label normalization can remove a parsed wrapper. Only
        // an exact surviving subtree (including function source ranges) binds.
        for index in self.cursor..self.roots.len() {
            let root = &self.roots[index];
            let mut ordinal = 0;
            let mut found = None;
            root.statement.visit(&mut |candidate| {
                if found.is_none() && !self.used[index][ordinal] && candidate.same_source(statement)
                {
                    let end = ordinal + statement_count(candidate);
                    if self.used[index][ordinal..end].iter().all(|used| !*used) {
                        found = Some((ordinal, end));
                    }
                }
                ordinal += 1;
            });
            if let Some((start, end)) = found {
                self.used[index][start..end].fill(true);
                return Some(root.spans[start..end].to_vec());
            }
        }
        None
    }
}

impl<N: CapturedSyntax> CapturedNode<N> {
    pub(super) fn new(
        statement: &N,
        span: (usize, usize),
        mut children: Vec<Self>,
        budget: &mut usize,
    ) -> Result<Self, ParseError> {
        if children.len() == 1 && children[0].statement.same_source(statement) {
            return Ok(children.pop().unwrap());
        }
        let count = statement_count(statement);
        let bytes = statement.clone_bytes().and_then(|bytes| {
            bytes
                .checked_add(std::mem::size_of::<Vec<Span>>())?
                .checked_add(
                    count
                        .checked_next_power_of_two()?
                        .max(4)
                        .checked_mul(std::mem::size_of::<Span>())?,
                )
        });
        *budget = bytes
            .and_then(|bytes| budget.checked_add(bytes))
            .filter(|bytes| *bytes <= crate::diagnostics::MAX_DIAGNOSTIC_SOURCE_MAP_BYTES)
            .ok_or_else(|| ParseError {
                message: "diagnostic source capture limit exceeded".into(),
                resource: None,
                known_syntax: false,
            })?;
        let mut children = Subtrees::new(children);
        let mut spans = Vec::new();
        let mut skip = 0;
        statement.visit(&mut |node| {
            if skip > 0 {
                skip -= 1;
            } else if let Some(found) = children.take(node) {
                skip = found.len() - 1;
                spans.extend(found);
            } else {
                spans.push(if spans.is_empty() { Some(span) } else { None });
            }
        });
        Ok(Self {
            statement: statement.clone(),
            spans,
        })
    }

    pub(super) fn shift(&mut self, offset: usize) {
        for (start, end) in self.spans.iter_mut().flatten() {
            *start += offset;
            *end += offset;
        }
    }

    pub(super) fn normalize(self, statement: &N) -> Self {
        if self.statement.same_source(statement) {
            return self;
        }
        let root_span = self.spans[0];
        let mut subtrees = Subtrees::new(vec![self]);
        let mut spans = subtrees
            .take(statement)
            .unwrap_or_else(|| vec![None; statement_count(statement)]);
        if spans[0].is_none() {
            spans[0] = root_span;
        }
        Self {
            statement: statement.clone(),
            spans,
        }
    }
}

pub(super) fn finish(body: &[Stmt], records: Vec<CapturedStatement>) -> Vec<Span> {
    let mut records = Subtrees::new(records);
    let mut spans = Vec::new();
    for statement in body {
        spans.extend(
            records
                .take(statement)
                .unwrap_or_else(|| vec![None; statement_count(statement)]),
        );
    }
    spans
}

/// Scatter expression/pattern forests into the unchanged executable-AST inventory.
pub(super) fn finish_nodes(
    body: &[Stmt],
    expressions: Vec<CapturedExpression>,
    patterns: Vec<CapturedPattern>,
    assignments: Vec<CapturedAssignmentPattern>,
    spans: &mut Vec<Span>,
) {
    let mut expressions = Subtrees::new(expressions);
    let mut patterns = Subtrees::new(patterns);
    let mut assignments = Subtrees::new(assignments);
    let mut expression_spans = Vec::new().into_iter();
    let mut pattern_spans = Vec::new().into_iter();
    let mut assignment_spans = Vec::new().into_iter();
    ast_ids::visit_program_nodes(body, &mut |node, ordinal| {
        spans.resize(spans.len().max(ordinal + 1), None);
        match node {
            DiagnosticNode::Expression(node) => {
                let span = expression_spans.next().unwrap_or_else(|| {
                    expression_spans = expressions
                        .take(node)
                        .unwrap_or_else(|| vec![None; statement_count(node)])
                        .into_iter();
                    expression_spans.next().unwrap_or(None)
                });
                spans[ordinal] = span;
            }
            DiagnosticNode::Pattern(node) => {
                let span = pattern_spans.next().unwrap_or_else(|| {
                    pattern_spans = patterns
                        .take(node)
                        .unwrap_or_else(|| vec![None; statement_count(node)])
                        .into_iter();
                    pattern_spans.next().unwrap_or(None)
                });
                spans[ordinal] = span;
            }
            DiagnosticNode::AssignmentPattern(node) => {
                let span = assignment_spans.next().unwrap_or_else(|| {
                    assignment_spans = assignments
                        .take(node)
                        .unwrap_or_else(|| vec![None; statement_count(node)])
                        .into_iter();
                    assignment_spans.next().unwrap_or(None)
                });
                spans[ordinal] = span;
            }
            _ => {}
        }
    });
}

impl Parser {
    pub(super) fn capture_statement_span(
        &mut self,
        statement: &Stmt,
        base: usize,
        start: usize,
    ) -> Result<(), ParseError> {
        if let Some(captures) = self.diagnostic_statements.as_mut() {
            let children = captures.split_off(base);
            let record = CapturedStatement::new(
                statement,
                (start, self.positions[self.pos]),
                children,
                &mut self.diagnostic_capture_bytes,
            )?;
            captures.push(record);
        }
        Ok(())
    }

    pub(super) fn finish_diagnostic_spans(
        &mut self,
        body: &[Stmt],
        capture_nodes: bool,
    ) -> Vec<Span> {
        let mut diagnostic_spans = Vec::new();
        if let Some(records) = self.diagnostic_statements.take() {
            let mut statements = finish(body, records).into_iter();
            let count = crate::program_debug::ast_ids::visit_program_statement_nodes(
                body,
                &mut |_, ordinal| {
                    diagnostic_spans.resize(ordinal + 1, None);
                    diagnostic_spans[ordinal] = statements.next().unwrap_or(None);
                },
            );
            diagnostic_spans.resize(count, None);
            if capture_nodes {
                finish_nodes(
                    body,
                    self.diagnostic_expressions.take().unwrap(),
                    self.diagnostic_patterns.take().unwrap(),
                    self.diagnostic_assignments.take().unwrap(),
                    &mut diagnostic_spans,
                );
            }
        }
        diagnostic_spans
    }

    pub(super) fn diagnostic_expression<F>(&mut self, parse: F) -> Result<Expr, ParseError>
    where
        F: FnOnce(&mut Self) -> Result<Expr, ParseError>,
    {
        let Some(base) = self.diagnostic_expressions.as_ref().map(Vec::len) else {
            return parse(self);
        };
        let start = self.token_start();
        let expression = parse(self)?;
        self.capture_expression(&expression, base, start)?;
        Ok(expression)
    }

    pub(super) fn capture_expression(
        &mut self,
        expression: &Expr,
        base: usize,
        start: usize,
    ) -> Result<(), ParseError> {
        if let Some(captures) = self.diagnostic_expressions.as_mut() {
            let children = captures.split_off(base);
            let record = CapturedExpression::new(
                expression,
                (start, self.positions[self.pos]),
                children,
                &mut self.diagnostic_capture_bytes,
            )?;
            captures.push(record);
        }
        Ok(())
    }
}
