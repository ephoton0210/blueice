// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain rejected decorator arguments using their bound structural types.
use super::*;
use crate::diagnostic::{type_text, TypeScriptRelatedInformation};
mod constructors;

fn line(depth: usize, text: impl AsRef<str>) -> String {
    format!("\n{}{}", "  ".repeat(depth), text.as_ref())
}

impl ModuleChecker<'_> {
    pub(super) fn decorator_relation_detail(
        &self,
        actual: &Type,
        expected: &Type,
    ) -> (String, Vec<TypeScriptRelatedInformation>) {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let mut related = Vec::new();
        let detail = self.decorator_reason(actual, expected, 1, &mut budget, &mut related);
        (detail, related)
    }

    fn relation_matches(
        &self,
        actual: &Type,
        expected: &Type,
        budget: &mut TypeExpansionBudget,
    ) -> bool {
        is_assignable(actual, expected, &self.types, &mut HashSet::new(), budget)
    }

    fn relation_expanded(&self, value: &Type, budget: &mut TypeExpansionBudget) -> Type {
        let value = type_operators::expanded(value, &self.types, &mut HashSet::new(), budget);
        if let Type::Function { parameters, result } = value {
            let signature = self.canonical_rest_signature(FunctionSignature {
                parameters,
                return_type: *result,
                type_parameters: Vec::new(),
            });
            Type::Function {
                parameters: signature.parameters,
                result: Box::new(signature.return_type),
            }
        } else {
            value
        }
    }

    fn relation_display(&self, value: &Type, budget: &mut TypeExpansionBudget) -> String {
        let mut value = value.clone();
        let mut visited = HashSet::new();
        while matches!(&value, Type::Named {name, ..} if self.types.get(name).is_some_and(|definition| definition.kind == TypeDefinitionKind::Alias))
        {
            let Some(next) = instantiate_named(
                &value,
                &self.types,
                &mut visited,
                budget,
                "diagnostic alias",
            ) else {
                break;
            };
            value = next;
        }
        type_text::render_in(&value, self.project)
    }

    fn decorator_reason(
        &self,
        actual: &Type,
        expected: &Type,
        depth: usize,
        budget: &mut TypeExpansionBudget,
        related: &mut Vec<TypeScriptRelatedInformation>,
    ) -> String {
        if depth > 16 || !budget.consume() {
            return String::new();
        }
        let parameter = |value: &Type| {
            let value = canonical_parameter_references(value, &self.types);
            let Type::Named { name, .. } = value else {
                return None;
            };
            self.types
                .get(&name)
                .filter(|definition| definition.kind == TypeDefinitionKind::Parameter)
                .map(|definition| (name, definition.value.clone()))
        };
        if let Some((name, constraint)) = parameter(expected) {
            if !self.relation_matches(actual, &constraint, budget) {
                return line(depth, format!("'{}' could be instantiated with an arbitrary type which could be unrelated to '{}'.", crate::parser::source_type_name(&name), type_text::render_in(actual, self.project)));
            }
        }
        if let Some((_, constraint)) = parameter(actual) {
            if !self.relation_matches(&constraint, expected, budget) {
                return line(
                    depth,
                    format!(
                        "Type '{}' is not assignable to type '{}'.",
                        type_text::render_in(&constraint, self.project),
                        type_text::render_in(expected, self.project)
                    ),
                );
            }
        }
        let a = self.relation_expanded(actual, budget);
        let e = self.relation_expanded(expected, budget);
        if let Type::Union(parts) = &a {
            if let Some(part) = parts
                .iter()
                .find(|part| !self.relation_matches(part, expected, budget))
            {
                let target = if matches!(part, Type::Intersection(_)) {
                    expected.clone()
                } else if let Type::Union(parts) = &e {
                    type_operators::union(
                        parts
                            .iter()
                            .filter(|part| !matches!(part, Type::Undefined | Type::Null))
                            .cloned(),
                    )
                } else {
                    expected.clone()
                };
                return line(
                    depth,
                    format!(
                        "Type '{}' is not assignable to type '{}'.",
                        type_text::render_in(part, self.project),
                        type_text::render_in(&target, self.project)
                    ),
                ) + &self.decorator_reason(part, &target, depth + 1, budget, related);
            }
        }
        if let Type::Union(parts) = &e {
            if let Some(part) = parts
                .iter()
                .find(|part| !matches!(part, Type::Undefined | Type::Null))
            {
                return line(
                    depth,
                    format!(
                        "Type '{}' is not assignable to type '{}'.",
                        type_text::render_in(actual, self.project),
                        type_text::render_in(part, self.project)
                    ),
                ) + &self.decorator_reason(actual, part, depth + 1, budget, related);
            }
        }
        if let Type::Intersection(parts) = &a {
            if matches!(e, Type::Function { .. }) {
                if let Some(part) = parts.iter().find(|part| {
                    matches!(self.relation_expanded(part, budget), Type::Function { .. })
                }) {
                    return self.decorator_reason(part, expected, depth, budget, related);
                }
            }
        }
        if let (
            Type::CallableRecord { signatures: ac, .. },
            Type::CallableRecord { signatures: ec, .. },
        ) = (&a, &e)
        {
            if let (Some(source), Some(target)) = (
                ac.iter().find(|signature| signature.construct),
                ec.iter().find(|signature| signature.construct),
            ) {
                if source.abstract_constructor && !target.abstract_constructor {
                    return line(depth, "Cannot assign an abstract constructor type to a non-abstract constructor type.");
                }
                for (ap, ep) in source.type_parameters.iter().zip(&target.type_parameters) {
                    let av = ap.constraint.as_ref().unwrap_or(&Type::StrictUnknown);
                    let ev = ep.constraint.as_ref().unwrap_or(&Type::StrictUnknown);
                    if self.relation_matches(ev, av, budget) {
                        continue;
                    }
                    let signature = |signature: &crate::parser::TypeSignature| {
                        let binders = signature
                            .type_parameters
                            .iter()
                            .map(|parameter| {
                                let mut name =
                                    crate::parser::source_type_name(&parameter.name).to_string();
                                if let Some(value) = &parameter.constraint {
                                    name += &format!(
                                        " extends {}",
                                        type_text::render_in(value, self.project)
                                    );
                                }
                                name
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!(
                            "new <{binders}>({}) => {}",
                            type_text::parameter_list_in(&signature.parameters, Some(self.project)),
                            type_text::render_in(&signature.result, self.project)
                        )
                    };
                    let sp = source.parameters.first();
                    let tp = target.parameters.first();
                    return line(depth, "Types of construct signatures are incompatible.")
                        + &line(
                            depth + 1,
                            format!(
                                "Type '{}' is not assignable to type '{}'.",
                                signature(source),
                                signature(target)
                            ),
                        )
                        + &line(
                            depth + 2,
                            format!(
                                "Types of parameters '{}' and '{}' are incompatible.",
                                sp.map_or("", |p| p.name.as_str()),
                                tp.map_or("", |p| p.name.as_str())
                            ),
                        )
                        + &line(
                            depth + 3,
                            format!(
                                "Type '{}' is not assignable to type '{}'.",
                                crate::parser::source_type_name(&ep.name),
                                type_text::render_in(av, self.project)
                            ),
                        )
                        + &line(
                            depth + 4,
                            format!(
                                "Type '{}' is not assignable to type '{}'.",
                                type_text::render_in(ev, self.project),
                                type_text::render_in(av, self.project)
                            ),
                        );
                }
                if let Some(detail) =
                    self.constructor_return_reason(source, target, depth, budget, related)
                {
                    if let Type::CallableRecord { fields, .. } = &e {
                        if fields.iter().any(|field| field.name == "prototype") {
                            return line(depth, "Types of construct signatures are incompatible.")
                                + &line(
                                    depth + 1,
                                    format!(
                                        "Type '{}' is not assignable to type '{}'.",
                                        constructors::signature_text(source, self.project),
                                        constructors::signature_text(target, self.project)
                                    ),
                                )
                                + &self
                                    .constructor_return_reason(
                                        source,
                                        target,
                                        depth + 2,
                                        budget,
                                        related,
                                    )
                                    .unwrap_or_default();
                        }
                    }
                    return detail;
                }
                if source.type_parameters.is_empty() && target.type_parameters.is_empty() {
                    if let (
                        PropertyType::Found { value: source, .. },
                        PropertyType::Found { value: target, .. },
                    ) = (
                        property_type(&a, "prototype", &self.types, &mut HashSet::new(), budget),
                        property_type(&e, "prototype", &self.types, &mut HashSet::new(), budget),
                    ) {
                        if !self.relation_matches(&source, &target, budget) {
                            return self.decorator_reason(&source, &target, depth, budget, related);
                        }
                    }
                }
            }
        }
        if let (
            Type::Named {
                name: an,
                arguments: aa,
            },
            Type::Named {
                name: en,
                arguments: ea,
            },
        ) = (actual, expected)
        {
            if an == en && (an.ends_with("DecoratorTarget") || an.ends_with("DecoratorResult")) {
                for (source, target) in aa.iter().zip(ea).rev() {
                    if self.relation_matches(source, target, budget) {
                        continue;
                    }
                    let previous = related.len();
                    let nested = self.decorator_reason(source, target, depth + 1, budget, related);
                    if nested.contains(" is missing ") {
                        related.truncate(previous);
                        return self.decorator_reason(source, target, depth, budget, related);
                    }
                    return line(
                        depth,
                        format!(
                            "Type '{}' is not assignable to type '{}'.",
                            type_text::render_in(source, self.project),
                            type_text::render_in(target, self.project)
                        ),
                    ) + &nested;
                }
            }
        }
        if let Type::Record(fields) | Type::CallableRecord { fields, .. } = &e {
            let missing = fields
                .iter()
                .filter(|field| {
                    !field.optional
                        && matches!(
                            property_type(
                                &a,
                                &field.name,
                                &self.types,
                                &mut HashSet::new(),
                                budget
                            ),
                            PropertyType::Missing
                        )
                })
                .collect::<Vec<_>>();
            if missing.len() > 1 {
                return line(
                    depth,
                    format!(
                        "Type '{}' is missing the following properties from type '{}': {}",
                        self.relation_display(actual, budget),
                        self.relation_display(expected, budget),
                        missing
                            .iter()
                            .map(|field| field.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
            for field in fields {
                let found =
                    property_type(&a, &field.name, &self.types, &mut HashSet::new(), budget);
                if matches!(found, PropertyType::Missing) && !field.optional {
                    let span = self
                        .project
                        .modules
                        .get(&field.span.module)
                        .and_then(|module| {
                            module.declarations.iter().find_map(|declaration| {
                                let Declaration::Class(class) = declaration else {
                                    return None;
                                };
                                class
                                    .members
                                    .iter()
                                    .find(|member| member.span == field.span)
                                    .and_then(|member| member.key.first())
                                    .map(|token| {
                                        SourceSpan::new(&field.span.module, token.start, token.end)
                                    })
                            })
                        })
                        .unwrap_or_else(|| field.span.clone());
                    related.push(TypeScriptRelatedInformation {
                        code: 2728,
                        message: format!("'{}' is declared here.", field.name),
                        span,
                        position: None,
                    });
                    return line(
                        depth,
                        format!(
                            "Property '{}' is missing in type '{}' but required in type '{}'.",
                            field.name,
                            self.relation_display(actual, budget),
                            self.relation_display(expected, budget)
                        ),
                    );
                }
                let PropertyType::Found { value, .. } = found else {
                    continue;
                };
                let target = if field.optional {
                    type_operators::union([field.value.clone(), Type::Undefined])
                } else {
                    field.value.clone()
                };
                if self.relation_matches(&value, &target, budget) {
                    continue;
                }
                let source_function = self.relation_expanded(&value, budget);
                let target_function = self.relation_expanded(&field.value, budget);
                if let (
                    Type::Function {
                        parameters: ap,
                        result: ar,
                    },
                    Type::Function {
                        parameters: ep,
                        result: er,
                    },
                ) = (&source_function, &target_function)
                {
                    if ap
                        .iter()
                        .filter(|p| p.name != "this")
                        .map(|p| &p.annotation)
                        .eq(ep
                            .iter()
                            .filter(|p| p.name != "this")
                            .map(|p| &p.annotation))
                        && **er != Type::Void
                        && !self.relation_matches(ar, er, budget)
                    {
                        return line(depth, format!("The types returned by '{}()' are incompatible between these types.", field.name))
                            + &line(depth + 1, format!("Type '{}' is not assignable to type '{}'.", type_text::render_in(ar, self.project), type_text::render_in(er, self.project)))
                            + &self.decorator_reason(ar, er, depth + 2, budget, related);
                    }
                }
                if let Some((path, source, target)) =
                    self.decorator_member_mismatch(&value, &target, &field.name, budget, 0)
                {
                    return line(
                        depth,
                        format!("The types of '{path}' are incompatible between these types."),
                    ) + &line(
                        depth + 1,
                        format!(
                            "Type '{}' is not assignable to type '{}'.",
                            type_text::render_in(&source, self.project),
                            type_text::render_in(&target, self.project)
                        ),
                    ) + &self.decorator_reason(
                        &source,
                        &target,
                        depth + 2,
                        budget,
                        related,
                    );
                }
                return line(
                    depth,
                    format!("Types of property '{}' are incompatible.", field.name),
                ) + &line(
                    depth + 1,
                    format!(
                        "Type '{}' is not assignable to type '{}'.",
                        type_text::render_in(&value, self.project),
                        type_text::render_in(&target, self.project)
                    ),
                ) + &self.decorator_reason(&value, &target, depth + 2, budget, related);
            }
        }
        if let (
            Type::Function {
                parameters: ap,
                result: ar,
            },
            Type::Function {
                parameters: ep,
                result: er,
            },
        ) = (&a, &e)
        {
            let both_receivers =
                ap.iter().any(|p| p.name == "this") && ep.iter().any(|p| p.name == "this");
            for (source, target) in ap
                .iter()
                .filter(|p| both_receivers || p.name != "this")
                .zip(ep.iter().filter(|p| both_receivers || p.name != "this"))
            {
                let av = source.annotation.as_ref().unwrap_or(&Type::Any);
                let ev = target.annotation.as_ref().unwrap_or(&Type::Any);
                if self.relation_matches(ev, av, budget) {
                    continue;
                }
                let mut detail = line(
                    depth,
                    format!(
                        "Types of parameters '{}' and '{}' are incompatible.",
                        source.name, target.name
                    ),
                );
                let nested = self.decorator_reason(ev, av, depth + 1, budget, related);
                if nested.contains(" is missing ") {
                    return detail + &nested;
                }
                detail += &line(
                    depth + 1,
                    format!(
                        "Type '{}' is not assignable to type '{}'.",
                        type_text::render_in(ev, self.project),
                        type_text::render_in(av, self.project)
                    ),
                );
                return detail + &nested;
            }
            if **er != Type::Void && !self.relation_matches(ar, er, budget) {
                return line(
                    depth,
                    format!(
                        "Type '{}' is not assignable to type '{}'.",
                        type_text::render_in(ar, self.project),
                        type_text::render_in(er, self.project)
                    ),
                );
            }
        }
        type_text::detail(actual, expected, self.project)
            .replace('\n', &format!("\n{}", "  ".repeat(depth - 1)))
    }

    fn decorator_member_mismatch(
        &self,
        actual: &Type,
        expected: &Type,
        path: &str,
        budget: &mut TypeExpansionBudget,
        depth: usize,
    ) -> Option<(String, Type, Type)> {
        if depth > 16 || !budget.consume() {
            return None;
        }
        let a = self.relation_expanded(actual, budget);
        let e = self.relation_expanded(expected, budget);
        if matches!((&a, &e), (Type::Function { .. }, Type::Function { .. })) {
            return Some((path.into(), a, e));
        }
        let Type::Record(fields) = &e else {
            return None;
        };
        for field in fields {
            let PropertyType::Found { value, .. } =
                property_type(&a, &field.name, &self.types, &mut HashSet::new(), budget)
            else {
                continue;
            };
            if !self.relation_matches(&value, &field.value, budget) {
                return self.decorator_member_mismatch(
                    &value,
                    &field.value,
                    &format!("{path}.{}", field.name),
                    budget,
                    depth + 1,
                );
            }
        }
        None
    }
}
