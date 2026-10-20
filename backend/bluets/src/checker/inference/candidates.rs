// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Candidate direction is retained until a parameter's solution is selected.
use super::*;

#[derive(Default)]
pub(super) struct Candidates {
    covariant: Vec<Type>,
    contravariant: Vec<Type>,
}

impl Candidates {
    pub(super) fn add(&mut self, value: &Type, contra: bool) {
        // An unfinished contextual parameter is not evidence for a solution.
        if matches!(value, Type::Unknown) {
            return;
        }
        let values = if contra {
            &mut self.contravariant
        } else {
            &mut self.covariant
        };
        if !values.contains(value) {
            values.push(value.clone());
        }
    }

    pub(super) fn solve(
        &self,
        preserve: bool,
        aliases: &BTreeMap<String, TypeDefinition>,
        budget: &mut TypeExpansionBudget,
    ) -> Option<Type> {
        let accepts = |actual: &Type, expected: &Type, budget: &mut TypeExpansionBudget| {
            is_assignable(actual, expected, aliases, &mut HashSet::new(), budget)
        };
        let covariant = if self.covariant.is_empty() {
            None
        } else if self.covariant.iter().all(|v| matches!(v, Type::Literal(_)))
            && self
                .covariant
                .iter()
                .all(|v| widen(v) == widen(&self.covariant[0]))
        {
            Some(if preserve {
                super::super::type_operators::union(self.covariant.clone())
            } else {
                widen(&self.covariant[0])
            })
        } else {
            Some(
                self.covariant
                    .iter()
                    .find(|candidate| {
                        self.covariant
                            .iter()
                            .all(|other| accepts(other, candidate, budget))
                    })
                    .unwrap_or(&self.covariant[0])
                    .clone(),
            )
        };
        if let Some(candidate) = covariant.as_ref() {
            if self
                .contravariant
                .iter()
                .all(|constraint| accepts(candidate, constraint, budget))
            {
                return Some(if preserve {
                    candidate.clone()
                } else {
                    widen(candidate)
                });
            }
        }
        self.contravariant
            .iter()
            .find(|candidate| {
                self.contravariant
                    .iter()
                    .all(|other| accepts(candidate, other, budget))
            })
            .or_else(|| self.contravariant.first())
            .cloned()
            .or(covariant)
    }
}
