// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admission of owner-selected emitted runtime crossing descriptors.

use crate::compiler::{is_declaration_module, CompilerOptions, Project, RuntimePolicy};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::parser::Declaration;
use std::collections::BTreeSet;

pub(crate) const HELPER_V1_VERSION: &str = "bluets-runtime-helper-v1";
const MAX_SAFE_INTEGER: usize = 9_007_199_254_740_991;

pub(crate) fn validate_descriptors(
    project: &Project,
    options: &CompilerOptions,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut contract_ids = BTreeSet::new();
    let mut selected_functions = BTreeSet::new();
    for boundary in &options.strict_runtime_boundaries {
        let reject = |message: &str| {
            Diagnostic::error(
                DiagnosticCode::InvalidContract,
                boundary.span.clone(),
                message,
            )
        };
        if options.runtime_policy != RuntimePolicy::StrictRuntime {
            diagnostics.push(reject("emitted boundary requires strict-runtime policy"));
        }
        if boundary.helper_version != HELPER_V1_VERSION {
            diagnostics.push(reject("unsupported emitted runtime helper version"));
        }
        if boundary.max_string_bytes > MAX_SAFE_INTEGER {
            diagnostics.push(reject(
                "string-byte limit exceeds helper v1's safe integer range",
            ));
        }
        if boundary.contract_id.is_empty()
            || boundary.contract_id.len() > 128
            || !boundary
                .contract_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            diagnostics.push(reject("invalid emitted contract ID"));
        }
        if !contract_ids.insert(boundary.contract_id.as_str()) {
            diagnostics.push(reject("duplicate emitted contract ID"));
        }
        if !selected_functions.insert((&boundary.span.module, &boundary.function)) {
            diagnostics.push(reject("duplicate emitted function boundary"));
        }
        let matched = !is_declaration_module(&boundary.span.module)
            && project
                .modules
                .get(&boundary.span.module)
                .is_some_and(|module| {
                    module.declarations.iter().any(|declaration| {
                        matches!(declaration, Declaration::Function(function)
                            if function.name == boundary.function
                                && function.exported
                                && !function.default_export
                                && !function.declared
                                && !function.overload
                                && function.span == boundary.span)
                    })
                });
        if !matched {
            diagnostics.push(reject(
                "emitted boundary does not match an exported function at the exact source span",
            ));
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{compile, MapLoader, ModuleSource, StrictRuntimeBoundary};
    use crate::diagnostic::SourceSpan;

    const MODULE: &str = "memory:///main.ts";
    const SOURCE: &str = "export function echo(value: string): string { return value; }\n";

    fn boundary() -> StrictRuntimeBoundary {
        StrictRuntimeBoundary {
            contract_id: "echo-string-v1".to_string(),
            function: "echo".to_string(),
            span: SourceSpan::new(MODULE, 0, SOURCE.find('}').unwrap() + 1),
            max_string_bytes: 64,
            helper_version: HELPER_V1_VERSION.to_string(),
        }
    }

    fn compile_with(
        boundaries: Vec<StrictRuntimeBoundary>,
        policy: RuntimePolicy,
    ) -> crate::Compilation {
        let loader = MapLoader::from([ModuleSource::new(MODULE, SOURCE)]);
        compile(
            MODULE,
            &loader,
            CompilerOptions {
                runtime_policy: policy,
                strict_runtime_boundaries: boundaries,
                ..CompilerOptions::default()
            },
        )
    }

    #[test]
    fn exact_owner_descriptor_is_fingerprinted_and_stale_or_duplicate_descriptors_refuse() {
        let valid = compile_with(vec![boundary()], RuntimePolicy::StrictRuntime);
        assert!(!valid.has_errors(), "{:?}", valid.diagnostics);
        let first_fingerprint = valid.output.unwrap().fingerprint;
        let mut changed_budget = boundary();
        changed_budget.max_string_bytes = 63;
        let changed = compile_with(vec![changed_budget], RuntimePolicy::StrictRuntime);
        assert!(!changed.has_errors(), "{:?}", changed.diagnostics);
        assert_ne!(first_fingerprint, changed.output.unwrap().fingerprint);

        let mut stale = boundary();
        stale.span.end -= 1;
        let rejected = compile_with(vec![stale], RuntimePolicy::StrictRuntime);
        assert!(rejected.has_errors());
        assert!(rejected.output.is_none());
        assert!(rejected.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidContract
                && diagnostic.message.contains("exact source span")
        }));

        let duplicate = compile_with(vec![boundary(), boundary()], RuntimePolicy::StrictRuntime);
        assert!(duplicate.output.is_none());
        assert!(duplicate
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("duplicate emitted contract ID") }));
    }

    #[test]
    fn wrong_policy_helper_and_limit_refuse_before_emission() {
        let wrong_policy = compile_with(vec![boundary()], RuntimePolicy::Checked);
        assert!(wrong_policy.output.is_none());
        assert!(wrong_policy.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("requires strict-runtime policy")
        }));

        let mut wrong_helper = boundary();
        wrong_helper.helper_version = "bluets-runtime-helper-v2".to_string();
        let refused = compile_with(vec![wrong_helper], RuntimePolicy::StrictRuntime);
        assert!(refused.output.is_none());

        let mut invalid_limit = boundary();
        invalid_limit.max_string_bytes = MAX_SAFE_INTEGER + 1;
        let refused = compile_with(vec![invalid_limit], RuntimePolicy::StrictRuntime);
        assert!(refused.output.is_none());
    }
}
