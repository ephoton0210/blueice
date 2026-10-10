// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original variable pattern AST lowering, preserving one source initializer
//! and the BlueJS engine's getter/default/rest evaluation order.

use super::*;
use blueice_bluets::{BindingPattern, VariableBindingPattern};

pub(super) fn variable(
    module: &Module,
    pattern: &VariableBindingPattern,
) -> Result<bluejs::Pattern, BridgeError> {
    match &pattern.pattern {
        BindingPattern::Object(bindings) => {
            let mut properties = Vec::new();
            for binding in bindings {
                properties.push(bluejs::ObjectPatternProp::KeyValue {
                    key: bluejs::PropertyKey::Identifier(binding.key.clone()),
                    value: bluejs::Pattern::Identifier(binding.name.clone()),
                    default: default(module, &binding.default)?,
                });
            }
            if let Some(rest) = &pattern.rest {
                properties.push(bluejs::ObjectPatternProp::Rest(
                    bluejs::Pattern::Identifier(rest.name.clone()),
                ));
            }
            Ok(bluejs::Pattern::Object(properties))
        }
        BindingPattern::Array(bindings) => {
            let mut elements = Vec::new();
            for binding in bindings {
                elements.push(match binding {
                    Some(binding) => Some(bluejs::ArrayPatternElement {
                        pattern: bluejs::Pattern::Identifier(binding.name.clone()),
                        default: default(module, &binding.default)?,
                        rest: false,
                    }),
                    None => None,
                });
            }
            if let Some(rest) = &pattern.rest {
                elements.push(Some(bluejs::ArrayPatternElement {
                    pattern: bluejs::Pattern::Identifier(rest.name.clone()),
                    default: None,
                    rest: true,
                }));
            }
            Ok(bluejs::Pattern::Array(elements))
        }
    }
}

fn default(
    module: &Module,
    tokens: &Option<Vec<Token>>,
) -> Result<Option<bluejs::Expr>, BridgeError> {
    tokens
        .as_ref()
        .map(|tokens| ExpressionLowerer::for_module(module, tokens).parse())
        .transpose()
}
