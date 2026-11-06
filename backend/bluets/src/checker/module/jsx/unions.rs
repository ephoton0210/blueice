// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Discriminant attributes retain the relationship between union prop fields.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn jsx_union_parts(&self, value: &Type) -> Option<Vec<Type>> {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let value = crate::checker::type_operators::expanded(
            value,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if budget.exhausted {
            return None;
        }
        match value {
            Type::Union(parts) => Some(parts),
            _ => None,
        }
    }

    pub(super) fn jsx_discriminant_target(
        &self,
        value: &Type,
        element: &JsxElement,
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let parts = self.jsx_union_parts(value)?;
        let mut selected = Vec::new();
        for part in parts {
            let fields = self.expanded_record_fields(part.clone()).0?;
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            budget.checking = self.checking;
            let fits = fields.iter().filter(|field| matches!(field.value, Type::Literal(_))).all(|field| {
                let Some(JsxAttribute::Named { value, .. }) = element.attributes.iter().find(|attribute| {
                    matches!(attribute, JsxAttribute::Named { name, .. } if name.text == field.name)
                }) else { return true; };
                let actual = match value {
                    Some(JsxValue::String { start, end }) => Type::Literal(self.module.source[*start..*end].to_string()),
                    Some(JsxValue::Expression { tokens, .. }) => self.infer_in_context(tokens, scope, &field.value),
                    None => Type::Literal("true".into()),
                    _ => return true,
                };
                is_assignable(&actual, &field.value, &self.types, &mut HashSet::new(), &mut budget)
            });
            if budget.exhausted {
                return None;
            }
            if fits {
                selected.push(part);
            }
        }
        (selected.len() == 1).then(|| selected.remove(0))
    }

    pub(super) fn jsx_narrow_props(
        &self,
        props: Props,
        element: &JsxElement,
        scope: &BTreeMap<String, Type>,
    ) -> Props {
        let Props::Known {
            fields,
            value_based,
            value,
        } = props
        else {
            return props;
        };
        let Some(selected) = self.jsx_discriminant_target(&value, element, scope) else {
            return Props::Known {
                fields,
                value_based,
                value,
            };
        };
        let Some(mut narrowed) = self.expanded_record_fields(selected).0 else {
            return Props::Known {
                fields,
                value_based,
                value,
            };
        };
        if value_based {
            if let Some(intrinsic) = self.jsx_interface_fields("IntrinsicAttributes") {
                for field in intrinsic {
                    if !narrowed.iter().any(|existing| existing.name == field.name) {
                        narrowed.push(field);
                    }
                }
            }
        }
        Props::Known {
            fields: narrowed,
            value_based,
            value,
        }
    }
}
