// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source-defined programs and literal Cartesian configuration axes.

use super::case_sources::{Case, Comparison};
use super::inventory::Suite;
use super::rust_facts::{self, Array, Facts};
use super::snapshot::{Result, Snapshot};
use std::collections::BTreeSet;

pub fn array<'a>(facts: &'a Facts, scope: &str, binding: &str) -> Result<&'a Array> {
    let matches: Vec<_> = facts
        .arrays
        .iter()
        .filter(|array| array.scope == scope && array.binding == binding)
        .collect();
    if matches.len() != 1 {
        return Err(format!("missing/ambiguous array: {scope}::{binding}").into());
    }
    Ok(matches[0])
}

pub fn array_cases(source: &str, array: &Array) -> Result<Vec<Case>> {
    if array.labels.is_empty() || array.labels.iter().any(|label| label.starts_with('#')) {
        return Err(format!("unresolved case names: {}::{}", array.scope, array.binding).into());
    }
    Ok(array
        .labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            Case::new(
                source,
                &format!("array:{}::{}/{index}", array.scope, array.binding),
                &label.chars().take(100).collect::<String>(),
                "program",
            )
        })
        .collect())
}

pub fn product(source: &str, bases: Vec<Case>, axes: &[&Array]) -> Result<Vec<Case>> {
    let mut cases = bases;
    for axis in axes {
        if axis.rows.is_empty()
            || axis
                .rows
                .iter()
                .flatten()
                .any(|value| value.starts_with('#'))
        {
            return Err(format!(
                "unresolved configuration axis: {}::{}",
                axis.scope, axis.binding
            )
            .into());
        }
        let mut next = Vec::new();
        for base in &cases {
            for (index, row) in axis.rows.iter().enumerate() {
                let mut case = Case::new(
                    &base.source,
                    &format!(
                        "{}*{source}#{}::{}/{index}",
                        base.selector, axis.scope, axis.binding
                    ),
                    &format!("{}/{}", base.label, row.join(",")),
                    "configuration",
                );
                case.comparison = base.comparison.clone();
                case.reason = base.reason.clone();
                next.push(case);
            }
        }
        cases = next;
    }
    Ok(cases)
}

fn single(source: &str, selector: &str, label: &str) -> Vec<Case> {
    vec![Case::new(source, selector, label, "program")]
}

pub fn programs(snapshot: &Snapshot, suite: &Suite) -> Result<Option<Vec<Case>>> {
    let source = &suite.harness;
    let stem = source
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".rs"))
        .ok_or("invalid harness")?;
    let facts = rust_facts::parse(snapshot.source(source)?)?;
    let from = |scope: &str, binding: &str| array_cases(source, array(&facts, scope, binding)?);
    let cases = match stem {
        "async_generator_targets"
        | "async_targets"
        | "es5_scope_targets"
        | "explicit_this_parameters"
        | "logical_assignment_targets"
        | "optional_targets"
        | "rest_array_parameters"
        | "variable_patterns" => {
            if !facts.constants.contains_key("SOURCE") {
                return Err(format!("{source}: missing SOURCE").into());
            }
            single(source, "const:SOURCE", "SOURCE")
        }
        "exponentiation_targets" => product(
            source,
            single(source, "const:SOURCE", "SOURCE"),
            &[array(&facts, &suite.ignored_tests[0], "for@0")?],
        )?,
        "decorators_direct" | "namespace_parity" | "commonjs_oracle" => from("", "PROGRAMS")?,
        "jsx_direct" => from("", "BODIES")?,
        "class_downlevel_oracle" | "enum_oracle" => product(
            source,
            from("", "FIXTURES")?,
            &[array(&facts, "", "MODES")?],
        )?,
        "contextual_type_imports" | "json_default_binding" | "module_type_resolution" => {
            from(&suite.ignored_tests[0], "for@0")?
        }
        "default_generators" => product(
            source,
            single(source, "generator", "default-generator"),
            &[
                array(&facts, &suite.ignored_tests[0], "for@0")?,
                array(&facts, &suite.ignored_tests[0], "for@1")?,
                array(&facts, &suite.ignored_tests[0], "for@2")?,
            ],
        )?,
        "jsx_oracle" => {
            if facts.program_vectors.len() != 1 {
                return Err("missing/ambiguous JSX program vector".into());
            }
            array_cases(source, &facts.program_vectors[0])?
        }
        "decorators_oracle" => {
            let bodies = product(
                source,
                from("programs", "for@0")?,
                &[array(&facts, "programs", "for@1")?],
            )?;
            let exports = product(
                source,
                single(source, "exports", "exports"),
                &[array(&facts, "programs", "for@2")?],
            )?;
            [bodies, exports, from("", "FAILURES")?].concat()
        }
        "legacy_decorators_oracle" => {
            let bodies = product(
                source,
                from("programs", "for@0")?,
                &[array(&facts, "programs", "for@1")?],
            )?;
            let exports = product(
                source,
                single(source, "exports", "exports"),
                &[array(&facts, "programs", "for@2")?],
            )?;
            [bodies, exports].concat()
        }
        "namespace_declaration_oracle" => [
            single(source, "const:SOURCE", "emitted-declaration"),
            from("", "CONSUMERS")?,
        ]
        .concat(),
        "namespace_oracle" => {
            let matrix = suite
                .inputs
                .keys()
                .find(|path| path.ends_with("-matrix.tsv"))
                .ok_or("missing namespace matrix")?;
            let deferred = suite
                .inputs
                .keys()
                .find(|path| path.ends_with("-deferred.txt"))
                .ok_or("missing namespace deferred entries")?;
            let excluded: BTreeSet<_> = snapshot.source(deferred)?.lines().collect();
            let records =
                super::case_sources::tsv_cases(matrix, snapshot.source(matrix)?, false, "program")?;
            let accepted = records
                .into_iter()
                .zip(snapshot.source(matrix)?.lines())
                .filter_map(|(case, line)| {
                    let (name, verdict) = line.split_once('\t')?;
                    (verdict == "accept" && !excluded.contains(name)).then_some(case)
                })
                .collect();
            product(source, accepted, &[array(&facts, "", "TARGETS")?])?
        }
        "strictness_controls" => controls(source, &facts, &suite.ignored_tests)?,
        "typescript_oracle" => core_programs(source, &facts, &suite.ignored_tests)?,
        _ => return Ok(None),
    };
    if cases.is_empty() {
        return Err(format!("empty inline case source: {source}").into());
    }
    let ids: BTreeSet<_> = cases.iter().map(|case| &case.id).collect();
    if ids.len() != cases.len() {
        return Err(format!("duplicate inline case id: {source}").into());
    }
    Ok(Some(cases))
}

