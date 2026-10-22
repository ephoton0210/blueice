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
    pub(super) evidence: usize,
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
                if *actual != Type::Unknown {
                    self.evidence += 1;
                }
                self.candidates
                    .entry(name.clone())
                    .or_default()
                    .add(actual, contra);
                return;
            }
        }
        if let Type::Union(values) = actual {
            let previous = (!contra).then(|| std::mem::take(&mut self.candidates));
            for value in values {
                self.collect(template, value, contra);
            }
            if let Some(previous) = previous {
                let collected = std::mem::replace(&mut self.candidates, previous);
                for (name, candidates) in collected {
                    self.candidates
                        .entry(name)
                        .or_default()
                        .merge_union(candidates);
                }
            }
            return;
        }
        if let (
            Type::Named { name, .. },
            Type::Named {
                name: actual_name,
                arguments,
            },
        ) = (template, actual)
        {
            if let Some(definition) = self
                .aliases
                .get(actual_name)
                .filter(|d| d.kind == TypeDefinitionKind::LibraryInterface)
            {
                if let Type::Intersection(parts) = &definition.value {
                    if let Some(parent) = parts.iter().find(
                        |part| matches!(part, Type::Named {name: parent, ..} if parent == name),
                    ) {
                        if let Some(arguments) =
                            complete_type_arguments(&definition.parameters, arguments)
                        {
                            let substitutions = definition
                                .parameters
                                .iter()
                                .map(|p| p.name.clone())
                                .zip(arguments)
                                .collect();
                            let parent = substitute_type(parent, &substitutions);
                            self.collect(template, &parent, contra);
                            return;
                        }
                    }
                }
            }
        }
        if let Type::Union(templates) = template {
            let naked = |value: &Type| matches!(value, Type::Named {name, arguments} if arguments.is_empty() && self.parameters.contains(name));
            let (direct, structured): (Vec<_>, Vec<_>) =
                templates.iter().partition(|value| naked(value));
            let evidence = self.evidence;
            for template in structured {
                if !matches!(template, Type::Undefined | Type::Null) {
                    self.collect(template, actual, contra);
                }
            }
            if self.evidence == evidence {
                for template in direct {
                    self.collect(template, actual, contra);
                }
            }
            return;
        }
        // Library container identities carry their declared argument evidence.
        // Expanding recursive methods first can lose that evidence and exhaust
        // the same owner's budget. Owner-defined replacements stay structural.
        if let (
            Type::Named { name, arguments },
            Type::Named {
                name: actual_name,
                arguments: actual_arguments,
            },
        ) = (template, actual)
        {
            if name == actual_name
                && arguments.len() == actual_arguments.len()
                && self.aliases.get(name).is_some_and(|definition| {
                    definition.kind == TypeDefinitionKind::LibraryInterface
                })
            {
                for (template, actual) in arguments.iter().zip(actual_arguments) {
                    self.collect(template, actual, contra);
                }
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
                if inferred != Type::Unknown {
                    self.evidence += 1;
                }
                self.candidates
                    .entry(name)
                    .or_default()
                    .add(&inferred, contra);
                return;
            }
        }
        if self.collect_indices(template, actual, contra) {
            return;
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
        if self.collect_surface(template, actual, contra) {
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
                Type::GenericFunction {
                    type_parameters: template_locals,
                    parameters: template_parameters,
                    result: template_result,
                    ..
                },
                Type::GenericFunction {
                    type_parameters: actual_locals,
                    parameters: actual_parameters,
                    result: actual_result,
                    ..
                },
            ) => {
                // Defaults are evaluated in the enclosing generic scope.
                for (template, actual) in template_locals.iter().zip(actual_locals) {
                    if let (Some(template), Some(actual)) = (&template.default, &actual.default) {
                        self.collect(template, actual, contra);
                    }
                }
                let mut parameters = self.parameters.clone();
                for parameter in template_locals.iter().chain(actual_locals) {
                    parameters.remove(&parameter.name);
                }
                let mut nested = Collector {
                    parameters: &parameters,
                    candidates: std::mem::take(&mut self.candidates),
                    aliases: self.aliases,
                    budget: self.budget,
                    active: std::mem::take(&mut self.active),
                    evidence: self.evidence,
                };
                nested.collect_parameters(template_parameters, actual_parameters, contra);
                nested.collect(template_result, actual_result, contra);
                self.candidates = nested.candidates;
                self.active = nested.active;
                self.evidence = nested.evidence;
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
                self.collect_parameters(templates, actuals, contra);
                self.collect(template, actual, contra);
            }
            _ => {}
        }
    }
}
