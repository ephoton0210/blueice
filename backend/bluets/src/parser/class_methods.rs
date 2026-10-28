// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared method/function suspension metadata retains the original source span.
use super::*;

impl ClassMethod {
    pub fn as_function(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: self.name.clone(),
            anonymous: false,
            async_function: self.async_function,
            generator: self.generator,
            body_open: None,
            type_parameters: self.type_parameters.clone(),
            parameters: self.parameters.clone(),
            return_type: self.return_type.clone(),
            body: self.body.clone().unwrap_or_default(),
            returns: Vec::new(),
            locals: Vec::new(),
            exported: false,
            default_export: false,
            declared: self.body.is_none(),
            overload: self.body.is_none(),
            span: self.span.clone(),
        }
    }
}
