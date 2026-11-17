// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The selected program chooses one inheritance ABI across its source modules.
use super::*;

#[derive(Clone, Copy)]
pub(in crate::emitter) struct ClassContext {
    pub(super) inheritance: bool,
}

impl ClassContext {
    pub(in crate::emitter) fn for_project(project: &Project) -> Self {
        fn inherits(declarations: &[Declaration]) -> bool {
            declarations.iter().any(|declaration| match declaration {
                Declaration::Class(class) => class.extends_span.is_some(),
                Declaration::Namespace(namespace) => {
                    !namespace.declared && inherits(&namespace.body)
                }
                _ => false,
            })
        }
        Self {
            inheritance: project
                .modules
                .values()
                .filter(|module| !crate::compiler::is_declaration_module(&module.id))
                .any(|module| {
                    inherits(&module.declarations)
                        || module
                            .class_expressions
                            .values()
                            .any(|node| node.class.extends_span.is_some())
                }),
        }
    }
}
