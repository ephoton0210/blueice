// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Positive and negative predicates on lexical values and discriminants.

use super::*;

pub(super) fn narrow(
    scopes: &ScopeModel<'_>,
    state: &State,
    tokens: &[Token],
    positive: bool,
) -> Option<State> {
    narrow_at(scopes, state, tokens, positive, 0)
}

fn narrow_at(
    scopes: &ScopeModel<'_>,
    state: &State,
    tokens: &[Token],
    positive: bool,
    depth: usize,
) -> Option<State> {
    if depth > scopes.max_type_expansions.max(32) {
        scopes.flow_limit(tokens, "flow predicate exceeds its bounded nesting limit");
        return Some(state.clone());
    }
    let tokens = super::super::targets::strip(tokens);
    if tokens.is_empty() {
        return positive.then(|| state.clone());
    }
    if tokens[0].is("!") {
        return narrow_at(scopes, state, &tokens[1..], !positive, depth + 1);
    }
    for (operator, required) in [("||", false), ("&&", true)] {
        if let Some(index) = super::super::targets::top_level(tokens, operator) {
            let left = &tokens[..index];
            let right = &tokens[index + 1..];
            if positive == required {
                let first = narrow_at(scopes, state, left, positive, depth + 1)?;
                return narrow_at(scopes, &first, right, positive, depth + 1);
            }
            let first = narrow_at(scopes, state, left, positive, depth + 1);
            let second = narrow_at(scopes, state, left, !positive, depth + 1)
                .and_then(|next| narrow_at(scopes, &next, right, positive, depth + 1));
            return match (first, second) {
                (Some(left), Some(right)) => Some(join(scopes, &left, &right)),
                (left, right) => left.or(right),
            };
        }
    }
    for operator in ["===", "!==", "==", "!="] {
        if let Some(index) = super::super::targets::top_level(tokens, operator) {
            let equality = positive == matches!(operator, "===" | "==");
            let loose = matches!(operator, "==" | "!=");
            let left = &tokens[..index];
            let right = &tokens[index + 1..];
            for (subject, literal) in [(left, right), (right, left)] {
                let [literal] = super::super::targets::strip(literal) else {
                    continue;
                };
                if subject.first().is_some_and(|token| token.is("typeof"))
                    && literal.kind == TokenKind::String
                {
                    if let Some((id, path)) = reference(scopes, &subject[1..], depth + 1) {
                        return filter(scopes, state, id, &path, |value| {
                            typeof_filter(value, literal.text.trim_matches(['\'', '"']), equality)
                        });
                    }
                }
                if matches!(literal.kind, TokenKind::String | TokenKind::Number)
                    || matches!(
                        literal.text.as_str(),
                        "true" | "false" | "null" | "undefined"
                    )
                {
                    if let Some((id, path)) = reference(scopes, subject, depth + 1) {
                        return filter(scopes, state, id, &path, |value| {
                            equality_filter(value, literal, equality, loose)
                        });
                    }
                }
            }
            return Some(state.clone());
        }
    }
    if let Some(index) = super::super::targets::top_level(tokens, "in") {
        if let [key] = &tokens[..index] {
            if key.kind == TokenKind::String {
                if let Some((id, path)) = reference(scopes, &tokens[index + 1..], depth + 1) {
                    let key = key.text.trim_matches(['\'', '"']);
                    return filter(scopes, state, id, &path, |value| {
                        let expanded = expand(scopes, value);
                        if let Type::Record(fields) = &expanded {
                            let field = fields.iter().find(|field| field.name == key);
                            return if field.is_some_and(|field| field.optional)
                                || field.is_some() == positive
                            {
                                Some(value.clone())
                            } else {
                                None
                            };
                        }
                        Some(value.clone())
                    });
                }
            }
        }
    }
    if let Some(index) = super::super::targets::top_level(tokens, "instanceof") {
        if let [constructor] = &tokens[index + 1..] {
            if let Some((id, path)) = reference(scopes, &tokens[..index], depth + 1) {
                let binding = scopes.flow_binding(&constructor.text, constructor.start);
                let local_class = binding
                    .as_ref()
                    .filter(|id| {
                        scopes.scopes[id.0]
                            .values
                            .get(&id.1)
                            .is_some_and(|binding| binding.kind == BindingKind::Class)
                    })
                    .map(|id| id.1.clone());
                let bound = binding.map(|id| scopes.flow_declared(&id));
                let class = match bound {
                    Some(Type::Named { name, .. }) => {
                        name.strip_prefix("typeof ").map(str::to_string)
                    }
                    _ => local_class,
                };
                if let Some(class) = class {
                    return filter(scopes, state, id, &path, |value| {
                        let matches = matches!(value, Type::Named {name,..} if properties::derives(scopes, name, &class));
                        (matches == positive).then(|| value.clone())
                    });
                }
            }
        }
    }
    if let [literal] = tokens {
        if matches!(
            literal.text.as_str(),
            "true" | "false" | "null" | "undefined"
        ) || literal.kind == TokenKind::Number
            || literal.kind == TokenKind::String
        {
            return truthy(&Type::Literal(literal.text.clone()), positive).map(|_| state.clone());
        }
    }
    if let Some((id, path)) = reference(scopes, tokens, depth + 1) {
        return filter(scopes, state, id, &path, |value| truthy(value, positive));
    }
    Some(state.clone())
}

