// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recover the declaring intrinsic before ordinary import/name rendering.

use super::*;
use crate::parser::{IntrinsicKind, StringMappingType};

impl Context<'_> {
    pub(super) fn retain_type(&mut self, name: &str) {
        let owns = |module: &Module| owns_type(&module.declarations, name);
        let imported = self.module.declarations.iter().any(|declaration| {
            matches!(declaration, Declaration::Import(import)
                if import.bindings.iter().any(|binding| binding.local == name))
        });
        let owner_type = owns(self.module)
            || self
                .project
                .ambient_declaration_modules
                .iter()
                .any(|id| self.project.modules.get(id).is_some_and(&owns));
        let library_intrinsic = !imported
            && !owner_type
            && self
                .output_options
                .library_target()
                .into_iter()
                .flat_map(crate::standard_library::modules)
                .any(|module| {
                    module.declarations.iter().any(|declaration| {
                        matches!(declaration, Declaration::TypeAlias(alias)
                        if alias.name == name && matches!(&alias.value,
                            Type::Intrinsic(intrinsic)
                                if matches!(intrinsic.kind, IntrinsicKind::StringMapping(_))))
                    })
                });
        if !library_intrinsic {
            self.retain(name);
        }
    }

    pub(super) fn string_mapping_reference(&self, mapping: &StringMappingType) -> Option<Type> {
        let name = if let Some(module) = self.project.modules.get(&mapping.origin.module) {
            let name = declaring_alias(&module.declarations, mapping)?;
            if mapping.origin.module == self.module.id {
                name
            } else {
                format!("{name}@{}", mapping.origin.module)
            }
        } else {
            let module = self
                .output_options
                .library_target()
                .into_iter()
                .flat_map(crate::standard_library::modules)
                .find(|module| module.id == mapping.origin.module)?;
            declaring_alias(&module.declarations, mapping)?
        };
        Some(Type::Named {
            name,
            arguments: vec![mapping.operand.clone()],
        })
    }

    pub(super) fn render_string_mapping(
        &mut self,
        mapping: &StringMappingType,
        indent: usize,
        span: &SourceSpan,
    ) -> Result<String, Diagnostic> {
        let reference = self.string_mapping_reference(mapping).ok_or_else(|| {
            self.cannot_name(mapping.operation.name(), &mapping.origin.module, span)
        })?;
        self.render(&reference, indent, span)
    }
}

fn owns_type(declarations: &[Declaration], name: &str) -> bool {
    declarations.iter().any(|declaration| match declaration {
        Declaration::TypeAlias(alias) => alias.name == name,
        Declaration::Interface(interface) => interface.name == name,
        Declaration::Class(class) => class.name == name,
        Declaration::Enum(enumeration) => enumeration.name == name,
        Declaration::Ambient(ambient) => owns_type(&ambient.body, name),
        _ => false,
    })
}

fn declaring_alias(declarations: &[Declaration], mapping: &StringMappingType) -> Option<String> {
    let mut pending = vec![(declarations, String::new())];
    while let Some((declarations, prefix)) = pending.pop() {
        for declaration in declarations {
            let qualify = |name: &str| {
                if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{prefix}.{name}")
                }
            };
            match declaration {
                Declaration::TypeAlias(alias)
                    if matches!(&alias.value, Type::Intrinsic(intrinsic)
                    if intrinsic.origin == mapping.origin && intrinsic.kind == IntrinsicKind::StringMapping(mapping.operation)) =>
                {
                    return Some(qualify(&alias.name));
                }
                Declaration::Namespace(namespace) => {
                    pending.push((&namespace.body, qualify(&namespace.name)))
                }
                Declaration::Ambient(ambient) => pending.push((&ambient.body, prefix.clone())),
                _ => {}
            }
        }
    }
    None
}
