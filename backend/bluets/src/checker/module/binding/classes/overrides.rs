// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compatibility checks for inherited class method declarations.

use super::*;
use crate::parser::{ClassMethodGroup, Parameter};

struct InheritedMethod<'a> {
    base_name: String,
    parameters: &'a [Parameter],
    return_type: Option<&'a Type>,
}

#[derive(Clone, Copy)]
enum RestShape {
    None,
    MatchingArrays,
    DerivedCoversFixed(usize),
    BaseCoversFixed(usize),
    ShiftedArrays {
        derived_fixed: usize,
        inherited_fixed: usize,
    },
    DerivedTuple {
        derived_fixed: usize,
        inherited_fixed: usize,
        inherited_array_rest: bool,
    },
    InheritedTuple {
        derived_fixed: usize,
        inherited_fixed: usize,
        derived_array_rest: bool,
    },
    BothTuples {
        derived_fixed: usize,
        inherited_fixed: usize,
    },
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn validate_class_method_overrides(
        &mut self,
        class: &ClassDeclaration,
    ) {
        for group in &class.method_groups {
            if !group.signature_member_indices.is_empty() {
                continue;
            }
            let Some(derived) = group
                .implementation_member_index
                .and_then(|index| class.members[index].method.as_ref())
            else {
                continue;
            };
            let Some(inherited) = nearest_inherited_method(
                &self.module.declarations,
                &self.types,
                &self.values,
                &self.class_constructors,
                class,
                group,
                self.max_type_expansions,
            ) else {
                continue;
            };
            // Align fixed positions and rest elements where the signatures
            // have a supported rest shape.
            let Some(rest_shape) = rest_shape(&derived.parameters, inherited.parameters) else {
                continue;
            };
            if derived.return_type.is_none()
                || inherited.return_type.is_none()
                || derived
                    .parameters
                    .iter()
                    .chain(inherited.parameters)
                    .any(|parameter| parameter.annotation.is_none())
            {
                continue;
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            // TypeScript compares class-method parameters bivariantly, then
            // requires the overriding result to fit the inherited result.
            // An override may omit inherited positions or add omittable ones.
            // With rests on both sides, TypeScript also permits more required
            // positions when aligned types agree, including tuple elements.
            let required_derived = derived
                .parameters
                .iter()
                .filter(|parameter| !parameter.optional && !parameter.rest)
                .count()
                + if matches!(
                    rest_shape,
                    RestShape::DerivedTuple { .. } | RestShape::BothTuples { .. }
                ) {
                    let Some(Type::Tuple(elements)) = derived
                        .parameters
                        .last()
                        .and_then(|parameter| parameter.annotation.as_ref())
                    else {
                        unreachable!("rest shape requires a tuple annotation")
                    };
                    elements.len()
                } else {
                    0
                };
            let inherited_arity = if let RestShape::InheritedTuple {
                inherited_fixed, ..
            }
            | RestShape::BothTuples {
                inherited_fixed, ..
            } = rest_shape
            {
                let Some(Type::Tuple(elements)) = inherited
                    .parameters
                    .last()
                    .and_then(|parameter| parameter.annotation.as_ref())
                else {
                    unreachable!("rest shape requires a tuple annotation")
                };
                inherited_fixed + elements.len()
            } else {
                inherited.parameters.len()
            };
            let arity_compatible = matches!(
                rest_shape,
                RestShape::MatchingArrays | RestShape::ShiftedArrays { .. }
            ) || matches!(
                rest_shape,
                RestShape::DerivedTuple {
                    inherited_array_rest: true,
                    ..
                }
            ) || required_derived <= inherited_arity;
            let parameters_compatible = arity_compatible
                && match rest_shape {
                    RestShape::None | RestShape::MatchingArrays => {
                        derived.parameters.iter().zip(inherited.parameters).all(
                            |(derived, base)| {
                                parameter_types_compatible(
                                    derived.annotation.as_ref().unwrap_or(&Type::Unknown),
                                    base.annotation.as_ref().unwrap_or(&Type::Unknown),
                                    &self.types,
                                    &mut budget,
                                )
                            },
                        )
                    }
                    RestShape::DerivedCoversFixed(prefix) => {
                        let fixed_compatible = derived.parameters[..prefix]
                            .iter()
                            .zip(inherited.parameters)
                            .all(|(derived, base)| {
                                parameter_types_compatible(
                                    derived.annotation.as_ref().unwrap_or(&Type::Unknown),
                                    base.annotation.as_ref().unwrap_or(&Type::Unknown),
                                    &self.types,
                                    &mut budget,
                                )
                            });
                        let Some(Type::Array(element)) = derived
                            .parameters
                            .last()
                            .and_then(|parameter| parameter.annotation.as_ref())
                        else {
                            unreachable!("rest shape requires an array annotation")
                        };
                        fixed_compatible
                            && inherited.parameters[prefix..].iter().all(|base| {
                                parameter_types_compatible(
                                    element,
                                    base.annotation.as_ref().unwrap_or(&Type::Unknown),
                                    &self.types,
                                    &mut budget,
                                )
                            })
                    }
                    RestShape::BaseCoversFixed(prefix) => {
                        let Some(Type::Array(element)) = inherited
                            .parameters
                            .last()
                            .and_then(|parameter| parameter.annotation.as_ref())
                        else {
                            unreachable!("rest shape requires an array annotation")
                        };
                        derived
                            .parameters
                            .iter()
                            .enumerate()
                            .all(|(index, derived)| {
                                let base_type = if index < prefix {
                                    inherited.parameters[index]
                                        .annotation
                                        .as_ref()
                                        .unwrap_or(&Type::Unknown)
                                } else {
                                    element
                                };
                                parameter_types_compatible(
                                    derived.annotation.as_ref().unwrap_or(&Type::Unknown),
                                    base_type,
                                    &self.types,
                                    &mut budget,
                                )
                            })
                    }
                    RestShape::ShiftedArrays {
                        derived_fixed,
                        inherited_fixed,
                    } => {
                        let Some(Type::Array(derived_element)) = derived
                            .parameters
                            .last()
                            .and_then(|parameter| parameter.annotation.as_ref())
                        else {
                            unreachable!("rest shape requires an array annotation")
                        };
                        let Some(Type::Array(inherited_element)) = inherited
                            .parameters
                            .last()
                            .and_then(|parameter| parameter.annotation.as_ref())
                        else {
                            unreachable!("rest shape requires an array annotation")
                        };
                        (0..derived_fixed.max(inherited_fixed)).all(|index| {
                            let derived_type = if index < derived_fixed {
                                derived.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                derived_element
                            };
                            let inherited_type = if index < inherited_fixed {
                                inherited.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                inherited_element
                            };
                            parameter_types_compatible(
                                derived_type,
                                inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        }) && parameter_types_compatible(
                            derived_element,
                            inherited_element,
                            &self.types,
                            &mut budget,
                        )
                    }
                    RestShape::DerivedTuple {
                        derived_fixed,
                        inherited_fixed,
                        inherited_array_rest,
                    } => {
                        let Some(Type::Tuple(elements)) = derived
                            .parameters
                            .last()
                            .and_then(|parameter| parameter.annotation.as_ref())
                        else {
                            unreachable!("rest shape requires a tuple annotation")
                        };
                        let inherited_element = if inherited_array_rest {
                            let Some(Type::Array(element)) = inherited
                                .parameters
                                .last()
                                .and_then(|parameter| parameter.annotation.as_ref())
                            else {
                                unreachable!("rest shape requires an array annotation")
                            };
                            Some(element.as_ref())
                        } else {
                            None
                        };
                        let derived_positions = derived_fixed + elements.len();
                        let comparison_positions = if inherited_array_rest {
                            derived_positions
                        } else {
                            derived_positions.min(inherited_fixed)
                        };
                        (0..comparison_positions).all(|index| {
                            let derived_type = if index < derived_fixed {
                                derived.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                &elements[index - derived_fixed].annotation
                            };
                            let inherited_type = if index < inherited_fixed {
                                inherited.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                inherited_element.expect("rest shape requires an array annotation")
                            };
                            parameter_types_compatible(
                                derived_type,
                                inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        })
                    }
                    RestShape::InheritedTuple {
                        derived_fixed,
                        inherited_fixed,
                        derived_array_rest,
                    } => {
                        let Some(Type::Tuple(elements)) = inherited
                            .parameters
                            .last()
                            .and_then(|parameter| parameter.annotation.as_ref())
                        else {
                            unreachable!("rest shape requires a tuple annotation")
                        };
                        let derived_element = if derived_array_rest {
                            let Some(Type::Array(element)) = derived
                                .parameters
                                .last()
                                .and_then(|parameter| parameter.annotation.as_ref())
                            else {
                                unreachable!("rest shape requires an array annotation")
                            };
                            Some(element.as_ref())
                        } else {
                            None
                        };
                        let inherited_positions = inherited_fixed + elements.len();
                        let comparison_positions = if derived_array_rest {
                            inherited_positions
                        } else {
                            derived_fixed.min(inherited_positions)
                        };
                        (0..comparison_positions).all(|index| {
                            let derived_type = if index < derived_fixed {
                                derived.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                derived_element.expect("rest shape requires an array annotation")
                            };
                            let inherited_type = if index < inherited_fixed {
                                inherited.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                &elements[index - inherited_fixed].annotation
                            };
                            parameter_types_compatible(
                                derived_type,
                                inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        })
                    }
                    RestShape::BothTuples {
                        derived_fixed,
                        inherited_fixed,
                    } => {
                        let Some(Type::Tuple(derived_elements)) = derived
                            .parameters
                            .last()
                            .and_then(|parameter| parameter.annotation.as_ref())
                        else {
                            unreachable!("rest shape requires a tuple annotation")
                        };
                        let Some(Type::Tuple(inherited_elements)) = inherited
                            .parameters
                            .last()
                            .and_then(|parameter| parameter.annotation.as_ref())
                        else {
                            unreachable!("rest shape requires a tuple annotation")
                        };
                        let positions = (derived_fixed + derived_elements.len())
                            .min(inherited_fixed + inherited_elements.len());
                        (0..positions).all(|index| {
                            let derived_type = if index < derived_fixed {
                                derived.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                &derived_elements[index - derived_fixed].annotation
                            };
                            let inherited_type = if index < inherited_fixed {
                                inherited.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                &inherited_elements[index - inherited_fixed].annotation
                            };
                            parameter_types_compatible(
                                derived_type,
                                inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        })
                    }
                };
            let compatible = parameters_compatible
                && is_assignable(
                    derived.return_type.as_ref().unwrap_or(&Type::Unknown),
                    inherited.return_type.unwrap_or(&Type::Unknown),
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                );
            if budget.exhausted {
                self.type_error(
                    &derived.span,
                    format!(
                        "class method override compatibility exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
            } else if !compatible {
                self.type_error(
                    &derived.span,
                    format!(
                        "class method `{}` is incompatible with inherited method from `{}`",
                        group.name, inherited.base_name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }
}

fn nearest_inherited_method<'a>(
    declarations: &'a [Declaration],
    types: &'a BTreeMap<String, TypeDefinition>,
    values: &'a BTreeMap<String, Type>,
    constructors: &BTreeMap<String, ClassConstructorBinding>,
    class: &ClassDeclaration,
    group: &ClassMethodGroup,
    max_edges: usize,
) -> Option<InheritedMethod<'a>> {
    let mut base_name = class.extends_name.clone();
    let mut visited = BTreeSet::new();
    for _ in 0..max_edges {
        let name = base_name?;
        if !visited.insert(name.clone()) {
            return None;
        }
        let base = declarations.iter().find_map(|declaration| {
            let Declaration::Class(base) = declaration else {
                return None;
            };
            (base.name == name && base.name_span.start < class.name_span.start).then_some(base)
        });
        if let Some(base) = base {
            if let Some(candidate) = base.method_groups.iter().find(|candidate| {
                candidate.name == group.name && candidate.is_static == group.is_static
            }) {
                if !candidate.signature_member_indices.is_empty() {
                    return None;
                }
                return candidate
                    .implementation_member_index
                    .and_then(|index| base.members[index].method.as_ref())
                    .map(|method| InheritedMethod {
                        base_name: base.name.clone(),
                        parameters: &method.parameters,
                        return_type: method.return_type.as_ref(),
                    });
            }
            base_name = base.extends_name.clone();
            continue;
        }
        // A forward local base has its own heritage diagnostic. Only a bound
        // value-imported class can provide a runtime base surface here.
        if declarations.iter().any(
            |declaration| matches!(declaration, Declaration::Class(local) if local.name == name),
        ) || !constructors.contains_key(&name)
        {
            return None;
        }
        let surface = if group.is_static {
            values.get(&name)
        } else {
            types.get(&name).and_then(|definition| {
                (definition.kind == TypeDefinitionKind::Class).then_some(&definition.value)
            })
        };
        let Type::Record(fields) = surface? else {
            return None;
        };
        let mut matching = fields.iter().filter(|field| field.name == group.name);
        let field = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        let Type::Function { parameters, result } = &field.value else {
            return None;
        };
        return Some(InheritedMethod {
            base_name: name,
            parameters,
            return_type: Some(result),
        });
    }
    None
}

fn rest_shape(derived: &[Parameter], inherited: &[Parameter]) -> Option<RestShape> {
    let derived_has_rest = derived.iter().any(|parameter| parameter.rest);
    let inherited_has_rest = inherited.iter().any(|parameter| parameter.rest);
    if !derived_has_rest && !inherited_has_rest {
        return Some(RestShape::None);
    }
    if !derived_has_rest {
        let (inherited_rest, inherited_fixed) = inherited.split_last()?;
        if !inherited_rest.rest || inherited_fixed.iter().any(|parameter| parameter.rest) {
            return None;
        }
        return match inherited_rest.annotation.as_ref() {
            Some(Type::Array(_)) => Some(RestShape::BaseCoversFixed(inherited_fixed.len())),
            Some(Type::Tuple(_)) => Some(RestShape::InheritedTuple {
                derived_fixed: derived.len(),
                inherited_fixed: inherited_fixed.len(),
                derived_array_rest: false,
            }),
            _ => None,
        };
    }
    let (derived_rest, derived_fixed) = derived.split_last()?;
    if !derived_rest.rest || derived_fixed.iter().any(|parameter| parameter.rest) {
        return None;
    }
    if matches!(derived_rest.annotation.as_ref(), Some(Type::Tuple(_))) {
        let inherited_array_rest = inherited.last().is_some_and(|parameter| {
            parameter.rest && matches!(parameter.annotation.as_ref(), Some(Type::Array(_)))
        });
        if inherited_has_rest && !inherited_array_rest {
            let (inherited_rest, inherited_fixed) = inherited.split_last()?;
            return (inherited_rest.rest
                && !inherited_fixed.iter().any(|parameter| parameter.rest)
                && matches!(inherited_rest.annotation.as_ref(), Some(Type::Tuple(_))))
            .then_some(RestShape::BothTuples {
                derived_fixed: derived_fixed.len(),
                inherited_fixed: inherited_fixed.len(),
            });
        }
        return Some(RestShape::DerivedTuple {
            derived_fixed: derived_fixed.len(),
            inherited_fixed: inherited.len() - usize::from(inherited_array_rest),
            inherited_array_rest,
        });
    }
    if !matches!(derived_rest.annotation.as_ref(), Some(Type::Array(_))) {
        return None;
    }
    if !inherited_has_rest && derived_fixed.len() <= inherited.len() {
        return Some(RestShape::DerivedCoversFixed(derived_fixed.len()));
    }
    let (inherited_rest, inherited_fixed) = inherited.split_last()?;
    if inherited_rest.rest
        && !inherited_fixed.iter().any(|parameter| parameter.rest)
        && matches!(inherited_rest.annotation.as_ref(), Some(Type::Tuple(_)))
    {
        return Some(RestShape::InheritedTuple {
            derived_fixed: derived_fixed.len(),
            inherited_fixed: inherited_fixed.len(),
            derived_array_rest: true,
        });
    }
    if !inherited_rest.rest
        || inherited_fixed.iter().any(|parameter| parameter.rest)
        || !matches!(inherited_rest.annotation.as_ref(), Some(Type::Array(_)))
    {
        return None;
    }
    if inherited_fixed.len() == derived_fixed.len() {
        Some(RestShape::MatchingArrays)
    } else {
        Some(RestShape::ShiftedArrays {
            derived_fixed: derived_fixed.len(),
            inherited_fixed: inherited_fixed.len(),
        })
    }
}

fn parameter_types_compatible(
    derived: &Type,
    inherited: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    is_assignable(derived, inherited, aliases, &mut HashSet::new(), budget)
        || is_assignable(inherited, derived, aliases, &mut HashSet::new(), budget)
}
