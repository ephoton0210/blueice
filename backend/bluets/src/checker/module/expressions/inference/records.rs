// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Record literal result inference, including referenced spread fields.

use super::*;

impl<'a> ModuleChecker<'a> {
    pub(super) fn infer_record(&self, tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
        let mut fields = Vec::new();
        let mut index = 1usize;
        while index < tokens.len() && !tokens[index].is("}") {
            if tokens[index].is("...") {
                let start = index + 1;
                let Some(end) = record_member_end(tokens, start) else {
                    return Type::Unknown;
                };
                if start == end {
                    return Type::Unknown;
                }
                let spread = self.infer_expression(&tokens[start..end], scope);
                let (spread, exhausted) = self.expanded_record_fields(spread);
                if exhausted
                    || (spread.is_none() && self.record_spread_inference_failure.get().is_none())
                {
                    self.record_spread_inference_failure.set(Some((
                        tokens.first().expect("record has opening brace").start,
                        tokens.last().expect("record has closing brace").end,
                        if exhausted {
                            RecordSpreadFailure::ResourceLimit
                        } else {
                            RecordSpreadFailure::UnprovenSource
                        },
                    )));
                }
                let Some(spread) = spread else {
                    return Type::Unknown;
                };
                for field in spread {
                    // Object spread creates a fresh writable property; the
                    // referenced field value retains its own readonly type.
                    insert_inferred_spread_field(&mut fields, field);
                }
                index = end + usize::from(tokens[end].is(","));
                continue;
            }
            let name = tokens[index].text.clone();
            let (value, value_end) = if tokens.get(index + 1).is_some_and(|token| token.is(":")) {
                let value_start = index + 2;
                let Some(value_end) = record_member_end(tokens, value_start) else {
                    return Type::Unknown;
                };
                (
                    self.infer_expression(&tokens[value_start..value_end], scope),
                    value_end,
                )
            } else if tokens
                .get(index + 1)
                .is_some_and(|token| token.is(",") || token.is("}"))
            {
                (
                    scope.get(&name).cloned().unwrap_or(Type::Unknown),
                    index + 1,
                )
            } else {
                return Type::Unknown;
            };
            insert_inferred_record_field(
                &mut fields,
                TypeField {
                    name,
                    readonly: false,
                    optional: false,
                    value,
                    span: SourceSpan::new(
                        "<inferred>",
                        tokens[index].start,
                        tokens[value_end.saturating_sub(1)].end,
                    ),
                },
            );
            index = value_end.saturating_add(1);
        }
        Type::Record(fields)
    }

    fn expanded_record_fields(&self, value: Type) -> (Option<Vec<TypeField>>, bool) {
        let mut visited = HashSet::new();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let fields = self.expand_record_fields(value, &mut visited, &mut budget);
        (fields, budget.exhausted)
    }

    fn expand_record_fields(
        &self,
        value: Type,
        visited: &mut HashSet<String>,
        budget: &mut TypeExpansionBudget,
    ) -> Option<Vec<TypeField>> {
        match value {
            Type::Record(fields) => Some(fields),
            Type::Named { .. } => {
                let expanded =
                    instantiate_named(&value, &self.types, visited, budget, "record spread")?;
                self.expand_record_fields(expanded, visited, budget)
            }
            Type::Union(branches) if !branches.is_empty() => {
                let branch_count = branches.len();
                let mut fields: Vec<(TypeField, usize)> = Vec::new();
                for branch in branches {
                    if !budget.consume() {
                        return None;
                    }
                    for field in self.expand_record_fields(branch, &mut visited.clone(), budget)? {
                        if let Some((existing, count)) = fields
                            .iter_mut()
                            .find(|(existing, _)| existing.name == field.name)
                        {
                            existing.value =
                                merge_conditional_branch_types(existing.value.clone(), field.value);
                            existing.optional |= field.optional;
                            *count += 1;
                        } else {
                            fields.push((field, 1));
                        }
                    }
                }
                Some(
                    fields
                        .into_iter()
                        .map(|(mut field, count)| {
                            field.optional |= count < branch_count;
                            field
                        })
                        .collect(),
                )
            }
            _ => None,
        }
    }
}

fn record_member_end(tokens: &[Token], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            "," | "}" if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn insert_inferred_record_field(fields: &mut Vec<TypeField>, field: TypeField) {
    if let Some(existing) = fields
        .iter_mut()
        .find(|existing| existing.name == field.name)
    {
        *existing = field;
    } else {
        fields.push(field);
    }
}

fn insert_inferred_spread_field(fields: &mut Vec<TypeField>, mut field: TypeField) {
    field.readonly = false;
    if field.optional {
        if let Some(existing) = fields
            .iter_mut()
            .find(|existing| existing.name == field.name)
        {
            existing.value = merge_conditional_branch_types(existing.value.clone(), field.value);
            return;
        }
    }
    insert_inferred_record_field(fields, field);
}
