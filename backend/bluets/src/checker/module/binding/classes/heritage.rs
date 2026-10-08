// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded inheritance and declaration-site specialization of class surfaces.

use super::*;

pub(in crate::checker) fn heritage_substitutions(
    parameters: &[TypeParameter],
    arguments: &[Type],
) -> BTreeMap<String, Type> {
    complete_type_arguments(parameters, arguments)
        .unwrap_or_else(|| vec![Type::Any; parameters.len()])
        .into_iter()
        .zip(parameters)
        .map(|(value, parameter)| (parameter.name.clone(), value))
        .collect()
}

pub(in crate::checker) fn specialize_constructor(
    signature: &FunctionSignature,
    substitutions: &BTreeMap<String, Type>,
    class_name: &str,
    parameters: &[TypeParameter],
) -> FunctionSignature {
    let substitutions = substitutions
        .iter()
        .flat_map(|(name, value)| {
            [
                (name.clone(), value.clone()),
                (
                    crate::parser::source_type_name(name).to_string(),
                    value.clone(),
                ),
            ]
        })
        .collect();
    FunctionSignature {
        parameters: signature
            .parameters
            .iter()
            .map(|parameter| Parameter {
                annotation: parameter
                    .annotation
                    .as_ref()
                    .map(|value| substitute_type(value, &substitutions)),
                ..parameter.clone()
            })
            .collect(),
        type_parameters: parameters.to_vec(),
        return_type: Type::Named {
            name: class_name.into(),
            arguments: parameters
                .iter()
                .map(|parameter| Type::Named {
                    name: parameter.name.clone(),
                    arguments: Vec::new(),
                })
                .collect(),
        },
    }
}

