// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class expressions carry a source-local identity and an optional lexical name.

use super::*;

mod captures;

pub(crate) fn type_parameter_identity(parameter: &TypeParameter) -> String {
    if parameter.name.starts_with("#typeparam@") {
        return parameter.name.clone();
    }
    format!(
        "#typeparam@{}:{}:{}",
        parameter.span.module, parameter.span.start, parameter.name
    )
}

pub(super) fn parameter_source_name(name: &str) -> Option<&str> {
    name.starts_with("#typeparam@")
        .then(|| name.rsplit_once(':').map(|(_, source)| source))
        .flatten()
}

impl ClassDeclaration {
    pub fn export_name(&self) -> &str {
        if self.default_export {
            "default"
        } else {
            &self.name
        }
    }

    pub(crate) fn instance_parameters(&self) -> Vec<TypeParameter> {
        self.captured_type_parameters
            .iter()
            .chain(&self.type_parameters)
            .cloned()
            .collect()
    }

    pub(crate) fn instance_reference(&self, arguments: Vec<Type>) -> Type {
        Type::Named {
            name: self.name.clone(),
            arguments: self
                .captured_type_parameters
                .iter()
                .map(parameter_reference)
                .chain(arguments)
                .collect(),
        }
    }

    pub(crate) fn this_type(&self) -> Type {
        self.instance_reference(
            self.type_parameters
                .iter()
                .map(parameter_reference)
                .collect(),
        )
    }
}

fn parameter_reference(parameter: &TypeParameter) -> Type {
    Type::Named {
        name: parameter.name.clone(),
        arguments: Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassExpression {
    /// A name bound only within the class body; anonymous expressions have none.
    pub name: Option<String>,
    /// The class structure uses a unique internal name for its static identity.
    pub class: ClassDeclaration,
}

impl Module {
    pub fn class_expressions(&self) -> impl Iterator<Item = &ClassExpression> {
        self.class_expressions.values()
    }
    pub(crate) fn classes(&self) -> impl Iterator<Item = &ClassDeclaration> {
        self.declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Class(class) => Some(class),
                _ => None,
            })
            .chain(
                self.class_expressions
                    .values()
                    .map(|expression| &expression.class),
            )
    }

    /// Returns the class expression beginning at an original source-byte offset.
    pub fn class_expression(&self, start: usize) -> Option<&ClassExpression> {
        self.class_expressions.get(&start)
    }
}
