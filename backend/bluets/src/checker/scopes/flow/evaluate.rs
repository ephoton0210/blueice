// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reachable joins, loop fixed points and expression-local flow snapshots.

use super::*;
use std::collections::VecDeque;

pub(super) fn build(
    scopes: &ScopeModel<'_>,
    infer: &impl Fn(&[Token], &BTreeMap<String, Type>, &Type) -> Type,
) -> FlowModel {
    let mut model = FlowModel::default();
    let mut remaining = scopes.max_type_expansions.saturating_mul(1024).max(1024);
    for graph in graph::graphs(scopes) {
        if graph.execution != 0 && scopes.flow_immediate(graph.creation) {
            continue;
        }
        let mut seed = State::new();
        if graph.execution != 0 {
            if let Some(captured) = model.facts.get(&graph.creation) {
                for (id, value) in captured.iter() {
                    if scopes.flow_capture_allowed(id, graph.creation, graph.hoisted) {
                        seed.insert(id.clone(), value.clone());
                    }
                }
            }
        }
        run_graph(scopes, &mut model, &graph, seed, infer, &mut remaining, 0);
    }
    if let Some(failure) = scopes.flow_failure.borrow_mut().take() {
        model.diagnostics.push(failure);
    }
    model
}

fn run_graph(
    scopes: &ScopeModel<'_>,
    model: &mut FlowModel,
    graph: &graph::Graph,
    seed: State,
    infer: &impl Fn(&[Token], &BTreeMap<String, Type>, &Type) -> Type,
    remaining: &mut usize,
    depth: usize,
) -> State {
    let mut incoming = vec![None; graph.nodes.len()];
    incoming[0] = Some(seed.clone());
    let mut outgoing: Vec<Option<State>> = vec![None; graph.nodes.len()];
    let mut queue = VecDeque::from([0]);
    while let Some(index) = queue.pop_front() {
        let node = &graph.nodes[index];
        let state = incoming[index].as_ref().expect("reachable node");
        let cost = node
            .tokens
            .len()
            .saturating_add(node.edges.len())
            .saturating_add(1)
            .saturating_mul(
                state
                    .len()
                    .saturating_add(state.properties.len())
                    .saturating_add(1),
            );
        if *remaining < cost {
            model.diagnostics.push(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                scopes.scopes[graph.execution].span.clone(),
                "control-flow analysis exceeds its bounded work limit",
            ));
            return seed;
        }
        *remaining -= cost;
        let mut preferred_state = state.clone();
        if let Some(condition) = &node.preferred {
            if let Some(preferred) = predicates::narrow(scopes, state, condition, true) {
                for (id, first) in preferred.bindings {
                    if let Some(Type::Union(parts)) = state.get(&id) {
                        let first_parts = match first {
                            Type::Union(parts) => parts,
                            first => vec![first],
                        };
                        preferred_state.insert(
                            id,
                            union(first_parts.into_iter().chain(parts.iter().cloned())),
                        );
                    }
                }
            }
        }
        let state = expression(
            scopes,
            model,
            &node.tokens,
            &preferred_state,
            node.expression_statement,
            infer,
            graph.execution,
            depth + 1,
            remaining,
        );
        outgoing[index] = Some(state.clone());
        for (next, guard) in &node.edges {
            let next_state = match guard {
                Some((tokens, positive)) => predicates::narrow(scopes, &state, tokens, *positive),
                None => Some(state.clone()),
            };
            let Some(next_state) = next_state else {
                continue;
            };
            let merged = incoming[*next].as_ref().map_or_else(
                || next_state.clone(),
                |before| join(scopes, before, &next_state),
            );
            if incoming[*next].as_ref() != Some(&merged) {
                incoming[*next] = Some(merged);
                if !queue.contains(next) {
                    queue.push_back(*next);
                }
            }
        }
    }

    if graph.completion_known {
        model.completions.insert(
            graph.creation,
            graph.normal_exits.iter().any(|exit| {
                outgoing[*exit]
                    .as_ref()
                    .is_some_and(|state| state.reachable)
            }),
        );
    }
    model.returns.insert(
        graph.creation,
        graph
            .returns
            .iter()
            .map(|index| {
                let tokens = &graph.nodes[*index].tokens;
                if tokens.last().is_some_and(|token| token.is(";")) {
                    tokens[..tokens.len() - 1].to_vec()
                } else {
                    tokens.clone()
                }
            })
            .collect(),
    );
    calls::check_cases(scopes, model, graph, infer);
    graph
        .exits
        .iter()
        .filter_map(|exit| {
            incoming[*exit].as_ref().map(|state| {
                expression(
                    scopes,
                    model,
                    &graph.nodes[*exit].tokens,
                    state,
                    graph.nodes[*exit].expression_statement,
                    infer,
                    graph.execution,
                    depth + 1,
                    remaining,
                )
            })
        })
        .reduce(|left, right| join(scopes, &left, &right))
        .unwrap_or_else(|| {
            let mut dead = seed;
            dead.reachable = false;
            dead
        })
}

