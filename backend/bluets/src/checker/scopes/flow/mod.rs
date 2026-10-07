// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded lexical flow facts over the retained, authorized source graph.

use super::flow_bindings::BindingId;
use super::*;

mod assignments;
mod calls;
mod evaluate;
mod graph;
mod predicates;
mod properties;

pub(crate) const VERSION: &str = "lexical-flow-v8";
#[derive(Clone, Debug, PartialEq)]
struct State {
    reachable: bool,
    bindings: BTreeMap<BindingId, Type>,
    properties: BTreeMap<(BindingId, Vec<String>), Type>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            reachable: true,
            bindings: BTreeMap::new(),
            properties: BTreeMap::new(),
        }
    }
}

impl State {
    fn new() -> Self {
        Self::default()
    }

    fn invalidate(&mut self, id: &BindingId) {
        self.properties.retain(|(owner, _), _| owner != id);
    }
}

impl std::ops::Deref for State {
    type Target = BTreeMap<BindingId, Type>;
    fn deref(&self) -> &Self::Target {
        &self.bindings
    }
}

impl std::ops::DerefMut for State {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bindings
    }
}

#[derive(Default)]
pub(in crate::checker) struct FlowModel {
    facts: BTreeMap<usize, State>,
    completions: BTreeMap<usize, bool>,
    returns: BTreeMap<usize, Vec<Vec<Token>>>,
    pub(in crate::checker) diagnostics: Vec<Diagnostic>,
}

impl FlowModel {
    pub(in crate::checker) fn build(
        scopes: &ScopeModel<'_>,
        infer: impl Fn(&[Token], &BTreeMap<String, Type>, &Type) -> Type,
    ) -> Self {
        evaluate::build(scopes, &infer)
    }

    pub(in crate::checker) fn has_return(&self, start: usize) -> bool {
        self.returns
            .get(&start)
            .is_some_and(|returned| !returned.is_empty())
    }

    pub(in crate::checker) fn completes(&self, start: usize) -> Option<bool> {
        self.completions.get(&start).copied()
    }

    pub(in crate::checker) fn returned_in(
        &self,
        function: usize,
        span: &SourceSpan,
    ) -> Vec<Vec<Token>> {
        self.returns
            .get(&function)
            .into_iter()
            .flatten()
            .filter(|tokens| {
                tokens
                    .first()
                    .is_some_and(|token| span.start <= token.start && token.start < span.end)
            })
            .cloned()
            .collect()
    }

    pub(in crate::checker) fn query(&self, scopes: &ScopeModel<'_>, token: &Token) -> Option<Type> {
        let id = scopes.flow_binding(&token.text, token.start)?;
        self.facts
            .get(&token.start)
            .and_then(|state| state.get(&id))
            .cloned()
            .or_else(|| {
                scopes
                    .flow_shadowed(&id)
                    .then(|| scopes.flow_declared(&id))
                    .filter(|value| !matches!(value, Type::Unknown))
            })
    }

    pub(in crate::checker) fn property(
        &self,
        scopes: &ScopeModel<'_>,
        tokens: &[Token],
    ) -> Option<Type> {
        let (id, path) = predicates::reference(scopes, tokens, 0)?;
        if path.is_empty() {
            return None;
        }
        let state = self.facts.get(&tokens.first()?.start)?;
        let value = state
            .properties
            .get(&(id.clone(), properties::names(&path)))?
            .clone();
        if path.iter().any(|(_, optional)| *optional) {
            return Some(value);
        }
        // A plain property access reports the nullable receiver separately;
        // its result excludes undefined introduced only by optional short circuit.
        let projected = properties::project(scopes, &current(scopes, state, &id), &path)?;
        let parts = predicates::parts(scopes, &projected);
        Some(union(predicates::parts(scopes, &value).into_iter().filter(
            |part| !matches!(part, Type::Null | Type::Undefined) || parts.contains(part),
        )))
    }
}

