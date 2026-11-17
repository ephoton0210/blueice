// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Selection preflight never invokes the caller's loader or resolver.

use super::*;

pub(super) fn refuse_invalid(
    roots: &[String],
    options: &CompilerOptions,
) -> Option<IncrementalResult> {
    let entry = roots.first().map_or("", String::as_str);
    let problem = if roots.len() > options.limits.max_modules {
        Some((
            DiagnosticCode::ResourceLimit,
            format!(
                "program roots exceed the {} module limit",
                options.limits.max_modules
            ),
        ))
    } else if roots.is_empty() {
        Some((
            DiagnosticCode::ModuleNotFound,
            "program requires at least one selected root".into(),
        ))
    } else if roots
        .iter()
        .any(|root| root.is_empty() || root.contains('\0'))
    {
        Some((
            DiagnosticCode::ModuleNotFound,
            "program root identities must be nonempty and contain no NUL".into(),
        ))
    } else {
        let mut seen = BTreeSet::new();
        roots
            .iter()
            .find(|root| !seen.insert(root.as_str()))
            .map(|_| {
                (
                    DiagnosticCode::ModuleNotFound,
                    "program root identities must be unique".into(),
                )
            })
    };
    problem.map(|(code, message)| {
        let project = Project::empty(entry);
        let project_fingerprint = fingerprint(&project, options);
        IncrementalResult {
            compilation: Compilation {
                project,
                project_fingerprint,
                checked: None,
                debug_info: None,
                diagnostics: vec![Diagnostic::error(
                    code,
                    SourceSpan::new(entry, 0, 0),
                    message,
                )],
                output: None,
                performance: crate::CompilerPerformance::default(),
            },
            cache_hit: false,
            parsed_modules: BTreeSet::new(),
            reused_parsed_modules: BTreeSet::new(),
            rechecked_modules: BTreeSet::new(),
            reused_checked_modules: BTreeSet::new(),
        }
    })
}
