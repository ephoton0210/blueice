// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optional tuple presentation is distinct from its write annotation.
use super::*;

pub(crate) fn display_type(value: &Type, checking: crate::CheckingOptions) -> Type {
    match value {
        Type::Readonly(value) => Type::Readonly(Box::new(display_type(value, checking))),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| {
                    let mut element = element.clone();
                    element.annotation = display_type(&element.annotation, checking);
                    if element.optional
                        && checking.strict_null_checks
                        && !checking.exact_optional_property_types
                    {
                        element.annotation = crate::checker::type_operators::union([
                            element.annotation,
                            Type::Undefined,
                        ]);
                    }
                    element
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}
