// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Dependency-ordered and incremental publication of checked module surfaces.

use super::*;

/// Rechecks the requested modules while retaining checker output for modules
/// that the incremental project graph proved unaffected. The caller must only
/// supply a previous project checked under the same compiler policy and must
/// include every reverse dependency of a changed module in `rechecked`.
pub(crate) fn check_incremental(
    project: &Project,
    policy: module::CheckerPolicy,
    previous: Option<&CheckedProject>,
    rechecked: &BTreeSet<String>,
    max_type_expansions: usize,
) -> (CheckedProject, Vec<Diagnostic>) {
    let mut diagnostics = project::declaration_module_diagnostics(project);
    let (ambient, mut ambient_diagnostics) = ambient_declarations(project);
    diagnostics.append(&mut ambient_diagnostics);
    let mut exports = ProjectExports {
        values: BTreeMap::new(),
        types: project::exported_types(project),
        classes: project::exported_classes(project, max_type_expansions),
        enums: project::exported_enums(project),
        exported_names: project::exported_names(project),
    };
    let empty_namespaces = BTreeMap::new();
    exports.values = project
        .modules
        .iter()
        .map(|(id, module)| {
            let mut checker = module::ModuleChecker::new(
                project,
                module,
                &exports,
                (!project.ambient_declaration_modules.contains(id)).then_some(&ambient),
                &empty_namespaces,
                policy.clone(),
                max_type_expansions,
            );
            {
                let _timer = crate::performance::timer(crate::performance::Stage::Bind);
                checker.bind();
            }
            (id.clone(), checker.exported_values(true))
        })
        .collect();
    let mut checked_modules: BTreeMap<String, CheckedModule> = BTreeMap::new();
    // What each checked module exports as a namespace, for the modules that
    // import it; modules are checked after the modules they import.
    let mut namespace_exports: NamespaceExports = BTreeMap::new();

    for module_id in dependency_order(project) {
        let module = &project.modules[&module_id];
        let module_id = &module_id;
        if !rechecked.contains(module_id) {
            if let Some(previous) = previous.and_then(|previous| previous.modules.get(module_id)) {
                namespace_exports.insert(module_id.clone(), previous.namespace_exports.clone());
                exports
                    .values
                    .insert(module_id.clone(), previous.value_exports.clone());
                checked_modules.insert(module_id.clone(), previous.clone());
                continue;
            }
        }
        let mut checker = module::ModuleChecker::new(
            project,
            module,
            &exports,
            (!project.ambient_declaration_modules.contains(module_id)).then_some(&ambient),
            &namespace_exports,
            policy.clone(),
            max_type_expansions,
        );
        {
            let _timer = crate::performance::timer(crate::performance::Stage::Bind);
            checker.bind();
        }
        if policy.enforce_types {
            checker.check_names();
            checker.check_types();
            checker.dedupe_name_diagnostics();
        }
        let exported_namespaces = checker.exported_namespaces();
        let value_exports = checker.exported_values(false);
        let inferred_returns = checker.inferred_return_types();
        let inferred_parameters = checker.inferred_parameter_types();
        let class_expression_surfaces = checker.class_expression_surfaces();
        diagnostics.extend(checker.diagnostics);
        let symbols = checker.symbols;
        namespace_exports.insert(module_id.clone(), exported_namespaces.clone());
        exports
            .values
            .insert(module_id.clone(), value_exports.clone());
        checked_modules.insert(
            module_id.clone(),
            CheckedModule {
                module: module.clone(),
                symbols,
                inferred_returns,
                inferred_parameters,
                class_expression_surfaces,
                namespace_exports: exported_namespaces,
                value_exports,
            },
        );
    }
    (
        CheckedProject {
            modules: checked_modules,
        },
        diagnostics,
    )
}

/// The modules of a project with every module after the ones it imports (a
/// cycle is broken where it is first entered).
fn dependency_order(project: &Project) -> Vec<String> {
    fn visit(project: &Project, id: &str, seen: &mut BTreeSet<String>, order: &mut Vec<String>) {
        if !project.modules.contains_key(id) || !seen.insert(id.to_string()) {
            return;
        }
        for ((from, _), resolved) in &project.resolutions {
            if from == id {
                visit(project, resolved, seen, order);
            }
        }
        order.push(id.to_string());
    }
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for id in project.modules.keys() {
        visit(project, id, &mut seen, &mut order);
    }
    order
}
