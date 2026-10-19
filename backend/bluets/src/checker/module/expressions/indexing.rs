// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded-type inference for a supported computed receiver.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_tuple_index_reads(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let execution = self
            .scopes
            .as_ref()
            .map(|scopes| scopes.flow_execution(span.start));
        for open in 1..tokens.len() {
            let receiver = &tokens[open - 1];
            if !tokens[open].is("[")
                || receiver.kind != TokenKind::Identifier
                || open
                    .checked_sub(2)
                    .is_some_and(|previous| tokens[previous].is(".") || tokens[previous].is("?."))
                || self
                    .scopes
                    .as_ref()
                    .is_some_and(|scopes| Some(scopes.flow_execution(receiver.start)) != execution)
            {
                continue;
            }
            let Some(close) = crate::checker::scopes::targets::close(tokens, open) else {
                continue;
            };
            let Some(index) = canonical_index_key(&tokens[open + 1..close]) else {
                continue;
            };
            let original = self.infer_expression(&tokens[open - 1..open], scope);
            let mut value = original.clone();
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let mut visited = HashSet::new();
            while let Some(expanded) =
                instantiate_named(&value, &self.types, &mut visited, &mut budget, "tuple read")
            {
                value = expanded;
            }
            let tuple = match &value {
                Type::Readonly(value) => value.as_ref(),
                value => value,
            };
            let Type::Tuple(elements) = tuple else {
                continue;
            };
            if index < elements.len() || elements.iter().any(|element| element.rest) {
                continue;
            }
            let label = type_label(&original);
            self.typescript_type_error(
                &SourceSpan::new(
                    &self.module.id,
                    tokens[open + 1].start,
                    tokens[close - 1].end,
                ),
                format!("tuple type `{label}` has no element at index {index}"),
                DiagnosticCode::TypeMismatch,
                2493,
                vec![label, elements.len().to_string(), index.to_string()],
            );
        }
    }
}

/// Preserve every declared possibility rather than losing readonly types in
/// an inferred local alias. A canonical index selects one tuple element;
/// otherwise a heterogeneous container yields a union of its value types.
pub(in crate::checker::module) fn indexed_value_type(
    value: &Type,
    index: Option<usize>,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> Type {
    match value {
        Type::Readonly(value) => indexed_value_type(value, index, aliases, visited, budget),
        Type::IndexedRecord { indices, .. } => {
            alternatives_type(indices.iter().map(|index| index.value.clone()).collect())
        }
        Type::Array(element) => {
            if budget.checking.no_unchecked_indexed_access {
                Type::Union(vec![(**element).clone(), Type::Undefined])
            } else {
                (**element).clone()
            }
        }
        Type::Tuple(values) => {
            if values.iter().any(|element| {
                element.rest && matches!(element.annotation, Type::Named { .. } | Type::Tuple(_))
            }) {
                let Ok(expanded) =
                    expand_concrete_tuple_spreads(values, aliases, &mut HashSet::new(), budget)
                else {
                    return Type::Unknown;
                };
                alternatives_type(tuple_indexed_candidates(&expanded, index))
            } else {
                alternatives_type(tuple_indexed_candidates(values, index))
            }
        }
        Type::Record(fields) if index.is_none() => alternatives_type(
            fields
                .iter()
                .map(|field| {
                    if field.optional {
                        Type::Union(vec![field.value.clone(), Type::Undefined])
                    } else {
                        field.value.clone()
                    }
                })
                .collect(),
        ),
        Type::Union(parts) => alternatives_type(
            parts
                .iter()
                .map(|part| indexed_value_type(part, index, aliases, &mut visited.clone(), budget))
                .collect(),
        ),
        Type::Named { .. } => {
            match instantiate_named(value, aliases, visited, budget, "computed receiver") {
                Some(instantiated) => {
                    indexed_value_type(&instantiated, index, aliases, visited, budget)
                }
                None => Type::Unknown,
            }
        }
        _ => Type::Unknown,
    }
}

pub(super) fn tuple_indexed_candidates(
    values: &[TupleTypeElement],
    index: Option<usize>,
) -> Vec<Type> {
    let Some(index) = index else {
        return values.iter().map(TupleTypeElement::indexed_type).collect();
    };
    if let Some(rest_index) = values
        .iter()
        .position(|value| value.rest)
        .filter(|index| index + 1 < values.len())
    {
        if index < rest_index {
            return vec![values[index].value_type()];
        }
        let mut candidates = vec![values[rest_index].indexed_type()];
        candidates.extend(
            values[rest_index + 1..]
                .iter()
                .take(index.saturating_sub(rest_index).saturating_add(1))
                .map(TupleTypeElement::value_type),
        );
        return candidates;
    }
    values
        .get(index)
        .or_else(|| values.last().filter(|value| value.rest))
        .map_or_else(Vec::new, |value| vec![value.indexed_type()])
}

pub(super) fn canonical_index_key(tokens: &[Token]) -> Option<usize> {
    let [key] = tokens else { return None };
    let text = match key.kind {
        TokenKind::Number => key.text.as_str(),
        TokenKind::String => unescaped_property_name(&key.text)?,
        _ => return None,
    };
    let index = text.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == text).then_some(index as usize)
}

fn alternatives_type(values: Vec<Type>) -> Type {
    let Some(first) = values.first() else {
        return Type::Unknown;
    };
    if values.iter().all(|value| value == first) {
        first.clone()
    } else {
        Type::Union(values)
    }
}
