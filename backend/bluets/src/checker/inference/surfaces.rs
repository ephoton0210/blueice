// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Infer declared protocol members from the same property lookup as checking.
use super::*;

impl collect::Collector<'_> {
    pub(super) fn collect_parameters(
        &mut self,
        templates: &[Parameter],
        actuals: &[Parameter],
        contra: bool,
    ) {
        let actuals: Vec<_> = actuals.iter().filter(|p| p.name != "this").collect();
        let mut index = 0;
        for template in templates {
            let Some(annotation) = &template.annotation else {
                continue;
            };
            if template.name == "this" {
                continue;
            }
            if template.rest
                && matches!(annotation, Type::Named {name, arguments} if arguments.is_empty() && self.parameters.contains(name))
            {
                let values = actuals[index..]
                    .iter()
                    .map(|p| TupleTypeElement {
                        annotation: p.annotation.clone().unwrap_or(Type::Unknown),
                        optional: p.optional,
                        rest: p.rest,
                        label: None,
                    })
                    .collect();
                self.collect(annotation, &Type::Tuple(values), !contra);
                break;
            }
            let Some(actual) = actuals.get(index) else {
                break;
            };
            if let Some(actual) = &actual.annotation {
                self.collect(annotation, actual, !contra);
            }
            index += 1;
        }
    }

    pub(super) fn collect_surface(&mut self, template: &Type, actual: &Type, contra: bool) -> bool {
        if let Type::Record(fields) = template {
            // A literal discriminant rules out the wrong union branch before
            // its unrelated payload can contribute generic evidence.
            fn fixed(value: &Type) -> bool {
                match value {
                    Type::Literal(_) | Type::Null | Type::Undefined | Type::UniqueSymbol(_) => true,
                    Type::Union(values) => values.iter().all(fixed),
                    _ => false,
                }
            }
            for field in fields.iter().filter(|field| fixed(&field.value)) {
                if let PropertyType::Found { value, .. } = property_type(
                    actual,
                    &field.name,
                    self.aliases,
                    &mut HashSet::new(),
                    self.budget,
                ) {
                    let expected = if field.optional {
                        Type::Union(vec![field.value.clone(), Type::Undefined])
                    } else {
                        field.value.clone()
                    };
                    if !is_assignable(
                        &value,
                        &expected,
                        self.aliases,
                        &mut HashSet::new(),
                        self.budget,
                    ) {
                        return true;
                    }
                }
            }
            for field in fields {
                if let PropertyType::Found { value, .. } = property_type(
                    actual,
                    &field.name,
                    self.aliases,
                    &mut HashSet::new(),
                    self.budget,
                ) {
                    self.collect(&field.value, &value, contra);
                }
            }
            return true;
        }
        if let (Type::Predicate(template), Type::Predicate(actual)) = (template, actual) {
            if let (Some(template), Some(actual)) = (&template.target, &actual.target) {
                self.collect(template, actual, contra);
            }
            return true;
        }
        if let (
            Type::CallableRecord {
                signatures: templates,
                ..
            },
            Type::CallableRecord {
                signatures: actuals,
                ..
            },
        ) = (template, actual)
        {
            // Like conditional function inference, the last overload supplies
            // the broadest inference signature; selection remains separate.
            for template in templates {
                if let Some(actual) = actuals
                    .iter()
                    .rev()
                    .find(|s| s.construct == template.construct)
                {
                    if actuals
                        .iter()
                        .filter(|s| s.construct == template.construct)
                        .count()
                        > 1
                    {
                        self.collect(&template.result, &actual.result, contra);
                        continue;
                    }
                    let template = Type::Function {
                        parameters: template.parameters.clone(),
                        result: Box::new(template.result.clone()),
                    };
                    let actual = Type::Function {
                        parameters: actual.parameters.clone(),
                        result: Box::new(actual.result.clone()),
                    };
                    self.collect(&template, &actual, contra);
                }
            }
            return true;
        }
        false
    }
}
