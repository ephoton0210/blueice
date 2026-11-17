// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Both CLI surfaces select one owner-authorized root program.

use super::*;
#[path = "program/root_layout.rs"]
mod root_layout;
pub(super) use root_layout::diagnostics as layout_diagnostics;

pub(super) fn selected_roots(entries: &[PathBuf], loader: &FileLoader) -> Vec<String> {
    entries
        .iter()
        .map(|entry| {
            loader
                .module_id(entry)
                .expect("entry has an authorized identity")
        })
        .collect()
}

pub(super) fn options(
    entries: &[PathBuf],
    roots: &[String],
    mut options: CompilerOptions,
) -> CompilerOptions {
    options
        .ambient_declaration_modules
        .retain(|source| !roots.contains(&source.id));
    if entries.iter().all(|entry| is_declaration_path(entry)) {
        if options.runtime_policy == RuntimePolicy::StrictRuntime {
            options.runtime_policy = RuntimePolicy::Checked;
        }
        options.strict_runtime_boundaries.clear();
    }
    options
}

pub(super) fn compile_entries(
    invocation: &Invocation,
    loader: &FileLoader,
    diagnostics_json: bool,
) -> CompileSummary {
    let roots = selected_roots(&invocation.entries, loader);
    let options = options(&invocation.entries, &roots, invocation.options.clone());
    let mut result = compile_program(&roots, loader, options);
    result
        .diagnostics
        .extend(layout_diagnostics(invocation, &result.project, loader));
    for diagnostic in &result.diagnostics {
        if diagnostics_json {
            eprintln!("{}", diagnostic.to_json());
        } else {
            eprintln!(
                "{}:{}:{}: {}: {}",
                diagnostic.span.module,
                diagnostic.span.start,
                diagnostic.span.end,
                diagnostic.code,
                diagnostic.message
            );
        }
    }
    let has_errors = result.has_errors();
    let fingerprint = fingerprint_entries(&[result.project_fingerprint]);
    let module_count = result.project.modules.len();
    let (artifacts, declaration_modules, assets) = result
        .output
        .map(|output| (output.artifacts, output.declaration_modules, output.assets))
        .unwrap_or_default();
    CompileSummary {
        artifacts,
        declaration_modules,
        assets,
        fingerprint,
        module_count,
        has_errors,
    }
}
