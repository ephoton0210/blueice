// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A conditional's true branch retains proven bounds and its infer binders.

use super::*;

fn rest_parameters(value: &Type, names: &mut BTreeSet<String>) {
    if let Type::Tuple(elements) = value {
        for element in elements {
            if element.rest {
                if let Type::Infer(parameter) = &element.annotation {
                    names.insert(parameter.name.clone());
                }
            }
            rest_parameters(&element.annotation, names);
        }
    } else if let Some(children) = value.operator_children() {
        for child in children {
            rest_parameters(child, names);
        }
    }
}

impl ModuleChecker<'_> {
    pub(super) fn check_condition_scope(&mut self, value: &Type, span: &SourceSpan) -> bool {
        if let Type::Mapped(mapped) = value {
            self.with_lexical_type_scope(std::slice::from_ref(&mapped.parameter), |checker| {
                if let Some(name) = &mapped.name_type {
                    checker.check_type(name, span);
                }
                checker.check_type(&mapped.value, span);
            });
            return true;
        }
        let Type::Conditional(conditional) = value else {
            return false;
        };
        self.check_type(&conditional.check, span);
        self.check_type(&conditional.extends, span);
        let subject = canonical_parameter_references(&conditional.check, &self.types);
        let mut inferred = conditional.extends.infer_parameters();
        // Repeated infer occurrences share one binder (for example both
        // holes in `${infer U}-${infer U}`), unlike duplicate declarations.
        let mut names = BTreeSet::new();
        inferred.retain(|parameter| names.insert(parameter.name.clone()));
        let mut tails = BTreeSet::new();
        rest_parameters(&conditional.extends, &mut tails);
        for parameter in &mut inferred {
            if parameter.constraint.is_none() && tails.contains(&parameter.name) {
                parameter.constraint = Some(Type::Array(Box::new(Type::StrictUnknown)));
            }
        }
        let refine = inferred.is_empty();
        self.with_lexical_type_scope(&inferred, |checker| {
            if refine {
                if let Type::Named { name, arguments } = &subject {
                    if arguments.is_empty() {
                        let expected =
                            canonical_parameter_references(&conditional.extends, &checker.types);
                        if let Some(definition) = checker
                            .types
                            .get_mut(name)
                            .filter(|definition| definition.kind == TypeDefinitionKind::Parameter)
                        {
                            definition.value =
                                Type::Intersection(vec![definition.value.clone(), expected]);
                        }
                    }
                }
            }
            checker.check_type(&conditional.when_true, span);
        });
        self.check_type(&conditional.when_false, span);
        true
    }
}
