// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import queries bind only static surfaces from the already authorized graph.
use super::*;
use crate::checker::project::local_type_definitions;
use crate::parser::ImportTypeReference;

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn qualify_type_import_definition(
        &mut self,
        source: &str,
        definition: &TypeDefinition,
    ) -> TypeDefinition {
        if definition.kind != TypeDefinitionKind::Alias {
            return definition.clone();
        }
        let definitions = self
            .project
            .modules
            .get(source)
            .map(local_type_definitions)
            .unwrap_or_default();
        let rename = definitions
            .keys()
            .map(|name| (name.clone(), ImportTypeReference::identity(source, name)))
            .collect::<BTreeMap<_, _>>();
        let qualify = Qualifier {
            rename: &rename,
            argument_prefix: None,
        };
        for (name, definition) in &definitions {
            self.types
                .insert(rename[name].clone(), qualify.definition(definition));
        }
        qualify.definition(definition)
    }
    pub(in crate::checker::module) fn bind_import_types(&mut self) {
        if !self.project.has_import_types {
            return;
        }
        let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (from, target) in self.project.resolution_edges() {
            edges.entry(from).or_default().push(target);
        }
        let mut pending = vec![self.module.id.clone()];
        let mut seen = BTreeSet::new();
        while let Some(owner) = pending.pop() {
            if !seen.insert(owner.clone()) {
                continue;
            }
            pending.extend(
                edges
                    .get(owner.as_str())
                    .into_iter()
                    .flatten()
                    .map(|target| target.to_string()),
            );
            let Some(module) = self.project.modules.get(&owner) else {
                continue;
            };
            let references = module.import_types.clone();
            for reference in references {
                let Some(target) = self
                    .project
                    .resolved_import_type(&owner, &reference.specifier)
                    .cloned()
                else {
                    continue;
                };
                // Follow type edges once, not recursive checker invocations. All
                // nodes/edges/bytes/depth were charged by ProjectBuilder already.
                pending.push(target.clone());
                self.bind_import_type_surface(&reference, &target, owner == self.module.id);
            }
        }
    }

    fn bind_import_type_surface(
        &mut self,
        reference: &ImportTypeReference,
        target: &str,
        diagnose: bool,
    ) {
        if reference.qualifier.is_empty() && !reference.value_query {
            if diagnose && self.enforce_types {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        reference.span.clone(),
                        "a module namespace is not itself an instance type",
                    )
                    .with_typescript(1340, vec![reference.specifier.clone()]),
                );
            }
            return;
        }
        if reference.value_query {
            let selected = if reference.qualifier.is_empty() {
                let values = self.exports.values.get(target).cloned().unwrap_or_default();
                let mut fields = Vec::new();
                for (name, value) in values {
                    if name == "export=" {
                        continue;
                    }
                    let local = format!("{}.{}", reference.name, name);
                    self.bind_imported_query(&local, &value, &reference.span);
                    fields.push(TypeField {
                        name,
                        value: Type::Named {
                            name: format!("typeof {local}"),
                            arguments: Vec::new(),
                        },
                        readonly: true,
                        optional: false,
                        method: false,
                        accessor_write_type: None,
                        span: reference.span.clone(),
                    });
                }
                Some(Type::Record(fields))
            } else if let Some(value) = self
                .exports
                .values
                .get(target)
                .and_then(|values| values.get(&reference.qualifier))
                .cloned()
            {
                self.bind_imported_query(&reference.name, &value, &reference.span);
                self.types
                    .get(&format!("typeof {}", reference.name))
                    .map(|definition| definition.value.clone())
            } else {
                self.import_namespace_value(target, &reference.qualifier)
            };
            let Some(value) = selected else {
                self.missing_import_type_member(reference, target, diagnose);
                return;
            };
            let (parameters, value) = match value {
                Type::GenericFunction {
                    type_parameters,
                    parameters,
                    result,
                    ..
                } if !reference.arguments.is_empty() => {
                    (type_parameters, Type::Function { parameters, result })
                }
                value => (Vec::new(), value),
            };
            self.types.insert(
                reference.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters,
                    value,
                },
            );
            return;
        }
        let mut definitions = self
            .project
            .modules
            .get(target)
            .map(local_type_definitions)
            .unwrap_or_default();
        let exported = self
            .exports
            .types
            .get(target)
            .and_then(|types| types.get(&reference.qualifier));
        // Local definitions preserve all merged interface blocks and private
        // recursive dependencies. Exported membership still gates selection.
        let selected = exported
            .map(|definition| {
                definitions
                    .get(&reference.qualifier)
                    .unwrap_or(definition)
                    .clone()
            })
            .or_else(|| self.import_namespace_type(target, &reference.qualifier));
        let Some(definition) = selected else {
            self.missing_import_type_member(reference, target, diagnose);
            return;
        };
        definitions.insert(reference.qualifier.clone(), definition.clone());
        let rename = definitions
            .keys()
            .map(|name| (name.clone(), ImportTypeReference::identity(target, name)))
            .collect::<BTreeMap<_, _>>();
        let qualify = Qualifier {
            rename: &rename,
            argument_prefix: None,
        };
        for (name, definition) in &definitions {
            self.types
                .insert(rename[name].clone(), qualify.definition(definition));
        }
        self.types
            .insert(reference.name.clone(), qualify.definition(&definition));
    }

    fn import_namespace_type(&self, target: &str, path: &str) -> Option<TypeDefinition> {
        let (root, _) = path.split_once('.')?;
        let namespace = self.namespace_exports.get(target)?.get(root)?;
        let (parent, member) = path.rsplit_once('.')?;
        let members = namespace.members.get(parent)?;
        members
            .types
            .contains(member)
            .then(|| namespace.types.get(path).cloned())
            .flatten()
    }

    fn import_namespace_value(&self, target: &str, path: &str) -> Option<Type> {
        let (root, _) = path.split_once('.')?;
        let namespace = self.namespace_exports.get(target)?.get(root)?;
        namespace.values.get(path).cloned()
    }

    fn missing_import_type_member(
        &mut self,
        reference: &ImportTypeReference,
        target: &str,
        diagnose: bool,
    ) {
        if !diagnose || !self.enforce_types {
            return;
        }
        let source = target.strip_prefix("memory:///").unwrap_or(target);
        let source = source
            .strip_suffix(".d.ts")
            .or_else(|| source.strip_suffix(".ts"))
            .unwrap_or(source);
        let (parent, member) = reference
            .qualifier
            .rsplit_once('.')
            .unwrap_or(("", reference.qualifier.as_str()));
        let namespace = format!(
            "\"{source}\"{}{}",
            if parent.is_empty() { "" } else { "." },
            parent
        );
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::UnknownType,
                reference.member_span.clone(),
                format!(
                    "type import has no exported member `{}`",
                    reference.qualifier
                ),
            )
            .with_typescript(2694, vec![namespace, member.into()]),
        );
    }

    pub(in crate::checker::module) fn check_import_type_arguments(
        &mut self,
        name: &str,
        arguments: &[Type],
        definition: &TypeDefinition,
        span: &SourceSpan,
    ) -> bool {
        let Some(reference) = self
            .module
            .import_types
            .iter()
            .find(|reference| {
                reference.name == name
                    && reference.arguments == arguments
                    && reference.span.module == span.module
                    && span.start <= reference.span.start
                    && reference.span.end <= span.end
            })
            .cloned()
        else {
            return false;
        };
        let member = reference
            .qualifier
            .rsplit('.')
            .next()
            .unwrap_or(&reference.qualifier);
        let display = if definition.parameters.is_empty() {
            member.to_string()
        } else {
            format!(
                "{member}<{}>",
                definition
                    .parameters
                    .iter()
                    .map(|p| crate::parser::source_type_name(&p.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let mut described = definition.clone();
        described.kind = TypeDefinitionKind::Interface;
        self.check_type_arguments(&display, arguments, &described, &reference.span);
        true
    }
}
