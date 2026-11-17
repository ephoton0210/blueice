// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native project checking and emission share one complete root generation.

use super::*;
use crate::program as owner_program;

pub(super) fn compile_project(
    invocation: &Invocation,
    loader: &FileLoader,
    emit_errors: bool,
) -> CompiledProject {
    let roots = owner_program::selected_roots(&invocation.entries, loader);
    let options = owner_program::options(&invocation.entries, &roots, invocation.options.clone());
    let mut result = compile_program(&roots, loader, options.clone());
    result.diagnostics.extend(owner_program::layout_diagnostics(
        invocation,
        &result.project,
        loader,
    ));
    let mut sources = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut source_ids = Vec::new();
    let mut emit_blocked = false;
    let mut statistics = statistics::Statistics::default();
    let mut explanations = explanations::Explanations::default();
    let explain_files = invocation
        .project_config
        .as_ref()
        .is_some_and(|config| config.options.get("explainFiles") == Some(&Value::Bool(true)));
    if explain_files {
        explanations.collect(&result.project, invocation, loader);
    }
    statistics.add(&result);
    if emit_errors && result.has_errors() && result.output.is_none() {
        let started = std::time::Instant::now();
        match result.emit_for_native_cli(&options) {
            Ok(Some(output)) => result.output = Some(output),
            Ok(None) => emit_blocked = true,
            Err(diagnostic) => {
                result.diagnostics.push(diagnostic);
                emit_blocked = true;
            }
        }
        statistics.extra_emit(started.elapsed());
    }
    for (id, module) in &result.project.modules {
        sources.insert(id.clone(), module.source.clone());
    }
    for diagnostic in &result.diagnostics {
        if let Some(source) = result.project.source(&diagnostic.span.module) {
            sources
                .entry(diagnostic.span.module.clone())
                .or_insert_with(|| source.into());
        }
    }
    let has_errors = result.has_errors();
    declaration_diagnostics::qualify(&mut result.diagnostics, &result.project, loader);
    for root in roots.iter().chain(result.project.modules.keys()) {
        ordered_sources(root, &result.project, &mut seen, &mut source_ids);
    }
    let files = source_ids
        .iter()
        .filter_map(|module| loader.source_path(module).ok())
        .collect();
    let fingerprint = fingerprint_entries(&[result.project_fingerprint]);
    let module_count = result.project.modules.len();
    let (artifacts, declaration_modules, assets) = result
        .output
        .map(|output| (output.artifacts, output.declaration_modules, output.assets))
        .unwrap_or_default();
    CompiledProject {
        summary: CompileSummary {
            artifacts,
            declaration_modules,
            assets,
            fingerprint,
            module_count,
            has_errors,
        },
        files,
        diagnostics: result.diagnostics,
        sources,
        emit_blocked,
        statistics,
        explanations: if explain_files {
            explanations.render(invocation, loader)
        } else {
            String::new()
        },
    }
}