fn record(_scopes: &ScopeModel<'_>, model: &mut FlowModel, token: &Token, state: &State) {
    // Node inputs already join every reachable predecessor. Replace the older
    // worklist snapshot to preserve the final case-predecessor order.
    model.facts.insert(token.start, state.clone());
}

#[allow(clippy::too_many_arguments)]
fn expression(
    scopes: &ScopeModel<'_>,
    model: &mut FlowModel,
    tokens: &[Token],
    state: &State,
    effects_allowed: bool,
    infer: &impl Fn(&[Token], &BTreeMap<String, Type>, &Type) -> Type,
    execution: ScopeId,
    depth: usize,
    remaining: &mut usize,
) -> State {
    let tokens = if tokens.last().is_some_and(|token| token.is(";")) {
        &tokens[..tokens.len() - 1]
    } else {
        tokens
    };
    let bare_expression = super::super::targets::strip(tokens).len() == tokens.len();
    let tokens = super::super::targets::strip(tokens);
    if depth > scopes.max_type_expansions.max(32) {
        scopes.flow_limit(tokens, "flow expression exceeds its bounded nesting limit");
        return state.clone();
    }
    if tokens.is_empty() {
        return state.clone();
    }
    // Every nested execution receives its creation fact, but its body is
    // evaluated only in its own graph (or at a proven immediate invocation).
    for token in tokens {
        if scopes.scopes[scopes.scope_at(token.start)].execution == execution
            || scopes.module.nested_functions.contains_key(&token.start)
        {
            record(scopes, model, token, state);
        }
    }
    // Comma operands have their own call effects even within an initializer
    // or argument; parentheses around a call preserve its separate eligibility.
    if let Some(index) = super::super::targets::top_level(tokens, ",") {
        let left = expression(
            scopes,
            model,
            &tokens[..index],
            state,
            true,
            infer,
            execution,
            depth + 1,
            remaining,
        );
        return expression(
            scopes,
            model,
            &tokens[index + 1..],
            &left,
            true,
            infer,
            execution,
            depth + 1,
            remaining,
        );
    }
    if let Some(equal) = super::super::targets::top_level(tokens, "=") {
        let target = if tokens
            .first()
            .is_some_and(|token| matches!(token.text.as_str(), "const" | "let" | "var"))
        {
            tokens.get(1).map(std::slice::from_ref).unwrap_or(&[])
        } else {
            &tokens[..equal]
        };
        let after = expression(
            scopes,
            model,
            &tokens[equal + 1..],
            state,
            false,
            infer,
            execution,
            depth + 1,
            remaining,
        );
        if let [name] = target {
            if let Some(id) = scopes.flow_binding(&name.text, name.start) {
                let declared = scopes.flow_declared(&id);
                let mut actual = predicates::reference(scopes, &tokens[equal + 1..], 0)
                    .and_then(|(id, path)| properties::read(scopes, &after, &id, &path))
                    .unwrap_or_else(|| {
                        infer(
                            &tokens[equal + 1..],
                            &values(scopes, &after, name.start),
                            &declared,
                        )
                    });
                if actual == Type::Unknown {
                    if let [keyword, constructor, open, close] = &tokens[equal + 1..] {
                        if keyword.is("new") && open.is("(") && close.is(")") {
                            if let Some(constructor_id) =
                                scopes.flow_binding(&constructor.text, constructor.start)
                            {
                                if scopes.scopes[constructor_id.0]
                                    .values
                                    .get(&constructor_id.1)
                                    .is_some_and(|binding| binding.kind == BindingKind::Class)
                                {
                                    actual = Type::Named {
                                        name: constructor_id.1,
                                        arguments: Vec::new(),
                                    };
                                }
                            }
                        }
                    }
                }
                if declared == Type::Any
                    || (declared == Type::Unknown && scopes.flow_annotated(&id))
                    || !matches!(
                        predicates::expand(scopes, &declared),
                        Type::Union(_) | Type::Unknown | Type::Any
                    ) && actual != Type::Unknown
                {
                    actual = declared;
                }
                if actual != Type::Unknown {
                    let mut next = after;
                    next.invalidate(&id);
                    if actual == scopes.flow_declared(&id) {
                        next.remove(&id);
                    } else {
                        next.insert(id, actual);
                    }
                    return next;
                }
            }
        } else if let Some((id, path)) = predicates::reference(scopes, target, 0) {
            let mut next = after;
            next.invalidate(&id);
            next.remove(&id);
            let owner = scopes.flow_declared(&id);
            if properties::accessor_target(scopes, &owner, &path) {
                return next;
            }
            if let Some(declared) = properties::project(scopes, &owner, &path) {
                let actual = infer(
                    &tokens[equal + 1..],
                    &values(scopes, &next, target[0].start),
                    &declared,
                );
                next.properties
                    .insert((id, properties::names(&path)), actual);
            }
            return next;
        }
        return after;
    }
    if let Some(next) = assignments::compound(scopes, tokens, state, infer) {
        return next;
    }
    if let Some(question) = super::super::targets::top_level(tokens, "?") {
        if let Some(colon) = super::super::targets::top_level(&tokens[question + 1..], ":") {
            let colon = question + 1 + colon;
            let mut branches = Vec::new();
            for (range, positive) in [
                (question + 1..colon, true),
                (colon + 1..tokens.len(), false),
            ] {
                if let Some(required) =
                    predicates::narrow(scopes, state, &tokens[..question], positive)
                {
                    for token in &tokens[range.clone()] {
                        model.facts.remove(&token.start);
                    }
                    branches.push(expression(
                        scopes,
                        model,
                        &tokens[range],
                        &required,
                        false,
                        infer,
                        execution,
                        depth + 1,
                        remaining,
                    ));
                }
            }
            return branches
                .into_iter()
                .reduce(|left, right| join(scopes, &left, &right))
                .unwrap_or_else(|| state.clone());
        }
    }
    for operator in ["||", "&&", "??"] {
        if let Some(index) = super::super::targets::top_level(tokens, operator) {
            let left = expression(
                scopes,
                model,
                &tokens[..index],
                state,
                false,
                infer,
                execution,
                depth + 1,
                remaining,
            );
            let positive = operator != "||";
            let required = if operator == "??" {
                Some(left.clone())
            } else {
                predicates::narrow(scopes, &left, &tokens[..index], positive)
            };
            if let Some(required) = required {
                // Replace the provisional parent snapshot: this operand is
                // reached only through its own logical predecessor.
                for token in &tokens[index + 1..] {
                    model.facts.remove(&token.start);
                }
                let right = expression(
                    scopes,
                    model,
                    &tokens[index + 1..],
                    &required,
                    false,
                    infer,
                    execution,
                    depth + 1,
                    remaining,
                );
                return join(scopes, &left, &right);
            }
            return left;
        }
    }
    let mut next = state.clone();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if let Some(function) = scopes.module.nested_functions.get(&token.start) {
            let end = tokens.partition_point(|token| token.start < function.span.end);
            let mut after = end;
            while tokens.get(after).is_some_and(|token| token.is(")")) {
                after += 1;
            }
            if tokens.get(after).is_some_and(|token| token.is("(")) {
                if let Some(graph) =
                    graph::execution(scopes, scopes.flow_execution(function.span.start))
                {
                    next = run_graph(scopes, model, &graph, next, infer, remaining, depth + 1);
                }
            }
            index = end.max(index + 1);
            continue;
        }
        if matches!(token.text.as_str(), "(" | "[") {
            if let Some(end) = super::super::targets::close(tokens, index) {
                let immediate = tokens[index + 1..end]
                    .iter()
                    .find_map(|token| scopes.module.nested_functions.get(&token.start))
                    .filter(|function| {
                        tokens[end + 1..].first().is_some_and(|token| token.is("("))
                            && tokens[index + 1..end]
                                .iter()
                                .filter(|token| token.start >= function.span.end)
                                .all(|token| token.is(")"))
                    });
                if let Some(function) = immediate {
                    if let Some(graph) =
                        graph::execution(scopes, scopes.flow_execution(function.span.start))
                    {
                        next = run_graph(scopes, model, &graph, next, infer, remaining, depth + 1);
                    }
                    index = end + 1;
                    continue;
                }
                next = expression(
                    scopes,
                    model,
                    &tokens[index + 1..end],
                    &next,
                    false,
                    infer,
                    execution,
                    depth + 1,
                    remaining,
                );
                index = end + 1;
                continue;
            }
        }
        index += 1;
    }
    if effects_allowed && bare_expression {
        calls::effect(scopes, model, tokens, &next).unwrap_or(next)
    } else {
        next
    }
}
