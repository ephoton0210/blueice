// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A bounded entry guard for the selected iterator-result loop protocol.

use super::*;

pub(super) struct LoopGuard {
    name: String,
    projected: Type,
    guard: usize,
    body: usize,
    end: usize,
}

impl LoopGuard {
    pub(super) fn scope_for(
        &self,
        checker: &ModuleChecker<'_>,
        usage: usize,
        scope: &BTreeMap<String, Type>,
    ) -> Option<BTreeMap<String, Type>> {
        if usage < self.body
            || usage >= self.end
            || !checker.scopes.as_ref().is_some_and(|scopes| {
                scopes.value_guard_holds(&self.name, self.guard, self.body, usage)
            })
        {
            return None;
        }
        let mut narrowed = scope.clone();
        narrowed.insert(self.name.clone(), self.projected.clone());
        Some(narrowed)
    }
}

impl ModuleChecker<'_> {
    pub(super) fn iterator_loop_guard(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<LoopGuard> {
        let [keyword, open, negate, name, dot, field, close, body, ..] = tokens else {
            return None;
        };
        if !keyword.is("while")
            || !open.is("(")
            || !negate.is("!")
            || name.kind != TokenKind::Identifier
            || !dot.is(".")
            || !close.is(")")
            || !body.is("{")
        {
            return None;
        }
        // An opaque top-level expression can include the following statement.
        // End this projection at the matched loop body, not the whole token slice.
        let mut depth = 0usize;
        let mut end = None;
        for token in &tokens[7..] {
            if token.is("{") {
                depth += 1;
            } else if token.is("}") {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    end = Some(token.start);
                    break;
                }
            }
        }
        let end = end?;
        let mut value = scope.get(&name.text)?.clone();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while matches!(value, Type::Named { .. }) {
            value = instantiate_named(
                &value,
                &self.types,
                &mut visited,
                &mut budget,
                "iterator guard",
            )?;
        }
        let Type::Union(parts) = value else {
            return None;
        };
        let mut selected = Vec::new();
        let total = parts.len();
        for part in parts {
            let PropertyType::Found { value, .. } = property_type(
                &part,
                &field.text,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) else {
                return None;
            };
            match value {
                Type::Literal(ref text) if text == "true" => {}
                value if definitely_false(&value) => selected.push(part),
                _ => return None,
            }
        }
        if selected.is_empty() || selected.len() == total {
            return None;
        }
        let projected = if selected.len() == 1 {
            selected.pop()?
        } else {
            Type::Union(selected)
        };
        Some(LoopGuard {
            name: name.text.clone(),
            projected,
            guard: name.start,
            body: body.end,
            end,
        })
    }
}

fn definitely_false(value: &Type) -> bool {
    match value {
        Type::Literal(text) => text == "false",
        Type::Undefined => true,
        Type::Union(parts) => !parts.is_empty() && parts.iter().all(definitely_false),
        _ => false,
    }
}
