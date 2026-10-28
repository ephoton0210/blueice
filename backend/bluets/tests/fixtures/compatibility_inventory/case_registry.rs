// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Semantic source adapters. Paths/selectors are explicit; counts never are.

use super::case_sources::{self, Case, Comparison};
use super::inventory::Suite;
use super::rust_facts;
use super::snapshot::{Result, Snapshot};
use serde_json::Value;

pub fn primary(snapshot: &Snapshot, suite: &Suite) -> Result<Option<Vec<Case>>> {
    let file = suite.harness.rsplit('/').next().ok_or("invalid harness")?;
    let stem = file.strip_suffix(".rs").ok_or("not a Rust harness")?;
    if stem == "gc1_source_diagnostics" {
        let path = "backend/bluets/tests/fixtures/diagnostics/source-families.json";
        return gc1_source_cases(
            snapshot.source(&suite.harness)?,
            snapshot.source(path)?,
            snapshot.source("backend/bluets/tests/fixtures/gc1_source_controls/reference.json")?,
        )
        .map(Some);
    }
    if let Some(relative) = json_record(stem) {
        let path = format!("backend/bluets/tests/fixtures/{relative}");
        let text = snapshot.source(&path)?;
        let root: Value = serde_json::from_str(text)?;
        let pointer = if root.is_array() {
            ""
        } else if root.get("cases").is_some() {
            "/cases"
        } else {
            "/observations"
        };
        let mut cases = case_sources::json_cases(&path, text, pointer, "program/configuration")?;
        if stem == "gt9_classes" {
            let defects = root["cases"]
                .as_array()
                .ok_or("missing G-T9 rows")?
                .iter()
                .filter(|row| row["nativeRuntimeDefect"] == true)
                .map(|row| {
                    row["id"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("invalid G-T9 runtime defect")
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if defects.len() != 2 {
                return Err("incomplete G-T9 native runtime defect references".into());
            }
            case_sources::classify(&mut cases,&defects,"",Comparison::ReferenceOnly,"Pinned TypeScript 5.9.3 define-mode branch-super output throws ReferenceError before super; BlueTS refuses publication. Native declarations/execution are references, not TypeScript agreement.")?;
        }
        if stem == "gt1_flow" {
            let facts = rust_facts::parse(snapshot.source(&suite.harness)?)?;
            // Historical stage-one snapshots retain their real allowance.
            let arrays: Vec<_> = facts
                .arrays
                .iter()
                .filter(|array| array.binding == "ORDER_ONLY_CASES")
                .collect();
            if arrays.len() > 1 {
                return Err("ambiguous G-T1 order allowance".into());
            }
            if let Some(order_only) = arrays.first() {
                case_sources::classify(&mut cases, &order_only.labels, "", Comparison::Open,
                    "Stage-one native type equivalence and literal-union permutation agree, but raw .d.ts member order differs; exact output is still required by stage two.")?;
            }
        }
        if stem == "emitted_target_editions" {
            let rows = root
                .pointer(pointer)
                .and_then(Value::as_array)
                .ok_or("missing target rows")?;
            cases = cases
                .into_iter()
                .zip(rows)
                .filter_map(|(case, row)| (row["accepts"] == true).then_some(case))
                .collect();
        }
        if stem == "diagnostic_source_families" {
            let facts = rust_facts::parse(snapshot.source(&suite.harness)?)?;
            for (binding, comparison, reason) in [
                (
                    "known_checker_gaps",
                    Comparison::Open,
                    "checker returns no diagnostic for this recorded TypeScript error",
                ),
                (
                    "subset_refusals",
                    Comparison::Refused,
                    "explicit subset refusal has no TypeScript diagnostic counterpart",
                ),
            ] {
                let arrays: Vec<_> = facts
                    .arrays
                    .iter()
                    .filter(|array| array.binding == binding)
                    .collect();
                if arrays.len() > 1 {
                    return Err(format!("ambiguous classification array: {binding}").into());
                }
                // A repaired public-boundary test no longer has an allowance
                // array. Historical snapshots retain their actual exclusions.
                if let Some(array) = arrays.first() {
                    case_sources::classify(
                        &mut cases,
                        &array.labels,
                        "diagnostics-other-",
                        comparison,
                        reason,
                    )?;
                }
            }
        }
        if stem == "diagnostics_matrix" {
            let path = "backend/bluets/tests/fixtures/diagnostics/no-typescript-counterpart.json";
            let reasons: Value = serde_json::from_str(snapshot.source(path)?)?;
            for row in reasons
                .as_array()
                .ok_or("missing counterpart classifications")?
            {
                let id = row["id"].as_str().ok_or("missing counterpart case id")?;
                let reason = row["reason"].as_str().ok_or("missing counterpart reason")?;
                case_sources::classify(
                    &mut cases,
                    &[id.to_string()],
                    "",
                    Comparison::Refused,
                    reason,
                )?;
            }
        }
        return Ok(Some(cases));
    }
    if tsv_suite(stem) {
        let paths: Vec<_> = suite
            .inputs
            .keys()
            .filter(|path| path.ends_with(".tsv"))
            .collect();
        if paths.len() != 1 {
            return Err(format!("missing/ambiguous primary matrix: {}", suite.harness).into());
        }
        let path = paths[0];
        let mut cases = case_sources::tsv_cases(
            path,
            snapshot.source(path)?,
            stem.starts_with("source_map_"),
            "program/configuration",
        )?;
        if stem == "project_reference_owner_gate" {
            case_sources::classify(&mut cases, &["symlink-escape".into()], "", Comparison::Refused,
                "TypeScript follows the reference; the canonical BlueTS owner boundary deliberately refuses it")?;
        }
        for path in suite
            .inputs
            .keys()
            .filter(|path| path.ends_with("-deferred.txt"))
        {
            let labels = snapshot
                .source(path)?
                .lines()
                .filter(|line| !line.is_empty())
                .map(String::from)
                .collect::<Vec<_>>();
            case_sources::classify(&mut cases, &labels, "", Comparison::Refused,
                "versioned deferred entry: TypeScript accepts this input but BlueTS explicitly refuses it")?;
        }
        if stem == "project_build_invalidation" {
            for (case, line) in cases.iter_mut().zip(snapshot.source(path)?.lines()) {
                let observations = line.split_once('\t').ok_or("invalid invalidation row")?.1;
                // assert_public() requires owner-current rebuilds. The native
                // table deliberately records stale scope/symlink outputs.
                let required = if case.label == "unrelated" {
                    "-"
                } else {
                    "tsconfig.json"
                };
                if observations.split('|').skip(1).any(|row| row != required) {
                    case.comparison = Comparison::Open;
                    case.reason = Some("BlueTS invalidates owner-current scope/symlink inputs while default TypeScript build reuse keeps these recorded outputs stale; rebuild policy is intentionally different.".into());
                }
            }
        }
        return Ok(Some(cases));
    }
    Ok(None)
}

fn gc1_source_cases(
    harness: &str,
    source_record: &str,
    controls_record: &str,
) -> Result<Vec<Case>> {
    let facts = rust_facts::parse(harness)?;
    let selected = super::case_inline::array(&facts, "", "CASES")?;
    if selected.labels.is_empty() {
        return Err("empty G-C1 source selection".into());
    }
    let path = "backend/bluets/tests/fixtures/diagnostics/source-families.json";
    let mut cases =
        case_sources::json_cases(path, source_record, "/cases", "program/configuration")?;
    cases.retain(|case| {
        selected
            .labels
            .iter()
            .any(|name| case.label == format!("diagnostics-other-{name}"))
    });
    if cases.len() != selected.labels.len() {
        return Err("G-C1 selected source observations are missing or ambiguous".into());
    }
    let path = "backend/bluets/tests/fixtures/gc1_source_controls/reference.json";
    cases.extend(case_sources::json_cases(
        path,
        controls_record,
        "/cases",
        "program/configuration",
    )?);
    Ok(cases)
}

pub fn cases(snapshot: &Snapshot, suite: &Suite) -> Result<Vec<Case>> {
    let primary = match primary(snapshot, suite)? {
        Some(cases) => cases,
        None => super::case_inline::programs(snapshot, suite)?
            .ok_or_else(|| format!("no case provider for {}", suite.harness))?,
    };
    let mut result = super::case_supplement::configurations(snapshot, suite, primary)?;
    result.extend(super::case_supplement::additional(snapshot, suite)?);
    Ok(result)
}

/// Development audit only: structured primary data is not yet the complete
/// per-suite program list. Supplemental and inline providers must be audited
/// before a pass-rate artifact can be published.
pub fn audit(snapshot: &Snapshot, suites: &[Suite]) -> Result<Value> {
    let mut rows = Vec::new();
    let mut missing = Vec::new();
    for suite in suites {
        match cases(snapshot, suite) {
            Ok(cases) => {
                rows.push(serde_json::json!({"harness":suite.harness,"primary_observations":cases.len(),
                "compared":cases.iter().filter(|case| case.comparison==Comparison::Compared).count(),
                "open":cases.iter().filter(|case| case.comparison==Comparison::Open).count(),
                "reference_only":cases.iter().filter(|case| case.comparison==Comparison::ReferenceOnly).count(),
                "boundary_controls":cases.iter().filter(|case| case.comparison==Comparison::BoundaryControl).count(),
                "refused":cases.iter().filter(|case| case.comparison==Comparison::Refused).count()}));
            }
            Err(error) => {
                missing
                    .push(serde_json::json!({"harness":suite.harness,"error":error.to_string()}));
            }
        }
    }
    Ok(
        serde_json::json!({"source_revision":snapshot.revision,"primary_sources":rows,
        "requires_inline_provider":missing,"publishable":false,
        "note":"Primary-source audit only; supplemental programs/configuration axes and measurement binding remain required."}),
    )
}

fn json_record(stem: &str) -> Option<&'static str> {
    Some(match stem {
        "ambient_checker_matrix" => "typescript_oracle/ambient-reference.json",
        "async_generator_context" => "async-generator-context-reference.json",
        "catch_flow_context" => "catch-flow-context-reference.json",
        "class_dynamic_checker_matrix" => "typescript_oracle/class-dynamic-reference.json",
        "class_modifiers" => "typescript_oracle/modifiers-constructor-carriers.json",
        "class_modifiers_checker_matrix" => "typescript_oracle/class-modifiers-reference.json",
        "class_retirement_checker_matrix" => "typescript_oracle/class-retirement-reference.json",
        "cli_surface" => "cli_surface/reference.json",
        "compatibility_checker_matrix" => "typescript_oracle/compatibility-reference.json",
        "declaration_inference" => "declaration_inference/reference.json",
        "declaration_options" => "declaration_options/reference.json",
        "decorator_residuals" => "decorator_residuals/reference.json",
        "decorator_targets" => "decorator_targets/reference.json",
        "decorator_typing" => "decorator_typing/reference.json",
        "diagnostic_source_families" => "diagnostics/source-families.json",
        "gc1_explain_files" => "gc1_explain/reference.json",
        "diagnostics_matrix" => "diagnostics/reference.json",
        "diagnostics_presentation" => "diagnostics/presentation-reference.json",
        "diagnostics_projects" => "diagnostics/project-positions.json",
        "explicit_this_context" => "explicit-this-context-reference.json",
        "guards_checker_matrix" => "typescript_oracle/guards-reference.json",
        "gt1_flow" => "gt1_flow/reference.json",
        "gt2_inference" => "gt2_inference/reference.json",
        "gt5_compositions" => "gt5_compositions/reference.json",
        "gt6_structural" => "gt6_structural/reference.json",
        "gt4_overload_chains" => "gt4_overload_chains/reference.json",
        "gt8_return_context" => "gt8_return_context/reference.json",
        "gt7_library_manifest" => "gt7_library_manifest/reference.json",
        "gt10_import_types" => "gt10_import_types/reference.json",
        "gt9_classes" => "gt9_classes/reference.json",
        "gm1_resolver_compositions" => "gm1_resolver_compositions/reference.json",
        "gt3_operators" => "gt3_operators/reference.json",
        "gt1_declaration_order" => "gt1_declaration_order/reference.json",
        "indexed_call_context" => "indexed-call-context-reference.json",
        "inference_checker_matrix" => "typescript_oracle/inference-reference.json",
        "jsx_runtime_namespace" => "jsx_runtime_namespace/reference.json",
        "jsx_typing_extras" => "jsx_typing_extras/reference.json",
        "module_exports_checker_matrix" => "typescript_oracle/module-exports-reference.json",
        "module_script_helpers" => "module_script_helpers/reference.json",
        "module_systems" => "module_systems/reference.json",
        "module_types_checker_matrix" => "typescript_oracle/module-types-reference.json",
        "more_types_checker_matrix" => "typescript_oracle/more_types-reference.json",
        "narrowing_checker_matrix" => "typescript_oracle/narrowing-reference.json",
        "object_method_context" => "object-method-context-reference.json",
        "operators_checker_matrix" => "typescript_oracle/operators-reference.json",
        "option_combinations_oracle" => "option_combinations/cases.json",
        "output_options" => "output_options/reference.json",
        "output_validation" => "output_validation/reference.json",
        "overloads_checker_matrix" => "typescript_oracle/overloads-reference.json",
        "package_extras" => "package_extras/reference.json",
        "path_extras" => "path_extras/reference.json",
        "project_build_order" => "project_build_order/build-order-cases.json",
        "project_references" => "project_references/reference-graph-cases.json",
        "receiver_parameter_boundaries" => "receiver-parameter-boundaries-reference.json",
        "rest_array_context" => "rest-array-context-reference.json",
        "source_extras" => "source_extras/reference.json",
        "target_library_options" => "target_library_options/reference.json",
        "target_protocols" => "target_protocols/reference.json",
        "targets_checker_matrix" | "emitted_target_editions" => {
            "typescript_oracle/targets-reference.json"
        }
        "template_constant_context" => "template-constant-context-reference.json",
        "variable_pattern_context" => "variable-pattern-context-reference.json",
        _ => return None,
    })
}

fn tsv_suite(stem: &str) -> bool {
    matches!(
        stem,
        "class_checker_matrix"
            | "decorators_checker_matrix"
            | "enum_checker_matrix"
            | "immutable_checker_matrix"
            | "imported_value_checker_matrix"
            | "inferred_return_checker_matrix"
            | "jsx_checker_matrix"
            | "legacy_decorators_checker_matrix"
            | "namespace_checker_matrix"
            | "project_build_corrupt_state"
            | "project_build_equivalence"
            | "project_build_errors"
            | "project_build_freshness"
            | "project_build_invalidation"
            | "project_build_watch"
            | "project_cache_key"
            | "project_config"
            | "project_reference_owner_gate"
            | "project_referenced_declarations"
            | "source_map_publication"
            | "source_map_tokens_oracle"
            | "standard_library_checker_matrix"
            | "strictness_flags"
            | "target_syntax_profiles"
            | "unknown_name_checker_matrix"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc1_selection_accounts_for_nine_repaired_observations_and_all_controls() {
        let harness = include_str!("../../gc1_source_diagnostics.rs");
        let source = include_str!("../diagnostics/source-families.json");
        let controls = include_str!("../gc1_source_controls/reference.json");
        let cases = gc1_source_cases(harness, source, controls).unwrap();
        assert_eq!(cases.len(), 27);
        assert!(cases
            .iter()
            .all(|case| case.comparison == Comparison::Compared));
        let mut missing: Value = serde_json::from_str(source).unwrap();
        missing["cases"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["id"] != "diagnostics-other-tuple-index");
        assert!(gc1_source_cases(harness, &missing.to_string(), controls).is_err());
        assert!(gc1_source_cases(
            &harness.replace("\"tuple-index\"", "\"missing-id\""),
            source,
            controls
        )
        .is_err());
    }
}