fn controls(source: &str, facts: &Facts, tests: &[String]) -> Result<Vec<Case>> {
    let mut reachable: BTreeSet<String> = tests.iter().cloned().collect();
    loop {
        let before = reachable.len();
        for call in &facts.calls {
            if reachable.contains(&call.scope)
                && facts.calls.iter().any(|other| other.scope == call.function)
            {
                reachable.insert(call.function.clone());
            }
        }
        if reachable.len() == before {
            break;
        }
    }
    facts
        .calls
        .iter()
        .filter(|call| reachable.contains(&call.scope) && call.function == "accepts")
        .enumerate()
        .map(|(index, call)| {
            if call.loop_depth != 0 {
                return Err("checking control in a loop needs an explicit axis".into());
            }
            Ok(Case::new(
                source,
                &format!("call:{}::accepts/{index}", call.scope),
                &format!("{}:{index}", call.scope),
                "checking-configuration",
            ))
        })
        .collect()
}

fn core_programs(source: &str, facts: &Facts, tests: &[String]) -> Result<Vec<Case>> {
    let mut cases = array_cases(source, array(facts, "", "CASES")?)?;
    for test in tests {
        if test == "pinned_bluetsc_oracle_matches_the_supported_fixture_matrix" {
            continue;
        }
        let primary = facts
            .arrays
            .iter()
            .find(|array| array.scope == *test && array.binding == "cases")
            .or_else(|| {
                facts
                    .arrays
                    .iter()
                    .find(|array| array.scope == *test && array.binding == "for@0")
            });
        let mut controls = if let Some(primary) = primary {
            array_cases(source, primary)?
        } else {
            Vec::new()
        };
        for array in facts.arrays.iter().filter(|array| {
            array.scope == *test
                && (array
                    .binding
                    .starts_with("arg:assert_pinned_no_emit_cases@")
                    || array
                        .binding
                        .starts_with("arg:assert_pinned_class_module_cases@"))
        }) {
            controls.extend(array_cases(source, array)?);
        }
        for (index, _) in facts
            .calls
            .iter()
            .filter(|call| {
                call.scope == *test
                    && call.loop_depth == 0
                    && call.function == "fs::write"
                    && call
                        .arguments
                        .first()
                        .is_some_and(|argument| argument == "input")
            })
            .enumerate()
        {
            controls.push(Case::new(
                source,
                &format!("call:{test}::entry-write/{index}"),
                &format!("{test}:entry-{index}"),
                "program",
            ));
        }
        if controls.is_empty() {
            return Err(format!("no program provider for core oracle: {test}").into());
        }
        for case in &mut controls {
            case.comparison = Comparison::ReferenceOnly;
            case.reason = Some("This no-emit test verifies pinned TypeScript expectations only; it does not invoke BlueTS. Do not count it as direct agreement in this suite.".into());
        }
        cases.extend(controls);
    }
    Ok(cases)
}
