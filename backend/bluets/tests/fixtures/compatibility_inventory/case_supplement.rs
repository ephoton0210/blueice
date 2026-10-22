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
        "gt7_library_manifest" => {
            let path = "backend/bluets/tests/fixtures/gt7_library_manifest/reference.json";
            let mut controls = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/profile_controls",
                "target/library-configuration",
            )?;
            let mut owner = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/owner_controls",
                "owner-library-control",
            )?;
            for case in &mut owner {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("Owner replacement, default-denied runtime globals, static source confinement, content/version identity and configured generic-work limits are owner controls, not extra native TypeScript agreements.".into());
            }
            controls.append(&mut owner);
            controls
        }
        "gt8_return_context" => {
            let path = "backend/bluets/tests/fixtures/gt8_return_context/reference.json";
            let mut controls = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/budget_controls",
                "owner-budget-control",
            )?;
            for case in &mut controls {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS's configured return-expansion and parser-depth controls; native default-budget acceptance is not agreement on owner-selected limits.".into());
            }
            let mut owner = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/owner_controls",
                "owner-declaration-control",
            )?;
            for case in &mut owner {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("Static declaration queries do not authorize a runtime counterpart; strict and legacy value-import denial and declaration-class body/initializer guards are owner controls, not extra native agreements.".into());
            }
            controls.extend(owner);
            controls
        }
        "gt4_overload_chains" => {
            let path = "backend/bluets/tests/fixtures/gt4_overload_chains/reference.json";
            let mut controls = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/budget_controls",
                "owner-budget-control",
            )?;
            for case in &mut controls {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS's below/above overload-chain work and parser-depth limits; native default-budget acceptance is not agreement on owner-selected limits.".into());
            }
            let mut readonly = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/readonly_controls",
                "owner-readonly-control",
            )?;
            for case in &mut readonly {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS retains owner readonly qualifiers through optional/non-null calls and never assertions in strict and legacy modes; native cast acceptance is not agreement on the owner policy.".into());
            }
            controls.extend(readonly);
            controls
        }
        "gt6_structural" => {
            let path = "backend/bluets/tests/fixtures/gt6_structural/reference.json";
            let mut controls = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/budget_controls",
                "owner-budget-control",
            )?;
            for case in &mut controls {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS's below/above nested structural work and parser-depth limits; native default-budget acceptance is not agreement on owner-selected limits.".into());
            }
            controls
        }
        "gt5_compositions" => {
            let path = "backend/bluets/tests/fixtures/gt5_compositions/reference.json";
            let mut controls = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/budget_controls",
                "owner-budget-control",
            )?;
            for case in &mut controls {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS's below/above tuple-composition expansion and parser-depth limits; default native acceptance is not agreement on owner-selected budgets.".into());
            }
            let mut readonly = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/readonly_controls",
                "owner-readonly-control",
            )?;
            for case in &mut readonly {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS deliberately retains runtime readonly/opaque qualifiers through never assertions, in strict and legacy modes; native acceptance of these casts is not TypeScript agreement on the owner policy.".into());
            }
            controls.extend(readonly);
            controls
        }
        "gt2_inference" => {
            let path = "backend/bluets/tests/fixtures/gt2_inference/reference.json";
            let mut controls = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/budget_controls",
                "owner-budget-control",
            )?;
            for case in &mut controls {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS's below/above generic-inference work and parser-depth budgets; default native acceptance is not agreement on owner-selected limits.".into());
            }
            controls
        }
        "gt3_operators" => {
            let path = "backend/bluets/tests/fixtures/gt3_operators/reference.json";
            let mut controls = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/budget_controls",
                "owner-budget-control",
            )?;
            for case in &mut controls {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("BlueTS's below/above type-expansion and parser-depth budgets; native acceptance at its default budget is not agreement on BlueTS limits.".into());
            }
            let path = "backend/bluets/tests/fixtures/gt3_operators/key-profiles.json";
            let mut profiles = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/controls",
                "native-key-profile",
            )?;
            for case in &mut profiles {
                case.comparison = Comparison::ReferenceOnly;
                case.reason = Some("Pinned native primitive/library key sets and regular literal order, independently reproduced; not an extra public compiler agreement.".into());
            }
            controls.extend(profiles);
            let path = "backend/bluets/tests/fixtures/gt3_operators/cases.json";
            let mut authority = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/capability_controls",
                "owner-authority-control",
            )?;
            for case in &mut authority {
                case.comparison = Comparison::BoundaryControl;
                case.reason = Some("Key facts install no runtime global; an explicitly supplied owner declaration is still required for the call. This is an authority control, not TypeScript agreement.".into());
            }
            controls.extend(authority);
            controls
        }
        "gt1_declaration_order" => {
            let path = "backend/bluets/tests/fixtures/gt1_declaration_order/reference.json";
            let mut controls = super::case_sources::json_cases(
                path,
                snapshot.source(path)?,
                "/profile_controls",
                "native-library-profile",
            )?;
            for case in &mut controls {
                case.comparison = Comparison::ReferenceOnly;
                case.reason = Some("Pinned native library fresh-literal allocation facts; independently reproduced, not additional public compiler agreement.".into());
            }
            controls
        }
        "gt1_flow" => {
            let scope = "finalizer_duplication_respects_the_owner_work_budget";
            if !facts.calls.iter().any(|call| call.scope == scope) {
                return Err("missing G-T1 owner-work-budget control".into());
            }
            let mut case = Case::new(source, &format!("function:{scope}"), scope, "owner-control");
            case.comparison = Comparison::BoundaryControl;
            case.reason = Some("BlueTS's below/above owner work budget for finalizer graph expansion; not TypeScript agreement.".into());
            let guard = "union_order::union_order_guard_rejects_every_non_permutation_change";
            let support = "backend/bluets/tests/oracle_support/union_order.rs";
            let guard_facts = rust_facts::parse(snapshot.source(support)?)?;
            let mutations = array(
                &guard_facts,
                "union_order_guard_rejects_every_non_permutation_change",
                "for@0",
            )?;
            if mutations.labels.is_empty() {
                return Err("missing G-T1 union-order boundary control".into());
            }
            let mut guard_case = Case::new(
                support,
                "function:union_order_guard_rejects_every_non_permutation_change",
                guard,
                "owner-control",
            );
            guard_case.comparison = Comparison::BoundaryControl;
            guard_case.reason = Some("BlueTS's bounded declaration comparator rejects member, grouping and non-union mutations; not an extra native program agreement.".into());
            vec![case, guard_case]
        }
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