pub(super) fn reference(
    scopes: &ScopeModel<'_>,
    tokens: &[Token],
    depth: usize,
) -> Option<(BindingId, Vec<(String, bool)>)> {
    if depth > scopes.max_type_expansions.max(32) {
        return None;
    }
    let tokens = super::super::targets::strip(tokens);
    let first = tokens.first()?;
    if first.kind != TokenKind::Identifier && !first.is("this") {
        return None;
    }
    let id = scopes.flow_binding(&first.text, first.start)?;
    let mut path = Vec::new();
    for pair in tokens[1..].chunks(2) {
        if let [dot, property] = pair {
            if !dot.is(".") && !dot.is("?.") {
                return None;
            }
            path.push((property.text.clone(), dot.is("?.")));
        } else {
            return None;
        }
    }
    if path.is_empty() {
        if let Some(alias) = scopes.flow_alias(&id) {
            if alias.len() == 3 && alias[1].is(".") {
                if let Some((root, path)) = reference(scopes, &alias, depth + 1) {
                    if !scopes.mutations.iter().any(|mutation| {
                        alias[0].start < mutation.operator.start
                            && mutation.operator.start < first.start
                            && scopes
                                .flow_binding(&root.1, mutation.operator.start)
                                .as_ref()
                                == Some(&root)
                    }) {
                        return Some((root, path));
                    }
                }
            }
        }
    }
    Some((id, path))
}

pub(super) fn expand(scopes: &ScopeModel<'_>, value: &Type) -> Type {
    let mut budget = TypeExpansionBudget::new(scopes.max_type_expansions);
    let mut visited = HashSet::new();
    let mut result = value.clone();
    while matches!(result, Type::Named { .. }) {
        let Some(next) = instantiate_named(
            &result,
            &scopes.type_definitions,
            &mut visited,
            &mut budget,
            "flow predicate",
        ) else {
            break;
        };
        result = next;
    }
    if budget.exhausted {
        scopes.flow_limit(&[], "flow type lookup exceeds its generic-expansion limit");
    }
    result
}

pub(super) fn parts(scopes: &ScopeModel<'_>, value: &Type) -> Vec<Type> {
    let expanded = match value {
        Type::Named { name, .. }
            if scopes
                .type_definitions
                .get(name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias) =>
        {
            expand(scopes, value)
        }
        _ => value.clone(),
    };
    match expanded {
        Type::Union(values) => values,
        value => vec![value],
    }
}

