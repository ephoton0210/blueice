// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration-owned value queries never resolve in a consuming module's scope.

use super::*;

pub(in crate::checker) fn seed_value_query_contexts(
    project: &Project,
    ambient: &mut AmbientDeclarations,
) {
    for (id, module) in &project.modules {
        for reference in module
            .type_references
            .iter()
            .filter(|reference| reference.value_query)
        {
            let query = format!("typeof {}@{}", reference.name, reference.span.start);
            ambient.contexts.entry(id.clone()).or_default().insert(
                query.clone(),
                Type::Named {
                    name: contexts::private_name(id, &query),
                    arguments: Vec::new(),
                },
            );
        }
    }
}

pub(in crate::checker) fn resolve_value_queries(
    project: &Project,
    exports: &ProjectExports,
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
    policy: &module::CheckerPolicy,
    limit: usize,
) {
    let mut budget = TypeExpansionBudget::new(limit);
    for (id, module) in &project.modules {
        let references = module
            .type_references
            .iter()
            .filter(|reference| reference.value_query)
            .collect::<Vec<_>>();
        if references.is_empty() {
            continue;
        }
        let namespaces = BTreeMap::new();
        let mut checker = module::ModuleChecker::new(
            project,
            module,
            exports,
            Some(ambient),
            &namespaces,
            policy.clone(),
            limit,
        );
        checker.bind();
        let query_types = checker.declaration_value_query_types();
        let mut resolved = Vec::new();
        for reference in references {
            if !budget.consume() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    reference.span.clone(),
                    "ambient value queries exceed the type-expansion limit",
                ));
                return;
            }
            let query = format!("typeof {}@{}", reference.name, reference.span.start);
            if let Some(value) = query_types.get(&query) {
                let value = ambient
                    .contexts
                    .get(id)
                    .map_or(value.clone(), |context| substitute_type(value, context));
                resolved.push((
                    contexts::private_name(id, &query),
                    TypeDefinition {
                        kind: TypeDefinitionKind::Alias,
                        parameters: Vec::new(),
                        value,
                    },
                ));
            }
        }
        ambient.private_types.extend(resolved);
    }
}
