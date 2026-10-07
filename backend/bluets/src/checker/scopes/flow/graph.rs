// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Flow edges for retained structured bodies and balanced opaque statements.

use super::*;

type Guard = (Vec<Token>, bool);
type Edge = (usize, Option<Guard>);

pub(super) struct Node {
    pub(super) tokens: Vec<Token>,
    pub(super) edges: Vec<Edge>,
    pub(super) preferred: Option<Vec<Token>>,
    pub(super) expression_statement: bool,
}

pub(super) struct Graph {
    pub(super) nodes: Vec<Node>,
    pub(super) creation: usize,
    pub(super) execution: ScopeId,
    pub(super) hoisted: bool,
    pub(super) exits: Vec<usize>,
    pub(super) normal_exits: Vec<usize>,
    pub(super) completion_known: bool,
    pub(super) returns: Vec<usize>,
    pub(super) comparisons: Vec<(Vec<Token>, Vec<Token>)>,
}

pub(super) fn graphs(scopes: &ScopeModel<'_>) -> Vec<Graph> {
    (0..scopes.scopes.len())
        .filter_map(|id| execution(scopes, id))
        .collect()
}

pub(super) fn execution(scopes: &ScopeModel<'_>, id: ScopeId) -> Option<Graph> {
    let scope = &scopes.scopes[id];
    if scope.execution != id {
        return None;
    }
    let body = if id == 0 {
        Some(scope.span.clone())
    } else {
        scopes
            .scopes
            .iter()
            .find(|child| child.parent == Some(id) && child.var_boundary)
            .map(|child| child.span.clone())
            .or_else(|| {
                scopes
                    .module
                    .nested_functions
                    .get(&scope.span.start)
                    .and_then(|function| match &function.body {
                        NestedFunctionBody::Expression(tokens) => Some(SourceSpan::new(
                            &scopes.module.id,
                            tokens.first()?.start,
                            tokens.last()?.end,
                        )),
                        _ => None,
                    })
            })
    }?;
    let tokens = scopes.tokens_in(&body);
    let mut builder = Builder {
        scopes,
        tokens: &tokens,
        index: 0,
        nodes: vec![Node {
            tokens: Vec::new(),
            edges: Vec::new(),
            preferred: None,
            expression_statement: false,
        }],
        breaks: Vec::new(),
        continues: Vec::new(),
        returns: Vec::new(),
        comparisons: Vec::new(),
        completion_known: true,
        execution: id,
        depth: 0,
    };
    let mut exits = builder.sequence(vec![0]);
    let expression_body = scopes
        .module
        .nested_functions
        .get(&scope.span.start)
        .is_some_and(|function| matches!(function.body, NestedFunctionBody::Expression(_)));
    let normal_exits = if expression_body {
        builder.returns.extend_from_slice(&exits);
        for node in &mut builder.nodes {
            node.expression_statement = false;
        }
        Vec::new()
    } else {
        exits.clone()
    };
    exits.extend_from_slice(&builder.returns);
    Some(Graph {
        nodes: builder.nodes,
        creation: scope.span.start,
        execution: id,
        hoisted: id != 0
            && !scopes
                .module
                .nested_functions
                .contains_key(&scope.span.start),
        exits,
        normal_exits,
        completion_known: builder.completion_known,
        returns: builder.returns,
        comparisons: builder.comparisons,
    })
}

struct Builder<'a, 'b> {
    scopes: &'a ScopeModel<'b>,
    tokens: &'a [Token],
    index: usize,
    nodes: Vec<Node>,
    breaks: Vec<usize>,
    continues: Vec<usize>,
    returns: Vec<usize>,
    comparisons: Vec<(Vec<Token>, Vec<Token>)>,
    completion_known: bool,
    execution: ScopeId,
    depth: usize,
}

