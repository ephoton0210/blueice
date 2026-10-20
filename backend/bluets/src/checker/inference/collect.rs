// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structural inference terminates repeated alias pairs without losing siblings.
use super::*;

pub(super) struct Collector<'a> {
    pub(super) parameters: &'a BTreeSet<String>,
    pub(super) candidates: BTreeMap<String, candidates::Candidates>,
    pub(super) aliases: &'a BTreeMap<String, TypeDefinition>,
    pub(super) budget: &'a mut TypeExpansionBudget,
    pub(super) active: HashSet<(String, String, bool)>,
}

impl Collector<'_> {
    pub(super) fn collect(&mut self, template: &Type, actual: &Type, contra: bool) {
        if !self.budget.consume() {
            return;
        }
        let pair = (type_identity(template), type_identity(actual), contra);
        if !self.active.insert(pair.clone()) {
            return;
        }
        self.inner(template, actual, contra);
        self.active.remove(&pair);
    }

    fn inner(&mut self, template: &Type, actual: &Type, contra: bool) {
        if let Type::Named { name, arguments } = template {
            if arguments.is_empty() && self.parameters.contains(name) {
                self.candidates
                    .entry(name.clone())
                    .or_default()
                    .add(actual, contra);
                return;
            }
        }
        if let Type::Conditional(value) = template {
            // Infer through the structural branch before evaluating its check.
            // The selected call still validates the instantiated conditional.
            self.collect(&value.when_true, actual, contra);
            return;
        }
        if let Type::Mapped(value) = template {
            if let Some((name, inferred)) =
                super::mapped::infer(value, actual, self.parameters, self.aliases, self.budget)
            {
                self.candidates
                    .entry(name)
                    .or_default()
                    .add(&inferred, contra);
                return;
            }
        }
        if let Some(expanded) = instantiate_named(
            template,
            self.aliases,
            &mut HashSet::new(),
            self.budget,
            "inference template",
        ) {
            self.collect(&expanded, actual, contra);
            return;
        }
        if let Some(expanded) = instantiate_named(
            actual,
            self.aliases,
            &mut HashSet::new(),
            self.budget,
            "inference argument",
        ) {
            self.collect(template, &expanded, contra);
            return;
        }
        match (template, actual) {
            (Type::Readonly(template), actual) => self.collect(template, actual, contra),
            (template, Type::Readonly(actual)) => self.collect(template, actual, contra),
            (Type::Array(template), Type::Array(actual)) => {
                self.collect(template, &widen(actual), contra)
            }
            (Type::Array(template), Type::Tuple(actuals)) => {
                for actual in actuals {
                    self.collect(template, &widen(&actual.annotation), contra);
                }
            }
            (Type::Tuple(templates), Type::Tuple(actuals)) => {
                for (template, actual) in templates.iter().zip(actuals) {
                    self.collect(&template.annotation, &widen(&actual.annotation), contra);
                }
            }
            (Type::Record(templates), Type::Record(actuals)) => {
                for template in templates {
                    if let Some(actual) = actuals.iter().find(|a| a.name == template.name) {
                        self.collect(&template.value, &actual.value, contra);
                    }
                }
            }
            (
                Type::Function {
                    parameters: templates,
                    result: template,
                },
                Type::Function {
                    parameters: actuals,
                    result: actual,
                },
            ) => {
                for (template, actual) in templates.iter().zip(actuals) {
                    if let (Some(template), Some(actual)) =
                        (&template.annotation, &actual.annotation)
                    {
                        self.collect(template, actual, !contra);
                    }
                }
                self.collect(template, actual, contra);
            }
            (Type::Union(templates), actual) => {
                for template in templates {
                    if !matches!(template, Type::Undefined | Type::Null) {
                        self.collect(template, actual, contra);
                    }
                }
            }
            _ => {}
        }
    }
}