impl ModuleChecker<'_> {
    /// Resolve a constructor alias for checking while emission keeps the
    /// original runtime base expression and its lexical lookup.
    pub(in crate::checker::module::binding) fn class_with_resolved_heritage(
        &self,
        original: &ClassDeclaration,
    ) -> ClassDeclaration {
        let mut class = original.clone();
        if let Some(signature) = class
            .extends_name
            .as_ref()
            .and_then(|name| self.class_constructors.get(name))
            .and_then(|binding| binding.signatures.first())
        {
            let Type::Named { name, arguments } = &signature.return_type else {
                return class;
            };
            if class.extends_name.as_ref() != Some(name) {
                let capture_count = self.types.get(name).map_or(0, |base| {
                    base.parameters
                        .len()
                        .saturating_sub(signature.type_parameters.len())
                });
                class.extends_arguments = arguments
                    .iter()
                    .take(capture_count)
                    .cloned()
                    .chain(class.extends_arguments)
                    .collect();
                class.extends_name = Some(name.clone());
            }
        }
        class
    }

    pub(in crate::checker::module::binding) fn validate_class_heritage_arguments(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let (Some(name), Some(span)) = (&class.extends_name, &class.extends_type_span) else {
            return;
        };
        if self.class_constructors.contains_key(name) && name != &class.name {
            self.check_type(
                &Type::Named {
                    name: name.clone(),
                    arguments: class.extends_arguments.clone(),
                },
                span,
            );
        }
    }

    pub(super) fn specialized_base_surface(
        &self,
        class: &ClassDeclaration,
        is_static: bool,
    ) -> Option<Type> {
        let resolved = self.class_with_resolved_heritage(class);
        let class = &resolved;
        let name = class.extends_name.as_ref()?;
        if is_static {
            return self.values.get(name).cloned();
        }
        let definition = self.types.get(name)?;
        let arguments = class
            .extends_arguments
            .iter()
            .map(|value| formal_class_type(class, value))
            .collect::<Vec<_>>();
        let substitutions = heritage_substitutions(&definition.parameters, &arguments);
        Some(substitute_type(&definition.value, &substitutions))
    }

    pub(in crate::checker::module::binding) fn bind_inherited_class_instance_methods(&mut self) {
        let classes = self.module.classes().cloned().collect::<Vec<_>>();
        let local_classes = classes
            .iter()
            .map(|class| (class.name.clone(), self.class_with_inferred_returns(class)))
            .collect::<BTreeMap<_, _>>();
        let mut surfaces = Vec::new();
        for class in local_classes.values() {
            if !self
                .types
                .get(&class.name)
                .is_some_and(|definition| definition.kind == TypeDefinitionKind::Class)
            {
                continue;
            }
            let Type::Record(mut fields) = class_instance_type(class) else {
                unreachable!()
            };
            let mut names = fields
                .iter()
                .map(|field| field.name.clone())
                .collect::<BTreeSet<_>>();
            let mut visited = BTreeSet::new();
            let mut base_name = class.extends_name.as_deref();
            let mut arguments = class
                .extends_arguments
                .iter()
                .map(|value| formal_class_type(class, value))
                .collect::<Vec<_>>();
            for _ in 0..self.max_type_expansions {
                let Some(name) = base_name else {
                    break;
                };
                if !visited.insert(name) {
                    break;
                }
                let inherited = if let Some(base) = local_classes.get(name) {
                    let substitutions =
                        heritage_substitutions(&class_definition_parameters(base), &arguments);
                    arguments = base
                        .extends_arguments
                        .iter()
                        .map(|value| {
                            substitute_type(&formal_class_type(base, value), &substitutions)
                        })
                        .collect();
                    base_name = base.extends_name.as_deref();
                    substitute_type(&class_instance_type(base), &substitutions)
                } else if let Some(definition) = self
                    .types
                    .get(name)
                    .filter(|definition| definition.kind == TypeDefinitionKind::Class)
                {
                    let Type::Record(fields) = definition.value.object_type() else {
                        break;
                    };
                    base_name = None;
                    substitute_type(
                        &Type::Record(fields.clone()),
                        &heritage_substitutions(&definition.parameters, &arguments),
                    )
                } else {
                    break;
                };
                let Type::Record(inherited) = inherited else {
                    unreachable!("field specialization retains record shape");
                };
                fields.extend(
                    inherited
                        .iter()
                        .filter(|field| !names.contains(&field.name))
                        .cloned(),
                );
                names.extend(inherited.iter().map(|field| field.name.clone()));
            }
            surfaces.push((class.name.clone(), Type::Record(fields)));
        }
        for (name, value) in surfaces {
            if let Some(definition) = self.types.get_mut(&name) {
                definition.value = value;
            }
        }
    }

    pub(in crate::checker::module::binding) fn bind_inherited_class_static_methods(&mut self) {
        let classes = self.module.classes().cloned().collect::<Vec<_>>();
        let local_classes = classes
            .iter()
            .map(|class| (class.name.clone(), self.class_with_inferred_returns(class)))
            .collect::<BTreeMap<_, _>>();
        let mut surfaces = Vec::new();
        for class in local_classes.values() {
            if !self.class_constructors.contains_key(&class.name) {
                continue;
            }
            let Type::CallableRecord {
                mut fields,
                signatures,
            } = class_constructor_side_type(class)
            else {
                unreachable!("class constructor side retains construct signatures")
            };
            let mut names = fields
                .iter()
                .map(|field| field.name.clone())
                .collect::<BTreeSet<_>>();
            let mut visited = BTreeSet::new();
            let mut base_name = class.extends_name.as_deref();
            for _ in 0..self.max_type_expansions {
                let Some(name) = base_name else {
                    break;
                };
                if !visited.insert(name) {
                    break;
                }
                let inherited = if let Some(base) = local_classes.get(name) {
                    base_name = base.extends_name.as_deref();
                    class_constructor_side_type(base)
                } else if self.class_constructors.contains_key(name) {
                    base_name = None;
                    self.values.get(name).cloned().unwrap_or(Type::Unknown)
                } else {
                    break;
                };
                let (Type::Record(inherited)
                | Type::CallableRecord {
                    fields: inherited, ..
                }) = inherited.object_type()
                else {
                    break;
                };
                fields.extend(
                    inherited
                        .iter()
                        .filter(|field| !names.contains(&field.name))
                        .cloned(),
                );
                names.extend(inherited.iter().map(|field| field.name.clone()));
            }
            surfaces.push((
                class.name.clone(),
                Type::CallableRecord { fields, signatures },
            ));
        }
        for (name, value) in surfaces {
            self.values.insert(name, value);
        }
    }

    pub(in crate::checker::module::binding) fn bind_inherited_class_constructors(&mut self) {
        let classes = self
            .module
            .classes()
            .map(|class| self.class_with_resolved_heritage(class))
            .collect::<Vec<_>>();
        for class in &classes {
            let Some(base_name) = class.extends_name.as_deref() else {
                continue;
            };
            if class_declares_constructor(class) {
                continue;
            }
            let Some(base) = self.class_constructors.get(base_name).cloned() else {
                continue;
            };
            if base.inherited {
                continue;
            }
            let substitutions = heritage_substitutions(
                &self
                    .types
                    .get(base_name)
                    .map(|definition| definition.parameters.clone())
                    .unwrap_or_default(),
                &class.extends_arguments,
            );
            let signatures = base
                .signatures
                .into_iter()
                .map(|signature| {
                    let mut specialized = specialize_constructor(
                        &signature,
                        &substitutions,
                        &class.name,
                        &class.type_parameters,
                    );
                    specialized.return_type = class.this_type();
                    specialized
                })
                .collect();
            self.class_constructors.insert(
                class.name.clone(),
                ClassConstructorBinding {
                    modifiers: ClassModifierSurface::declared(class),
                    signatures,
                    inherited: false,
                    visibility: base.visibility,
                },
            );
        }
    }
}
