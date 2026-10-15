// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Temporary parser provenance, discarded after binding statement ordinals.

use super::*;
use crate::program_debug::ast_ids::{same_statement_source, visit_statement_tree};

type Span = Option<(usize, usize)>;

pub(super) struct CapturedStatement {
    statement: Stmt,
    spans: Vec<Span>,
}

fn statement_count(statement: &Stmt) -> usize {
    let mut count = 0;
    visit_statement_tree(statement, &mut |_| count += 1);
    count
}

struct Subtrees {
    roots: Vec<CapturedStatement>,
    used: Vec<Vec<bool>>,
    cursor: usize,
}

impl Subtrees {
    fn new(roots: Vec<CapturedStatement>) -> Self {
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

    fn take(&mut self, statement: &Stmt) -> Option<Vec<Span>> {
        while self.cursor < self.roots.len() && self.used[self.cursor].iter().all(|used| *used) {
            self.cursor += 1;
        }
        for index in self.cursor..self.roots.len() {
            let root = &self.roots[index];
            if !self.used[index].iter().any(|used| *used)
                && same_statement_source(&root.statement, statement)
            {
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
            visit_statement_tree(&root.statement, &mut |candidate| {
                if found.is_none()
                    && !self.used[index][ordinal]
                    && same_statement_source(candidate, statement)
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

impl CapturedStatement {
    pub(super) fn new(statement: &Stmt, span: (usize, usize), children: Vec<Self>) -> Self {
        let mut children = Subtrees::new(children);
        let mut spans = Vec::new();
        let mut skip = 0;
        visit_statement_tree(statement, &mut |node| {
            if spans.is_empty() {
                spans.push(Some(span));
            } else if skip > 0 {
                skip -= 1;
            } else if let Some(found) = children.take(node) {
                skip = found.len() - 1;
                spans.extend(found);
            } else {
                spans.push(None);
            }
        });
        Self {
            statement: statement.clone(),
            spans,
        }
    }

    pub(super) fn normalize(self, statement: &Stmt) -> Self {
        if same_statement_source(&self.statement, statement) {
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
