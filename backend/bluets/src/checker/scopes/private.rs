// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private symbols belong to the lexical class, including on derived receivers.

use super::*;
use crate::parser::ClassDeclaration;

pub(super) struct PrivateSlot {
    pub(super) value: Type,
    pub(super) readonly: bool,
    pub(super) method: bool,
    is_static: bool,
}

pub(super) struct PrivateClass {
    marker: String,
    name: String,
    slots: BTreeMap<String, PrivateSlot>,
}

impl ScopeModel<'_> {
    pub(super) fn private_class(&mut self, class: &ClassDeclaration, scope: ScopeId) {
        self.scopes[scope].class_owner = Some(scope);
        let path = &self.scopes[scope].namespace_path;
        let name = if path.is_empty() {
            class.name.clone()
        } else {
            format!("{path}.{}", class.name)
        };
        let mut slots = BTreeMap::new();
        for member in &class.members {
            if let Some(field) = &member.field {
                if field.name.starts_with('#') {
                    slots.insert(
                        field.name.clone(),
                        PrivateSlot {
                            value: field.declared_type().unwrap_or(Type::Unknown),
                            readonly: field.readonly,
                            method: false,
                            is_static: field.is_static,
                        },
                    );
                }
            }
            if let Some(method) = &member.method {
                if method.name.starts_with('#') {
                    slots.insert(
                        method.name.clone(),
                        PrivateSlot {
                            value: walk::function_type(&method.parameters, &method.return_type),
                            readonly: true,
                            method: true,
                            is_static: method.is_static,
                        },
                    );
                }
            }
            if let Some(accessor) = &member.accessor {
                if accessor.name.starts_with('#') {
                    let setter = class
                        .members
                        .iter()
                        .filter_map(|member| member.accessor.as_ref())
                        .any(|other| other.name == accessor.name && !other.getter);
                    let value = accessor
                        .return_type
                        .clone()
                        .or_else(|| {
                            accessor
                                .parameters
                                .first()
                                .and_then(|parameter| parameter.annotation.clone())
                        })
                        .unwrap_or(Type::Unknown);
                    let slot = slots.entry(accessor.name.clone()).or_insert(PrivateSlot {
                        value: Type::Unknown,
                        readonly: !setter,
                        method: false,
                        is_static: accessor.is_static,
                    });
                    if slot.value == Type::Unknown {
                        slot.value = value;
                    }
                }
            }
        }
        self.private_classes.insert(
            scope,
            PrivateClass {
                marker: format!("{}@{}", class.name, class.span.module),
                name,
                slots,
            },
        );
    }

    pub(super) fn private_slot(&self, scope: ScopeId, property: &str) -> Option<&PrivateSlot> {
        self.private_classes
            .get(&self.scopes[scope].class_owner?)?
            .slots
            .get(property)
    }

    pub(super) fn private_marker(&self, scope: ScopeId) -> Option<&str> {
        self.private_classes
            .get(&self.scopes[scope].class_owner?)
            .map(|class| class.marker.as_str())
    }

    pub(super) fn private_slot_type(
        &self,
        scope: ScopeId,
        property: &str,
        budget: &mut TypeExpansionBudget,
    ) -> Option<Type> {
        let class = self.private_classes.get(&self.scopes[scope].class_owner?)?;
        let slot = class.slots.get(property)?;
        let owner = Type::Named {
            name: if slot.is_static {
                format!("typeof {}", class.name)
            } else {
                class.name.clone()
            },
            arguments: Vec::new(),
        };
        if let PropertyType::Found { value, .. } = property_type(
            &owner,
            property,
            &self.type_definitions,
            &mut HashSet::new(),
            budget,
        ) {
            return Some(value);
        }
        if let Some(value) = mutation_field_type(
            &owner,
            property,
            &self.type_definitions,
            &mut HashSet::new(),
            budget,
            Some(&class.marker),
            0,
        ) {
            return Some(value);
        }
        (!budget.exhausted).then(|| slot.value.clone())
    }
}