fn filter(
    scopes: &ScopeModel<'_>,
    state: &State,
    id: BindingId,
    path: &[(String, bool)],
    apply: impl Fn(&Type) -> Option<Type>,
) -> Option<State> {
    let base = current(scopes, state, &id);
    let narrowed = union(parts(scopes, &base).iter().filter_map(|value| {
        if path.is_empty() {
            return apply(value);
        }
        let property = properties::project(scopes, value, path)?;
        parts(scopes, &property)
            .iter()
            .any(|property| apply(property).is_some())
            .then(|| value.clone())
    }));
    let mut next = state.clone();
    next.insert(id.clone(), narrowed.clone());
    if !path.is_empty() {
        if let Some(property) = properties::read(scopes, state, &id, path) {
            let projected = union(parts(scopes, &property).iter().filter_map(&apply));
            next.properties
                .insert((id, properties::names(path)), projected);
        }
    }
    if narrowed == Type::Never {
        next.reachable = false;
    }
    Some(next)
}

fn typeof_filter(value: &Type, kind: &str, positive: bool) -> Option<Type> {
    if matches!(value, Type::Any | Type::Unknown) {
        return if !positive {
            Some(value.clone())
        } else {
            Some(match kind {
                "string" => Type::String,
                "number" => Type::Number,
                "boolean" => Type::Boolean,
                "undefined" => Type::Undefined,
                "object" => Type::Union(vec![Type::Record(Vec::new()), Type::Null]),
                _ => value.clone(),
            })
        };
    }
    let primitive = match value {
        Type::Literal(text) => literal_primitive(text),
        value => value.clone(),
    };
    let actual = match primitive {
        Type::String => "string",
        Type::Number => "number",
        Type::Boolean => "boolean",
        Type::Undefined | Type::Void => "undefined",
        Type::Function { .. } => "function",
        _ => "object",
    };
    ((actual == kind) == positive).then(|| value.clone())
}

fn equality_filter(value: &Type, literal: &Token, positive: bool, loose: bool) -> Option<Type> {
    let expected = match literal.text.as_str() {
        "null" => Type::Null,
        "undefined" => Type::Undefined,
        _ => Type::Literal(literal.text.clone()),
    };
    if *value == Type::Any || *value == Type::Unknown {
        return Some(if positive { expected } else { value.clone() });
    }
    let nullish = matches!(value, Type::Null | Type::Undefined);
    let matches = if loose && matches!(expected, Type::Null | Type::Undefined) {
        nullish
    } else {
        match (value, &expected) {
            (Type::Literal(left), Type::Literal(right)) => {
                left.trim_matches(['\'', '"']) == right.trim_matches(['\'', '"'])
            }
            _ => value == &expected,
        }
    };
    if let Type::Literal(text) = &expected {
        if *value == literal_primitive(text) {
            return Some(if positive { expected } else { value.clone() });
        }
    }
    (matches == positive).then(|| value.clone())
}

fn truthy(value: &Type, positive: bool) -> Option<Type> {
    match value {
        Type::Null | Type::Undefined | Type::Void | Type::Never => {
            (!positive).then(|| value.clone())
        }
        Type::Boolean => Some(Type::Literal(
            if positive { "true" } else { "false" }.into(),
        )),
        Type::String | Type::Number | Type::Unknown | Type::Any => Some(value.clone()),
        Type::Literal(text) => {
            let truth = !matches!(
                text.as_str(),
                "false" | "null" | "undefined" | "0" | "-0" | "''" | "\"\""
            );
            (truth == positive).then(|| value.clone())
        }
        _ => positive.then(|| value.clone()),
    }
}

pub(super) fn truthy_type(value: &Type, positive: bool) -> Option<Type> {
    let parts = match value {
        Type::Union(parts) => parts.as_slice(),
        value => std::slice::from_ref(value),
    };
    let value = union(parts.iter().filter_map(|value| truthy(value, positive)));
    (value != Type::Never).then_some(value)
}
