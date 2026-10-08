// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Abstract obligations and explicit override policy follow class ownership.

use super::*;
use crate::parser::ClassMemberShell;

fn is_static(member: &ClassMemberShell) -> bool {
    member.method.as_ref().is_some_and(|item| item.is_static)
        || member.field.as_ref().is_some_and(|item| item.is_static)
        || member.accessor.as_ref().is_some_and(|item| item.is_static)
}

fn member_name_span(member: &ClassMemberShell) -> Option<&SourceSpan> {
    member
        .method
        .as_ref()
        .map(|item| &item.name_span)
        .or_else(|| member.field.as_ref().map(|item| &item.name_span))
        .or_else(|| member.accessor.as_ref().map(|item| &item.name_span))
}

impl ClassModifierSurface {
    pub(in crate::checker) fn declared(class: &ClassDeclaration) -> Self {
        let mut result = Self {
            abstract_class: class.abstract_modifier.is_some(),
            ..Self::default()
        };
        for member in &class.members {
            if is_static(member) || member_name_span(member).is_none() {
                continue;
            }
            if let Some(name) = &member.name {
                result.declared_members.insert(name.clone());
                if member.abstract_modifier.is_some() {
                    result.abstract_members.insert(name.clone());
                }
            }
        }
        result.declared_members.extend(
            class
                .parameter_property_fields()
                .into_iter()
                .map(|field| field.name),
        );
        result
    }

