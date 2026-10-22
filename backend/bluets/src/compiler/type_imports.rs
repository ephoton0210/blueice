// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Read-only declaration provenance for a type edge; never resolves new inputs.
use super::*;
use crate::parser::{Declaration, ImportTypeReference, TypeParameter};

pub(crate) struct ImportTypeDeclaration<'a> {
    pub(crate) name: &'a str,
    pub(crate) parameters: &'a [TypeParameter],
}

impl Project {
    pub(crate) fn import_type_reference(
        &self,
        name: &str,
    ) -> Option<(&crate::Module, &ImportTypeReference)> {
        let (module, _) = name.strip_prefix("\0import:")?.split_once('\0')?;
        let module = self.modules.get(module)?;
        Some((
            module,
            module
                .import_types
                .iter()
                .find(|reference| reference.name == name)?,
        ))
    }

    pub(crate) fn import_type_declaration(
        &self,
        owner: &str,
        reference: &ImportTypeReference,
    ) -> Option<ImportTypeDeclaration<'_>> {
        let target = self.resolved_import_type(owner, &reference.specifier)?;
        self.exported_type_origin(target, &reference.qualifier, &mut BTreeSet::new())
    }

    pub(crate) fn imported_type_declaration(
        &self,
        owner: &str,
        path: &str,
    ) -> Option<ImportTypeDeclaration<'_>> {
        let module = self.modules.get(owner)?;
        let (root, tail) = path
            .split_once('.')
            .map_or((path, ""), |(root, tail)| (root, tail));
        for declaration in &module.declarations {
            let Declaration::Import(import) = declaration else {
                continue;
            };
            let Some(binding) = import.bindings.iter().find(|binding| binding.local == root) else {
                continue;
            };
            let target = self.resolved_import(owner, import)?;
            let path = if binding.imported == "*" {
                tail.to_string()
            } else if tail.is_empty() {
                binding.imported.clone()
            } else {
                format!("{}.{tail}", binding.imported)
            };
            if let Some(found) = self.exported_type_origin(target, &path, &mut BTreeSet::new()) {
                return Some(found);
            }
        }
        None
    }

    fn exported_type_origin<'a>(
        &'a self,
        module: &str,
        path: &str,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Option<ImportTypeDeclaration<'a>> {
        if !seen.insert((module.into(), path.into())) {
            return None;
        }
        let module = self.modules.get(module)?;
        if let Some(found) = declaration_origin(&module.declarations, path, true) {
            return Some(found);
        }
        let (head, tail) = path
            .split_once('.')
            .map_or((path, String::new()), |(head, tail)| {
                (head, format!(".{tail}"))
            });
        for declaration in &module.declarations {
            let (bindings, star, target) = match declaration {
                Declaration::TypeExport(export) => (
                    &export.bindings,
                    export.star,
                    self.resolved_type_export(&module.id, export),
                ),
                Declaration::ValueExport(export) => (
                    &export.bindings,
                    export.star,
                    export
                        .specifier
                        .as_ref()
                        .and_then(|specifier| self.resolved_import_type(&module.id, specifier)),
                ),
                _ => continue,
            };
            if let Some(target) = target {
                let imported = bindings
                    .iter()
                    .find(|binding| binding.exported == head)
                    .map(|binding| binding.local.as_str())
                    .or(star.then_some(head));
                if let Some(imported) = imported {
                    if let Some(found) =
                        self.exported_type_origin(target, &format!("{imported}{tail}"), seen)
                    {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
}

fn declaration_origin<'a>(
    declarations: &'a [Declaration],
    path: &str,
    exported: bool,
) -> Option<ImportTypeDeclaration<'a>> {
    let (head, tail) = path
        .split_once('.')
        .map_or((path, None), |(head, tail)| (head, Some(tail)));
    for declaration in declarations {
        let (name, public, parameters) = match declaration {
            Declaration::Namespace(namespace)
                if namespace.name == head && (!exported || namespace.exported) =>
            {
                if let Some(tail) = tail {
                    if let Some(found) =
                        declaration_origin(&namespace.body, tail, !namespace.exports_every_member())
                    {
                        return Some(found);
                    }
                }
                continue;
            }
            Declaration::Interface(interface)
                if interface.export_name() == head || (!exported && interface.name == head) =>
            {
                (
                    &interface.name,
                    interface.exported,
                    &interface.type_parameters,
                )
            }
            Declaration::TypeAlias(alias) if alias.name == head => {
                (&alias.name, alias.exported, &alias.type_parameters)
            }
            Declaration::Class(class)
                if class.export_name() == head || (!exported && class.name == head) =>
            {
                (&class.name, class.exported, &class.type_parameters)
            }
            _ => continue,
        };
        if tail.is_none() && (!exported || public) {
            return Some(ImportTypeDeclaration { name, parameters });
        }
    }
    None
}