impl Builder<'_, '_> {
    fn node(&mut self, tokens: &[Token], incoming: &[usize]) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node {
            tokens: tokens.to_vec(),
            edges: Vec::new(),
            preferred: None,
            expression_statement: false,
        });
        for previous in incoming {
            self.edge(*previous, id, None);
        }
        id
    }

    fn edge(&mut self, from: usize, to: usize, guard: Option<Guard>) {
        self.nodes[from].edges.push((to, guard));
    }

    fn sequence(&mut self, mut incoming: Vec<usize>) -> Vec<usize> {
        while self.index < self.tokens.len() && !self.tokens[self.index].is("}") {
            incoming = self.statement(incoming);
        }
        incoming
    }

    fn statement(&mut self, incoming: Vec<usize>) -> Vec<usize> {
        if self.index >= self.tokens.len() {
            return incoming;
        }
        self.depth += 1;
        let result = self.statement_inner(incoming);
        self.depth -= 1;
        result
    }

    fn statement_inner(&mut self, incoming: Vec<usize>) -> Vec<usize> {
        let token = &self.tokens[self.index];
        let scope = self.scopes.scope_at(token.start);
        if self.scopes.scopes[scope].execution != self.execution {
            let execution = self.scopes.scopes[scope].execution;
            let end = self.scopes.scopes[execution].span.end;
            self.index = self
                .tokens
                .partition_point(|token| token.start < end)
                .max(self.index + 1);
            return incoming;
        }
        // Exhaustion is a precise refusal, including opaque adapter nesting.
        if self.depth > self.scopes.max_type_expansions.max(32) {
            self.scopes.flow_limit(
                std::slice::from_ref(token),
                "control-flow graph exceeds its bounded nesting limit",
            );
            self.index = self.tokens.len();
            return incoming;
        }
        match token.text.as_str() {
            ";" => {
                self.index += 1;
                incoming
            }
            "{" => {
                self.index += 1;
                let outgoing = self.sequence(incoming);
                if self.index < self.tokens.len() {
                    self.index += 1;
                }
                outgoing
            }
            "if" => self.branch(incoming),
            "while" | "for" | "do" => self.loop_statement(incoming),
            "switch" => self.switch_statement(incoming),
            "try" => self.try_statement(incoming),
            "break" | "continue" => {
                let target = if token.is("break") {
                    self.breaks.last()
                } else {
                    self.continues.last()
                }
                .copied();
                let node = self.node(&[], &incoming);
                if let Some(target) = target {
                    self.edge(node, target, None);
                }
                self.index = self.end_expression(self.index);
                Vec::new()
            }
            "return" | "throw" => {
                let end = self.end_expression(self.index);
                let node = self.node(&self.tokens[self.index + 1..end], &incoming);
                if token.is("return") {
                    self.returns.push(node);
                }
                self.index = end;
                Vec::new()
            }
            "class" | "interface" | "namespace" | "enum" | "type" | "function" | "declare"
            | "import" | "export" => {
                // Erased declarations and delayed bodies do not execute in this
                // graph. Exported variables still carry initializer writes.
                if token.is("export")
                    && self
                        .tokens
                        .get(self.index + 1)
                        .is_some_and(|token| matches!(token.text.as_str(), "const" | "let" | "var"))
                {
                    self.index += 1;
                    return self.statement(incoming);
                }
                self.skip_declaration();
                incoming
            }
            _ => {
                let end = self.end_expression(self.index);
                let node = self.node(&self.tokens[self.index..end], &incoming);
                self.nodes[node].expression_statement =
                    !matches!(token.text.as_str(), "const" | "let" | "var");
                self.index = end.max(self.index + 1);
                vec![node]
            }
        }
    }

    fn condition(&mut self) -> Vec<Token> {
        self.index += 1;
        let open = self.index;
        let end = super::super::targets::close(self.tokens, open).unwrap_or(open);
        self.index = end + 1;
        self.tokens.get(open + 1..end).unwrap_or(&[]).to_vec()
    }

    fn branch(&mut self, incoming: Vec<usize>) -> Vec<usize> {
        let condition = self.condition();
        let test = self.node(&condition, &incoming);
        let positive = self.node(&[], &[]);
        let negative = self.node(&[], &[]);
        self.edge(test, positive, Some((condition.clone(), true)));
        self.edge(test, negative, Some((condition, false)));
        let mut outgoing = self.statement(vec![positive]);
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.is("else"))
        {
            self.index += 1;
            outgoing.extend(self.statement(vec![negative]));
        } else {
            outgoing.push(negative);
        }
        outgoing
    }

    fn loop_statement(&mut self, incoming: Vec<usize>) -> Vec<usize> {
        let kind = self.tokens[self.index].text.clone();
        let mut head = if kind == "do" {
            self.index += 1;
            Vec::new()
        } else {
            self.condition()
        };
        let mut incoming = incoming;
        let mut increment = Vec::new();
        if kind == "for" {
            if let Some(first) = super::super::targets::top_level(&head, ";") {
                let rest = &head[first + 1..];
                if let Some(second) = super::super::targets::top_level(rest, ";") {
                    let init = self.node(&head[..first], &incoming);
                    incoming = vec![init];
                    increment = rest[second + 1..].to_vec();
                    head = rest[..second].to_vec();
                }
            }
        }
        let header = self.node(&[], &incoming);
        let test = self.node(&head, &[]);
        let body = self.node(&[], &[]);
        let after = self.node(&[], &[]);
        let update = self.node(&increment, &[]);
        self.edge(header, if kind == "do" { body } else { test }, None);
        self.edge(test, body, Some((head.clone(), true)));
        self.edge(test, after, Some((head, false)));
        self.edge(update, test, None);
        self.breaks.push(after);
        self.continues.push(update);
        let outgoing = self.statement(vec![body]);
        for previous in outgoing {
            self.edge(previous, update, None);
        }
        self.breaks.pop();
        self.continues.pop();
        if kind == "do"
            && self
                .tokens
                .get(self.index)
                .is_some_and(|token| token.is("while"))
        {
            let condition = self.condition();
            self.nodes[test].tokens = condition.clone();
            self.nodes[test].edges = vec![
                (body, Some((condition.clone(), true))),
                (after, Some((condition, false))),
            ];
        }
        vec![after]
    }

    fn switch_statement(&mut self, incoming: Vec<usize>) -> Vec<usize> {
        let expression = self.condition();
        let mut residual = self.node(&expression, &incoming);
        let after = self.node(&[], &[]);
        self.breaks.push(after);
        self.index += usize::from(
            self.tokens
                .get(self.index)
                .is_some_and(|token| token.is("{")),
        );
        let mut fallthrough = Vec::new();
        let mut default = None;
        while self.index < self.tokens.len() && !self.tokens[self.index].is("}") {
            if self.tokens[self.index].is("case") || self.tokens[self.index].is("default") {
                let is_default = self.tokens[self.index].is("default");
                self.index += 1;
                let start = self.index;
                while self.index < self.tokens.len() && !self.tokens[self.index].is(":") {
                    self.index += 1;
                }
                let mut condition = expression.clone();
                condition.push(Token {
                    kind: TokenKind::Punct,
                    text: "===".into(),
                    start: 0,
                    end: 0,
                });
                if !is_default {
                    self.comparisons
                        .push((expression.clone(), self.tokens[start..self.index].to_vec()));
                }
                condition.extend_from_slice(&self.tokens[start..self.index]);
                self.index += usize::from(self.index < self.tokens.len());
                let case = self.node(&[], &fallthrough);
                if is_default {
                    default = Some(case);
                } else {
                    self.nodes[case].preferred = Some(condition.clone());
                    let next = self.node(&[], &[]);
                    self.edge(residual, case, Some((condition.clone(), true)));
                    self.edge(residual, next, Some((condition, false)));
                    residual = next;
                }
                fallthrough = vec![case];
            } else {
                fallthrough = self.statement(fallthrough);
            }
        }
        for previous in fallthrough {
            self.edge(previous, after, None);
        }
        self.edge(residual, default.unwrap_or(after), None);
        self.breaks.pop();
        if self.index < self.tokens.len() {
            self.index += 1;
        }
        vec![after]
    }

    fn end_expression(&self, start: usize) -> usize {
        let mut index = start;
        while index < self.tokens.len() {
            if self.tokens[index].is(";") {
                return index + 1;
            }
            if self.tokens[index].is("}") {
                return index;
            }
            if matches!(self.tokens[index].text.as_str(), "(" | "[" | "{") {
                if let Some(end) = super::super::targets::close(self.tokens, index) {
                    index = end;
                }
            }
            index += 1;
        }
        index
    }

    fn try_statement(&mut self, incoming: Vec<usize>) -> Vec<usize> {
        self.completion_known = false;
        let start = self.index;
        self.index += 1;
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.is("{"))
        {
            self.index = super::super::targets::close(self.tokens, self.index)
                .map_or(self.tokens.len(), |end| end + 1);
        }
        for keyword in ["catch", "finally"] {
            if !self
                .tokens
                .get(self.index)
                .is_some_and(|token| token.is(keyword))
            {
                continue;
            }
            self.index += 1;
            if keyword == "catch"
                && self
                    .tokens
                    .get(self.index)
                    .is_some_and(|token| token.is("("))
            {
                self.index = super::super::targets::close(self.tokens, self.index)
                    .map_or(self.tokens.len(), |end| end + 1);
            }
            if self
                .tokens
                .get(self.index)
                .is_some_and(|token| token.is("{"))
            {
                self.index = super::super::targets::close(self.tokens, self.index)
                    .map_or(self.tokens.len(), |end| end + 1);
            }
        }
        // Structured completion owns exceptional exits. The adapter retains
        // the lexical boundary and resumes at the following normal statement.
        let node = self.node(&self.tokens[start..self.index], &incoming);
        vec![node]
    }

    fn skip_declaration(&mut self) {
        while self.index < self.tokens.len() {
            let token = &self.tokens[self.index];
            if token.is("{") {
                self.index = super::super::targets::close(self.tokens, self.index)
                    .map_or(self.index + 1, |end| end + 1);
                return;
            }
            self.index += 1;
            if token.is(";") {
                return;
            }
        }
    }
}