    pub(in crate::checker) fn inherit(&mut self, base: &Self) {
        let inherited = base
            .abstract_members
            .iter()
            .filter(|name| {
                !self.declared_members.contains(*name) || self.abstract_members.contains(*name)
            })
            .cloned()
            .collect::<Vec<_>>();
        self.abstract_members.extend(inherited);
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module::binding) fn bind_class_modifier_surfaces(&mut self) {
        let classes = self
            .module
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Class(class) => Some((class.name.clone(), class.clone())),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        for class in classes.values() {
            if self.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::ResourceLimit
                    && diagnostic.span == class.name_span
            }) {
                continue;
            }
            let mut chain = vec![ClassModifierSurface::declared(class)];
            let mut current = class.extends_name.as_deref();
            let mut visited = BTreeSet::new();
            while let Some(name) = current {
                if !visited.insert(name) {
                    break;
                }
                if chain.len() > self.max_type_expansions {
                    self.type_error(
                        &class.name_span,
                        "abstract heritage exceeds its expansion limit".into(),
                        DiagnosticCode::ResourceLimit,
                    );
                    break;
                }
                if let Some(base) = classes.get(name) {
                    chain.push(ClassModifierSurface::declared(base));
                    current = base.extends_name.as_deref();
                } else {
                    if let Some(base) = self.class_constructors.get(name) {
                        chain.push(base.modifiers.clone());
                    }
                    break;
                }
            }
            let mut inherited = chain.pop().unwrap();
            while let Some(mut own) = chain.pop() {
                own.inherit(&inherited);
                inherited = own;
            }
            if let Some(binding) = self.class_constructors.get_mut(&class.name) {
                binding.modifiers = inherited;
            }
        }
    }

    fn base_has_member(&self, base: &str, name: &str, static_side: bool) -> bool {
        let value = if static_side {
            self.values.get(base)
        } else {
            self.types.get(base).map(|definition| &definition.value)
        };
        matches!(value,Some(Type::Record(fields)|Type::CallableRecord{fields,..}) if visibility::effective_visibility(fields,name).is_some())
    }

    pub(in crate::checker::module::binding) fn validate_class_modifiers(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let Some(surface) = self
            .class_constructors
            .get(&class.name)
            .map(|binding| binding.modifiers.clone())
        else {
            return;
        };
        let base = class.extends_name.as_deref();
        let abstract_base = base
            .and_then(|name| self.class_constructors.get(name))
            .map(|binding| binding.modifiers.abstract_members.clone())
            .unwrap_or_default();
        if !surface.abstract_class && !surface.abstract_members.is_empty() {
            let parent = base.unwrap_or_default().to_string();
            let (code, args) = if surface.abstract_members.len() == 1 {
                (
                    2515,
                    vec![
                        class.name.clone(),
                        surface.abstract_members.iter().next().unwrap().clone(),
                        parent,
                    ],
                )
            } else {
                (
                    2654,
                    vec![
                        class.name.clone(),
                        parent,
                        surface
                            .abstract_members
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", "),
                    ],
                )
            };
            self.typescript_type_error(
                &class.name_span,
                "class has unimplemented abstract members".into(),
                DiagnosticCode::TypeMismatch,
                code,
                args,
            );
        }
        for member in &class.members {
            let (Some(name), Some(span)) = (&member.name, member_name_span(member)) else {
                continue;
            };
            let found =
                base.is_some_and(|base| self.base_has_member(base, name, is_static(member)));
            let needs_override = self.checking.no_implicit_override
                && found
                && (member.abstract_modifier.is_some() || !abstract_base.contains(name));
            self.check_member_override(
                &class.name,
                base,
                found,
                member.override_modifier.is_some(),
                needs_override.then_some(if abstract_base.contains(name) {
                    4116
                } else {
                    4114
                }),
                span,
            );
        }
        for constructor in class
            .members
            .iter()
            .filter_map(|member| member.constructor.as_ref())
        {
            for property in &constructor.parameter_properties {
                let parameter = &constructor.parameters[property.parameter_index];
                let found =
                    base.is_some_and(|base| self.base_has_member(base, &parameter.name, false));
                let needs_override = self.checking.no_implicit_override
                    && found
                    && !abstract_base.contains(&parameter.name);
                let span = SourceSpan::new(
                    &parameter.span.module,
                    property.modifiers_start,
                    parameter.span.end,
                );
                self.check_member_override(
                    &class.name,
                    base,
                    found,
                    property.override_modifier,
                    needs_override.then_some(4115),
                    &span,
                );
            }
        }
        for access in class.body.windows(3) {
            if access[0].is("super") && access[1].is(".") && abstract_base.contains(&access[2].text)
            {
                self.typescript_type_error(
                    &access[2].span(&class.span.module),
                    "abstract member has no super implementation".into(),
                    DiagnosticCode::TypeMismatch,
                    2513,
                    vec![access[2].text.clone(), base.unwrap_or_default().into()],
                );
            }
        }
    }

    fn check_member_override(
        &mut self,
        containing_class: &str,
        base: Option<&str>,
        found: bool,
        explicit: bool,
        required: Option<u32>,
        span: &SourceSpan,
    ) {
        let error = if explicit && base.is_none() {
            Some((4112, vec![containing_class.into()]))
        } else if explicit && !found {
            Some((4113, vec![base.unwrap_or_default().into()]))
        } else if !explicit {
            required.map(|code| (code, vec![base.unwrap_or_default().into()]))
        } else {
            None
        };
        if let Some((code, args)) = error {
            self.typescript_type_error(
                span,
                "class member override policy is not satisfied".into(),
                DiagnosticCode::TypeMismatch,
                code,
                args,
            );
        }
    }

    pub(super) fn reject_abstract_construction(
        &mut self,
        tokens: &[Token],
        callee: &str,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        let class = self.is_bound_class_constructor_value(callee, scope)
            && self
                .class_constructors
                .get(callee)
                .is_some_and(|binding| binding.modifiers.abstract_class);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        let mut value = scope.get(callee).cloned().unwrap_or(Type::Unknown);
        while let Some(expanded) = instantiate_named(
            &value,
            &self.types,
            &mut visited,
            &mut budget,
            "abstract constructor",
        ) {
            value = expanded;
        }
        let signature = matches!(&value,Type::CallableRecord{signatures,..} if signatures.iter().any(|signature|signature.construct && signature.abstract_constructor));
        if class || signature {
            let span =
                SourceSpan::new(&self.module.id, tokens[0].start, tokens.last().unwrap().end);
            self.typescript_type_error(
                &span,
                "cannot instantiate an abstract constructor".into(),
                DiagnosticCode::TypeMismatch,
                2511,
                Vec::new(),
            );
        }
        class || signature
    }
}
