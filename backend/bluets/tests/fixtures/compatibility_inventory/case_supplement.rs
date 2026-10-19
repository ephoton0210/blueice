// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Additional input/configuration probes outside a suite's primary record.
//! Comparing the two output directories is one probe, not two programs.

use super::case_inline::{array, array_cases, product};
use super::case_sources::{Case, Comparison};
use super::inventory::Suite;
use super::rust_facts::{self, Facts};
use super::snapshot::{Result, Snapshot};

fn one(source: &str, facts: &Facts, scope: &str) -> Result<Vec<Case>> {
    if !facts.ignored_tests.iter().any(|name| name == scope) {
        return Err(format!("missing single-program comparison block: {scope}").into());
    }
    Ok(vec![Case::new(
        source,
        &format!("function:{scope}"),
        scope,
        "program-probe",
    )])
}

pub fn additional(snapshot: &Snapshot, suite: &Suite) -> Result<Vec<Case>> {
    let source = &suite.harness;
    let stem = source
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".rs"))
        .ok_or("invalid harness")?;
    let facts = rust_facts::parse(snapshot.source(source)?)?;
    let named_array = |scope: &str| array_cases(source, array(&facts, scope, "for@0")?);
    let mut cases = match stem {
        "gc1_explain_files" => {
            let scope = "explain_files_does_not_follow_or_describe_outside_owner_input";
            if !facts.calls.iter().any(|call| call.scope == scope) {
                return Err("missing G-C1 owner-boundary control".into());
            }
            let mut case = Case::new(source, &format!("function:{scope}"), scope, "owner-control");
            case.comparison = Comparison::BoundaryControl;
            case.reason = Some("The sibling input is not authorized: this checks BlueTS confinement, not TypeScript agreement.".into());
            vec![case]
        }
        "unknown_name_checker_matrix" => one(
            source,
            &facts,
            "resolved_scopes_preserve_execution_and_declarations",
        )?,
        "immutable_checker_matrix" => one(
            source,
            &facts,
            "mutable_program_preserves_execution_and_declarations",
        )?,
        "standard_library_checker_matrix" => [
            one(source, &facts, "library_calls_preserve_node_execution")?,
            named_array("library_inferred_declarations_match_typescript")?,
        ]
        .concat(),
        "inferred_return_checker_matrix" => [
            one(source, &facts, "inferred_returns_preserve_node_execution")?,
            named_array("inferred_return_declarations_match_typescript")?,
            named_array("union_declarations_have_the_same_types_despite_literal_intern_order")?,
        ]
        .concat(),
        "imported_value_checker_matrix" => [
            named_array("imported_values_preserve_execution_and_declarations")?,
            named_array("inferred_import_declarations_match_typescript")?,
        ]
        .concat(),
        "strictness_flags" => {
            let scope = "strict_and_relaxed_valid_programs_print_the_same_result";
            product(
                source,
                one(source, &facts, scope)?,
                &[array(&facts, scope, "for@0")?],
            )?
        }
        "project_config" => {
            let mut result = one(
                source,
                &facts,
                "configuration_builds_match_node_and_exact_declarations",
            )?;
            let mut owned = one(
                source,
                &facts,
                "strict_runtime_crossings_keep_their_audit_after_output_relocation",
            )?;
            for case in &mut owned {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS owner-selected strict-runtime boundary and relocated helper; no TypeScript comparison is performed by this block.".into());
            }
            result.extend(owned);
            result
        }
        "project_build_invalidation" => {
            let mut result = named_array("pinned_scope_edits_leave_stale_output_without_force")?;
            for case in &mut result {
                case.comparison = Comparison::ReferenceOnly;
                case.reason = Some("Native positive control of stale default TypeScript reuse and --force recovery; not a direct BlueTS agreement.".into());
            }
            result
        }
        "emitted_target_editions" => {
            if !facts.constants.contains_key("OPAQUE_GENERATOR") {
                return Err("missing opaque-generator program".into());
            }
            let mut case = Case::new(
                source,
                "const:OPAQUE_GENERATOR",
                "opaque-generator-es5",
                "program",
            );
            case.comparison = Comparison::Refused;
            case.reason = Some("Pinned TypeScript accepts and executes the ES5 suspension graph; BlueTS deliberately refuses unsupported lowering.".into());
            vec![case]
        }
        _ => Vec::new(),
    };
    for case in &mut cases {
        if case.comparison == Comparison::Compared {
            case.unit = "supplemental-program/configuration-probe".into();
        }
    }
    Ok(cases)
}

pub fn configurations(snapshot: &Snapshot, suite: &Suite, primary: Vec<Case>) -> Result<Vec<Case>> {
    let source = &suite.harness;
    if source.ends_with("/source_map_publication.rs") {
        let facts = rust_facts::parse(snapshot.source(source)?)?;
        return product(
            source,
            primary,
            &[array(
                &facts,
                "external_and_inline_source_maps_match_public_typescript_output",
                "for@1",
            )?],
        );
    }
    if source.ends_with("/source_map_tokens_oracle.rs") {
        let mut expanded = Vec::new();
        for base in primary {
            // The frozen matrix declares inline_sources=both. Each boolean
            // setting is compiled by both front ends; map-off/repeat checks
            // and extra frame witnesses reuse those inputs, not new cases.
            let fields = snapshot
                .source(&base.source)?
                .lines()
                .nth(
                    base.selector
                        .strip_prefix("line:")
                        .ok_or("invalid map row")?
                        .parse::<usize>()?
                        - 1,
                )
                .ok_or("missing map row")?
                .split('\t')
                .collect::<Vec<_>>();
            if fields.get(9) != Some(&"both") {
                return Err("unhandled source-map inline-source domain".into());
            }
            for inline in [false, true] {
                expanded.push(Case::new(
                    &base.source,
                    &format!("{};inline_sources={inline}", base.selector),
                    &format!("{};inline_sources={inline}", base.label),
                    "configuration",
                ));
            }
        }
        return Ok(expanded);
    }
    Ok(primary)
}
