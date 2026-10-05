// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compatibility checks for inherited class method declarations.

use super::*;
use crate::parser::{ClassMethodGroup, Parameter};

mod overloads;
mod signatures;

use signatures::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn validate_class_method_overrides(
        &mut self,
        class: &ClassDeclaration,
    ) {
        for group in &class.method_groups {
            if group.name.starts_with('#') {
                continue;
            }
            if self.validate_overload_set_override(class, group) {
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
            let mut specialization_budget = TypeExpansionBudget::new(self.max_type_expansions);
            let (Ok(derived_parameters), Ok(inherited_parameters)) = (
                specialize_class_rest_parameters(
                    &derived.parameters,
                    &self.types,
                    &mut specialization_budget,
                ),
                specialize_class_rest_parameters(
                    inherited.parameters,
                    &self.types,
                    &mut specialization_budget,
                ),
            ) else {
                self.type_error(
                    &derived.span,
                    "class method rest annotations cannot be specialized within the type budget"
                        .to_string(),
                    if specialization_budget.exhausted {
                        DiagnosticCode::ResourceLimit
                    } else {
                        DiagnosticCode::UnsupportedSyntax
                    },
                );
                continue;
            };
            let mut derived = derived.clone();
            derived.parameters = derived_parameters;
            let inherited = InheritedMethod {
                base_name: inherited.base_name,
                parameters: &inherited_parameters,
                return_type: inherited.return_type,
            };
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
                    RestShape::DerivedTuple { .. }
                        | RestShape::DerivedTrailingTuple { .. }
                        | RestShape::DerivedMiddleTuple { .. }
                        | RestShape::BothTuples { .. }
                        | RestShape::BothTrailingTuples { .. }
                        | RestShape::BothVariableTuples { .. }
                        | RestShape::MiddleFixedTuples { .. }
                ) {
                    let Some(Type::Tuple(elements)) = derived
                        .parameters
                        .last()
                        .and_then(|parameter| parameter.annotation.as_ref())
                    else {
                        unreachable!("rest shape requires a tuple annotation")
                    };
                    elements
                        .iter()
                        .filter(|element| !element.optional && !element.rest)
                        .count()
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
                RestShape::MatchingArrays
                    | RestShape::ShiftedArrays { .. }
                    | RestShape::InheritedTrailingTuple { .. }
                    | RestShape::BothTrailingTuples { .. }
                    | RestShape::BothVariableTuples { .. }
            ) || matches!(
                rest_shape,
                RestShape::DerivedTuple {
                    inherited_array_rest: true,
                    ..
                } | RestShape::DerivedTrailingTuple {
                    inherited_array_rest: true,
                    ..
                } | RestShape::DerivedMiddleTuple {
                    inherited_array_rest: true,
                    ..
                }
            ) || match rest_shape {
                RestShape::DerivedMiddleTuple {
                    inherited_array_rest: false,
                    derived_fixed,
                    ..
                } => {
                    middle_tuple_required(&derived.parameters, derived_fixed)
                        <= inherited
                            .parameters
                            .iter()
                            .filter(|parameter| !parameter.optional)
                            .count()
                }
                RestShape::InheritedMiddleTuple {
                    derived_fixed,
                    inherited_fixed,
                    derived_array_rest: true,
                } => derived_fixed <= inherited_fixed + middle_tuple_prefix(inherited.parameters),
                RestShape::InheritedMiddleTuple {
                    inherited_fixed,
                    derived_array_rest: false,
                    ..
                } => {
                    required_derived >= middle_tuple_required(inherited.parameters, inherited_fixed)
                }
                RestShape::MiddleFixedTuples {
                    derived_fixed,
                    inherited_fixed,
                    derived_variable: true,
                } => {
                    middle_tuple_required(&derived.parameters, derived_fixed)
                        <= fixed_tuple_required(inherited.parameters, inherited_fixed)
                }
                RestShape::MiddleFixedTuples {
                    derived_fixed,
                    inherited_fixed,
                    derived_variable: false,
                } => {
                    fixed_tuple_required(&derived.parameters, derived_fixed)
                        >= middle_tuple_required(inherited.parameters, inherited_fixed)
                }
                RestShape::OptionalFixedMiddleTuples => false,
                _ => required_derived <= inherited_arity,
            };
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
                                &elements[index - derived_fixed].value_type()
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
                    RestShape::DerivedTrailingTuple {
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
                        let (tail, fixed_elements) = elements
                            .split_last()
                            .expect("trailing tuple rest requires a rest element");
                        let tail_type = tail.indexed_type();
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
                        let derived_known = derived_fixed + fixed_elements.len();
                        let positions = if inherited_array_rest {
                            derived_known.max(inherited_fixed)
                        } else {
                            inherited_fixed
                        };
                        (0..positions).all(|index| {
                            let derived_type = if index < derived_fixed {
                                derived.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else if index - derived_fixed < fixed_elements.len() {
                                &fixed_elements[index - derived_fixed].value_type()
                            } else {
                                &tail_type
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
                        }) && inherited_element.is_none_or(|element| {
                            parameter_types_compatible(
                                &tail_type,
                                element,
                                &self.types,
                                &mut budget,
                            )
                        })
                    }
                    RestShape::DerivedMiddleTuple {
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
                        let minimum_length = if inherited_array_rest {
                            middle_tuple_required(&derived.parameters, derived_fixed)
                                .max(inherited_fixed)
                        } else {
                            inherited
                                .parameters
                                .iter()
                                .filter(|parameter| !parameter.optional)
                                .count()
                        };
                        let maximum_length = if inherited_array_rest {
                            derived_fixed + elements.len() + inherited_fixed + 1
                        } else {
                            inherited_fixed
                        };
                        (minimum_length..=maximum_length).all(|length| {
                            (0..length).all(|index| {
                                if !budget.consume() {
                                    return false;
                                }
                                let derived_type = if index < derived_fixed {
                                    Some(override_parameter_type(&derived.parameters[index]))
                                } else {
                                    length.checked_sub(derived_fixed).and_then(|tuple_length| {
                                        tuple_type_at_length(
                                            elements,
                                            tuple_length,
                                            index - derived_fixed,
                                        )
                                    })
                                };
                                let Some(derived_type) = derived_type else {
                                    return false;
                                };
                                let inherited_type = if index < inherited_fixed {
                                    override_parameter_type(&inherited.parameters[index])
                                } else {
                                    inherited_element
                                        .expect("rest shape requires an array annotation")
                                        .clone()
                                };
                                parameter_types_compatible(
                                    &derived_type,
                                    &inherited_type,
                                    &self.types,
                                    &mut budget,
                                )
                            })
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
                                &elements[index - inherited_fixed].value_type()
                            };
                            parameter_types_compatible(
                                derived_type,
                                inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        })
                    }
                    RestShape::InheritedTrailingTuple {
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
                        let (tail, fixed_elements) = elements
                            .split_last()
                            .expect("trailing tuple rest requires a rest element");
                        let tail_type = tail.indexed_type();
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
                        let inherited_known = inherited_fixed + fixed_elements.len();
                        let positions = if derived_array_rest {
                            derived_fixed.max(inherited_known)
                        } else {
                            derived_fixed
                        };
                        (0..positions).all(|index| {
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
                            } else if index - inherited_fixed < fixed_elements.len() {
                                &fixed_elements[index - inherited_fixed].value_type()
                            } else {
                                &tail_type
                            };
                            parameter_types_compatible(
                                derived_type,
                                inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        }) && derived_element.is_none_or(|element| {
                            parameter_types_compatible(
                                element,
                                &tail_type,
                                &self.types,
                                &mut budget,
                            )
                        })
                    }
                    RestShape::InheritedMiddleTuple {
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
                        let minimum_length = if derived_array_rest {
                            middle_tuple_required(inherited.parameters, inherited_fixed)
                                .max(derived_fixed)
                        } else {
                            required_derived
                        };
                        let maximum_length = if derived_array_rest {
                            inherited_fixed + elements.len() + derived_fixed + 1
                        } else {
                            derived_fixed
                        };
                        (minimum_length..=maximum_length).all(|length| {
                            (0..length).all(|index| {
                                if !budget.consume() {
                                    return false;
                                }
                                let derived_type = if index < derived_fixed {
                                    Some(override_parameter_type(&derived.parameters[index]))
                                } else {
                                    derived_element.cloned()
                                };
                                let Some(derived_type) = derived_type else {
                                    return false;
                                };
                                let inherited_type = if index < inherited_fixed {
                                    Some(override_parameter_type(&inherited.parameters[index]))
                                } else {
                                    length
                                        .checked_sub(inherited_fixed)
                                        .and_then(|tuple_length| {
                                            tuple_type_at_length(
                                                elements,
                                                tuple_length,
                                                index - inherited_fixed,
                                            )
                                        })
                                };
                                let Some(inherited_type) = inherited_type else {
                                    return false;
                                };
                                parameter_types_compatible(
                                    &derived_type,
                                    &inherited_type,
                                    &self.types,
                                    &mut budget,
                                )
                            })
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
                                &derived_elements[index - derived_fixed].value_type()
                            };
                            let inherited_type = if index < inherited_fixed {
                                inherited.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else {
                                &inherited_elements[index - inherited_fixed].value_type()
                            };
                            parameter_types_compatible(
                                derived_type,
                                inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        })
                    }
                    RestShape::BothTrailingTuples {
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
                        let (derived_tail, derived_prefix) = derived_elements
                            .split_last()
                            .expect("trailing tuple rest requires a rest element");
                        let (inherited_tail, inherited_prefix) = inherited_elements
                            .split_last()
                            .expect("trailing tuple rest requires a rest element");
                        let derived_tail_type = derived_tail.indexed_type();
                        let inherited_tail_type = inherited_tail.indexed_type();
                        let positions = (derived_fixed + derived_prefix.len())
                            .max(inherited_fixed + inherited_prefix.len());
                        (0..positions).all(|index| {
                            let derived_type = if index < derived_fixed {
                                derived.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else if index - derived_fixed < derived_prefix.len() {
                                &derived_prefix[index - derived_fixed].value_type()
                            } else {
                                &derived_tail_type
                            };
                            let inherited_type = if index < inherited_fixed {
                                inherited.parameters[index]
                                    .annotation
                                    .as_ref()
                                    .unwrap_or(&Type::Unknown)
                            } else if index - inherited_fixed < inherited_prefix.len() {
                                &inherited_prefix[index - inherited_fixed].value_type()
                            } else {
                                &inherited_tail_type
                            };
                            parameter_types_compatible(
                                derived_type,
                                inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        }) && parameter_types_compatible(
                            &derived_tail_type,
                            &inherited_tail_type,
                            &self.types,
                            &mut budget,
                        )
                    }
                    RestShape::BothVariableTuples {
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
                        let derived_required = derived_fixed
                            + derived_elements
                                .iter()
                                .filter(|element| !element.optional && !element.rest)
                                .count();
                        let inherited_required = inherited_fixed
                            + inherited_elements
                                .iter()
                                .filter(|element| !element.optional && !element.rest)
                                .count();
                        let minimum_length = derived_required.max(inherited_required);
                        let maximum_length = derived_fixed
                            + derived_elements.len()
                            + inherited_fixed
                            + inherited_elements.len()
                            + 1;
                        (minimum_length..=maximum_length).all(|length| {
                            (0..length).all(|index| {
                                if !budget.consume() {
                                    return false;
                                }
                                let derived_type = if index < derived_fixed {
                                    Some(override_parameter_type(&derived.parameters[index]))
                                } else {
                                    tuple_type_at_length(
                                        derived_elements,
                                        length - derived_fixed,
                                        index - derived_fixed,
                                    )
                                };
                                let inherited_type = if index < inherited_fixed {
                                    Some(override_parameter_type(&inherited.parameters[index]))
                                } else {
                                    tuple_type_at_length(
                                        inherited_elements,
                                        length - inherited_fixed,
                                        index - inherited_fixed,
                                    )
                                };
                                let (Some(derived_type), Some(inherited_type)) =
                                    (derived_type, inherited_type)
                                else {
                                    return false;
                                };
                                parameter_types_compatible(
                                    &derived_type,
                                    &inherited_type,
                                    &self.types,
                                    &mut budget,
                                )
                            })
                        })
                    }
                    RestShape::MiddleFixedTuples {
                        derived_fixed,
                        inherited_fixed,
                        derived_variable,
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
                        let length = if derived_variable {
                            inherited_fixed + inherited_elements.len()
                        } else {
                            derived_fixed + derived_elements.len()
                        };
                        (0..length).all(|index| {
                            if !budget.consume() {
                                return false;
                            }
                            let derived_type = if index < derived_fixed {
                                Some(override_parameter_type(&derived.parameters[index]))
                            } else {
                                tuple_type_at_length(
                                    derived_elements,
                                    length - derived_fixed,
                                    index - derived_fixed,
                                )
                            };
                            let inherited_type = if index < inherited_fixed {
                                Some(override_parameter_type(&inherited.parameters[index]))
                            } else {
                                tuple_type_at_length(
                                    inherited_elements,
                                    length - inherited_fixed,
                                    index - inherited_fixed,
                                )
                            };
                            let (Some(derived_type), Some(inherited_type)) =
                                (derived_type, inherited_type)
                            else {
                                return false;
                            };
                            parameter_types_compatible(
                                &derived_type,
                                &inherited_type,
                                &self.types,
                                &mut budget,
                            )
                        })
                    }
                    RestShape::OptionalFixedMiddleTuples => false,
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
