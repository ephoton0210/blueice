// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fresh literal provenance follows only the already-authorized source graph.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn fresh_return(
        &self,
        tokens: &[Token],
        names: &BTreeSet<String>,
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        let tokens = strip_outer_parentheses(tokens);
        matches!(tokens, [name] if names.contains(&name.text))
            || self.fresh_array_element(tokens, scope, &mut BTreeSet::new(), 0)
    }

    pub(in crate::checker::module) fn fresh_local_return(&self, name: &str) -> Option<bool> {
        self.return_inference
            .active
            .borrow()
            .last()
            .filter(|frame| frame.bindings.contains(name))
            .map(|frame| frame.fresh_names.contains(name))
    }

    pub(in crate::checker::module) fn imported_literal_freshness(
        &self,
        name: &str,
    ) -> Option<bool> {
        let (module, imported) = self.module.declarations.iter().find_map(|declaration| {
            let Declaration::Import(import) = declaration else {
                return None;
            };
            let binding = import
                .bindings
                .iter()
                .find(|binding| binding.local == name)?;
            let target = self.project.resolved_import(&self.module.id, import)?;
            Some((self.project.modules.get(target)?, binding.imported.as_str()))
        })?;
        Some(source_name_fresh(
            self.project,
            module,
            imported,
            &mut BTreeSet::new(),
            self.max_type_expansions.min(128),
        ))
    }

    pub(in crate::checker::module) fn fresh_return_call(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        let mut tokens = strip_outer_parentheses(tokens);
        while tokens.first().is_some_and(|token| token.is("await")) {
            tokens = strip_outer_parentheses(&tokens[1..]);
        }
        if tokens.len() > 2 && tokens[tokens.len() - 2].is(".") {
            let property = tokens.last().unwrap();
            if let Some(call) = constructor_call_parts(&tokens[..tokens.len() - 2]) {
                if scope.get(&call.callee.text) != self.values.get(&call.callee.text) {
                    return false;
                }
                return self.module.declarations.iter().any(|declaration| {
                    matches!(declaration, Declaration::Class(class) if class.name == call.callee.text
                        && class.members.iter().filter_map(|member| member.accessor.as_ref())
                            .any(|accessor| accessor.getter && accessor.name == property.text
                                && accessor.return_type.is_none()
                                && self.return_inference.fresh_results.borrow().contains(&accessor.span.start)))
                });
            }
        }
        let Some(call) = direct_call_parts(tokens) else {
            return false;
        };
        if scope.get(&call.callee.text) != self.values.get(&call.callee.text) {
            return false;
        }
        self.module.declarations.iter().any(|declaration| {
            matches!(declaration, Declaration::Function(function) if function.name == call.callee.text
                && function.return_type.is_none()
                && self.return_inference.fresh_results.borrow().contains(&function.span.start))
        })
    }
}

pub(in crate::checker::module) fn getter_union(mut value: Type) -> Type {
    if let Type::Union(parts) = &mut value {
        parts.sort_by_key(|part| match part {
            Type::Literal(text) if text.parse::<f64>().is_ok() => 0,
            Type::Literal(text) if text.starts_with(['\'', '"', '`']) => 1,
            _ => 2,
        });
    }
    value
}

fn source_name_fresh(
    project: &Project,
    module: &Module,
    name: &str,
    seen: &mut BTreeSet<(String, String)>,
    limit: usize,
) -> bool {
    if limit == 0 || !seen.insert((module.id.clone(), name.into())) {
        return false;
    }
    for declaration in &module.declarations {
        match declaration {
            Declaration::Variable(variable) if variable.name == name => {
                if variable.kind != VariableKind::Const || variable.annotation.is_some() {
                    return false;
                }
                let tokens = strip_outer_parentheses(&variable.initializer);
                if let [token] = tokens {
                    if matches!(
                        token.kind,
                        TokenKind::Number | TokenKind::String | TokenKind::Template
                    ) || token.is("true")
                        || token.is("false")
                    {
                        return true;
                    }
                    return source_name_fresh(project, module, &token.text, seen, limit - 1);
                }
                return false;
            }
            Declaration::Import(import) => {
                if let Some(binding) = import.bindings.iter().find(|binding| binding.local == name)
                {
                    return project
                        .resolved_import(&module.id, import)
                        .and_then(|id| project.modules.get(id))
                        .is_some_and(|target| {
                            source_name_fresh(project, target, &binding.imported, seen, limit - 1)
                        });
                }
            }
            Declaration::ValueExport(export) => {
                if let Some(binding) = export
                    .bindings
                    .iter()
                    .find(|binding| binding.exported == name && !binding.type_only)
                {
                    if let Some(specifier) = &export.specifier {
                        return project
                            .resolved_module(&module.id, specifier)
                            .and_then(|id| project.modules.get(id))
                            .is_some_and(|target| {
                                source_name_fresh(project, target, &binding.local, seen, limit - 1)
                            });
                    }
                    return source_name_fresh(project, module, &binding.local, seen, limit - 1);
                }
            }
            _ => {}
        }
    }
    false
}
