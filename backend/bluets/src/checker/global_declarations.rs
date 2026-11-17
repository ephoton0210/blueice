// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Project-owned global declarations share signatures without copying bodies.

use super::*;
mod collisions;
pub(super) use collisions::{diagnose, name_span};

#[derive(Clone)]
pub(super) struct Implementation {
    pub(super) span: SourceSpan,
    pub(super) signature: FunctionSignature,
}

#[derive(Default)]
pub(super) struct FunctionGroup {
    pub(super) has_overloads: bool,
    pub(super) implementations: Vec<Implementation>,
}

pub(super) fn implementation(function: &FunctionDeclaration) -> Implementation {
    Implementation {
        span: function.span.clone(),
        signature: FunctionSignature {
            parameters: function.parameters.clone(),
            type_parameters: function.type_parameters.clone(),
            return_type: function.return_type.clone().unwrap_or(Type::Unknown),
        },
    }
}

pub(super) fn bodies(project: &Project) -> Vec<&[Declaration]> {
    fn augmentations<'a>(
        declarations: &'a [Declaration],
        allow_global: bool,
        bodies: &mut Vec<&'a [Declaration]>,
    ) {
        for declaration in declarations {
            match declaration {
                Declaration::Ambient(item) if item.specifier.is_some() => {
                    augmentations(&item.body, true, bodies)
                }
                Declaration::Ambient(item) if allow_global => {
                    bodies.push(&item.body);
                    augmentations(&item.body, false, bodies);
                }
                Declaration::Namespace(item) => augmentations(&item.body, false, bodies),
                _ => {}
            }
        }
    }
    let mut bodies = Vec::new();
    for id in checking::dependency_order(project) {
        let module = &project.modules[&id];
        if !project.is_external_module(module) {
            bodies.push(module.declarations.as_slice());
        }
        augmentations(
            &module.declarations,
            project.is_external_module(module),
            &mut bodies,
        );
    }
    bodies
}

pub(super) fn function_groups(project: &Project) -> BTreeMap<String, FunctionGroup> {
    let mut groups = BTreeMap::<String, FunctionGroup>::new();
    for body in bodies(project) {
        for declaration in body {
            let Declaration::Function(function) = declaration else {
                continue;
            };
            let group = groups.entry(function.name.clone()).or_default();
            group.has_overloads |= function.overload || function.declared;
            if !function.overload
                && !function.declared
                && !is_declaration_module(&function.span.module)
            {
                group.implementations.push(implementation(function));
            }
        }
    }
    groups
}
