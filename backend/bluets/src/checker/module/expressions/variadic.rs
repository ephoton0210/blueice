// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Forwarding a rigid argument pack requires the same lexical parameter.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn correlated_argument_pack(
        &self,
        actual: &Type,
        index: usize,
        signatures: &[FunctionSignature],
    ) -> bool {
        let actual = canonical_parameter_references(actual, &self.types);
        let Type::Named { name, arguments } = &actual else {
            return false;
        };
        if !arguments.is_empty()
            || !self.types.get(name).is_some_and(|definition| {
                definition.kind == TypeDefinitionKind::Parameter
                    && matches!(definition.value, Type::Tuple(_) | Type::Array(_))
            })
        {
            return false;
        }
        !signatures.is_empty()
            && signatures.iter().all(|signature| {
                let parameters = signature
                    .parameters
                    .iter()
                    .filter(|p| p.name != "this")
                    .collect::<Vec<_>>();
                signature.type_parameters.is_empty()
                    && parameters.len() == index + 1
                    && parameters.last().is_some_and(|parameter| {
                        parameter.rest
                            && parameter.annotation.as_ref().is_some_and(|expected| {
                                canonical_parameter_references(expected, &self.types) == actual
                            })
                    })
            })
    }
}
