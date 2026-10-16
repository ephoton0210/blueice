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
                if arrays.len() != 1 {
                    return Err(format!("missing/ambiguous classification array: {binding}").into());
                }
                case_sources::classify(
                    &mut cases,
                    &arrays[0].labels,
                    "diagnostics-other-",
                    comparison,
                    reason,
                )?;
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
        "diagnostics_matrix" => "diagnostics/reference.json",
        "diagnostics_presentation" => "diagnostics/presentation-reference.json",
        "diagnostics_projects" => "diagnostics/project-positions.json",
        "explicit_this_context" => "explicit-this-context-reference.json",
        "guards_checker_matrix" => "typescript_oracle/guards-reference.json",
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