fn current(scopes: &ScopeModel<'_>, state: &State, id: &BindingId) -> Type {
    state
        .get(id)
        .cloned()
        .unwrap_or_else(|| scopes.flow_declared(id))
}

fn union(values: impl IntoIterator<Item = Type>) -> Type {
    let mut result = Vec::new();
    for value in values {
        let parts = match value {
            Type::Union(parts) => parts,
            value => vec![value],
        };
        for part in parts {
            if part != Type::Never && !result.iter().any(|value| equivalent(value, &part)) {
                result.push(part);
            }
        }
    }
    // An unrestricted primitive absorbs its literals at a reachable join.
    let broad = result.clone();
    result.retain(
        |value| !matches!(value, Type::Literal(text) if broad.contains(&literal_primitive(text))),
    );
    match result.len() {
        0 => Type::Never,
        1 => result.remove(0),
        _ => Type::Union(result),
    }
}

fn literal_primitive(text: &str) -> Type {
    if text.starts_with(['\'', '"', '`']) {
        Type::String
    } else if matches!(text, "true" | "false") {
        Type::Boolean
    } else {
        Type::Number
    }
}

fn equivalent(left: &Type, right: &Type) -> bool {
    match (left, right) {
        (Type::Record(left), Type::Record(right)) => {
            left.len() == right.len()
                && left.iter().all(|field| {
                    right
                        .iter()
                        .find(|other| other.name == field.name)
                        .is_some_and(|other| {
                            field.optional == other.optional
                                && field.readonly == other.readonly
                                && equivalent(&field.value, &other.value)
                        })
                })
        }
        (Type::Array(left), Type::Array(right)) => equivalent(left, right),
        (Type::Union(left), Type::Union(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|value| right.iter().any(|other| equivalent(value, other)))
        }
        _ => left == right,
    }
}

fn join(scopes: &ScopeModel<'_>, left: &State, right: &State) -> State {
    if !left.reachable {
        return right.clone();
    }
    if !right.reachable {
        return left.clone();
    }
    let bindings = left.keys()
        .chain(right.keys())
        .map(|id| {
            let left = current(scopes, left, id);
            let right = current(scopes, right, id);
            let expanded = match &right {
                Type::Union(parts) => {
                    let earlier = match &left {
                        Type::Union(parts) => parts.as_slice(),
                        value => std::slice::from_ref(value),
                    };
                    parts.len() > earlier.len()
                        && earlier
                            .iter()
                            .all(|value| parts.iter().any(|part| equivalent(value, part)))
                }
                _ => false,
            };
            let mut value = if expanded { union([right,left]) } else { union([left,right]) };
            let declared = scopes.flow_declared(id);
            if matches!(&declared,Type::Union(parts) if parts.iter().all(|part| matches!(part,Type::String|Type::Number|Type::Boolean|Type::Null|Type::Undefined)))
                && equivalent(&value,&declared) {value = declared;}
            (id.clone(),value)
        })
        .collect();
    let mut properties = BTreeMap::new();
    for key in left.properties.keys().chain(right.properties.keys()) {
        let path = key
            .1
            .iter()
            .map(|name| (name.clone(), false))
            .collect::<Vec<_>>();
        if let (Some(left), Some(right)) = (
            properties::read(scopes, left, &key.0, &path),
            properties::read(scopes, right, &key.0, &path),
        ) {
            properties.insert(key.clone(), union([left, right]));
        }
    }
    State {
        reachable: true,
        bindings,
        properties,
    }
}

fn values(scopes: &ScopeModel<'_>, state: &State, offset: usize) -> BTreeMap<String, Type> {
    let mut result = scopes.flow_values(offset);
    for (name, value) in &mut result {
        if let Some(id) = scopes.flow_binding(name, offset) {
            if let Some(flow) = state.get(&id) {
                *value = flow.clone();
            }
        }
    }
    result
}
