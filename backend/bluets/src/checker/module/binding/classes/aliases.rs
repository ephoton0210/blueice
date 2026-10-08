// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor aliases retain captured arguments and original class capabilities.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module::binding) fn bind_class_expression_aliases(&mut self) {
        let mut scope = self.values.clone();
        for declaration in &self.module.declarations {
            let Declaration::Variable(variable) = declaration else {
                continue;
            };
            if variable.annotation.is_some() {
                continue;
            }
            let value = self.infer_variable_type(variable, &scope);
            let Type::CallableRecord { signatures, .. } = value.object_type() else {
                continue;
            };
            let Some(identity) = signatures.iter().find_map(|signature| {
                if let (true, Type::Named { name, .. }) = (signature.construct, &signature.result) {
                    Some(name)
                } else {
                    None
                }
            }) else {
                continue;
            };
            let Some(mut binding) = self.class_constructors.get(identity).cloned() else {
                continue;
            };
            // A factory result has already substituted outer type arguments.
            // Its constructor still quantifies only its own class parameters.
            binding.signatures = signatures
                .iter()
                .filter(|signature| signature.construct)
                .map(|signature| FunctionSignature {
                    parameters: signature.parameters.clone(),
                    type_parameters: signature.type_parameters.clone(),
                    return_type: signature.result.clone(),
                })
                .collect();
            self.class_constructors
                .insert(variable.name.clone(), binding);
            scope.insert(variable.name.clone(), value.clone());
            self.values.insert(variable.name.clone(), value);
        }
    }
}
