// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compare complete declaration provenance for every frozen graph and profile.
use super::{cli, decoded, inputs, javascript_maps};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn compare(
    specification: &Value,
    row: &Value,
    native: &Value,
    actual: &cli::Observation,
) -> Vec<String> {
    let id = specification["id"].as_str().unwrap();
    let mut failures = Vec::new();
    let success = native["diagnostics"].as_array().unwrap().is_empty();
    if actual.success != success || actual.diagnostics != native["diagnostics"] {
        failures.push(format!(
            "{id}: verdict/diagnostics expected {}; got {}",
            native["diagnostics"], actual.diagnostics
        ));
        return failures;
    }
    if success {
        let expected = native["selectedInputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_string())
            .collect::<BTreeSet<_>>();
        if actual.selected_inputs != expected {
            failures.push(format!("{id}: complete selected input inventory differs"));
        }
    }
    let expected = native["outputs"].as_object().unwrap();
    if expected.keys().collect::<BTreeSet<_>>() != actual.outputs.keys().collect::<BTreeSet<_>>() {
        failures.push(format!("{id}: complete artifact inventory differs"));
    }
    for (name, hash) in expected {
        if ![
            ".d.ts",
            ".d.mts",
            ".d.cts",
            ".d.ts.map",
            ".d.mts.map",
            ".d.cts.map",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        {
            continue;
        }
        let Some(text) = actual.outputs.get(name) else {
            continue;
        };
        let native = inputs::blob(hash.as_str().unwrap());
        if name.ends_with(".map") {
            if decoded::decoded(text) != decoded::decoded(native) {
                failures.push(format!(
                    "{id}: every decoded declaration segment/metadata: {name}"
                ));
            }
        } else if text != native {
            failures.push(format!("{id}: raw declaration bytes: {name}"));
        }
    }
    failures.extend(javascript_maps::compare(specification, row, native, actual));
    failures
}

pub fn metadata_regressions() {
    let temporary = cli::Temporary::new("metadata-options");
    let native = inputs::reference()["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut count = 0;
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if specification["form"] != "gd2_declaration_maps/unicode-comment-metadata" {
            continue;
        }
        let row = inputs::resolve(specification);
        let observation = cli::observe_builds(&temporary, index, &row, 2);
        failures.extend(compare(specification, &row, &native[index], &observation));
        count += 1;
    }
    assert_eq!(count, 19);
    assert!(
        failures.is_empty(),
        "G-E5 metadata/options regressions:\n{}",
        failures.join("\n")
    );
}

pub fn representative_forms() {
    let temporary = cli::Temporary::new("representative-forms");
    let native = inputs::reference()["cases"].as_array().unwrap();
    let mut forms = BTreeSet::new();
    let mut failures = Vec::new();
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let primary = specification["profile"] == "external"
            && (specification["variant"] == "original" || specification["variant"] == "lf");
        if !primary || !forms.insert(specification["form"].as_str().unwrap()) {
            continue;
        }
        let row = inputs::resolve(specification);
        let observation = cli::observe_builds(&temporary, index, &row, 2);
        failures.extend(compare(specification, &row, &native[index], &observation));
    }
    assert_eq!(forms.len(), 137);
    assert!(
        failures.is_empty(),
        "{} representative declaration differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

pub fn run() {
    let input = inputs::input();
    let specifications = input["cases"].as_array().unwrap();
    let reference = inputs::reference();
    let native = reference["cases"].as_array().unwrap();
    assert_eq!(specifications.len(), 8407);
    assert_eq!(native.len(), specifications.len());
    let next = AtomicUsize::new(0);
    let results = std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|worker| {
                let next = &next;
                scope.spawn(move || {
                    let temporary = cli::Temporary::new(&format!("all-declarations-{worker}"));
                    let mut failures = Vec::new();
                    let mut count = 0;
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(specification) = specifications.get(index) else {
                            break;
                        };
                        assert_eq!(specification["id"], native[index]["id"]);
                        let row = inputs::resolve(specification);
                        let observation = cli::observe_builds(&temporary, index, &row, 2);
                        failures.extend(compare(specification, &row, &native[index], &observation));
                        count += 1;
                        if (index + 1).is_multiple_of(250) {
                            eprintln!(
                                "G-E5 full declarations / JavaScript metadata and bounds: {}/8407; worker {worker}",
                                index + 1
                            );
                        }
                    }
                    (count, failures)
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().map(|(count, _)| count).sum::<usize>(), 8407);
    let failures = results
        .into_iter()
        .flat_map(|(_, failures)| failures)
        .collect::<Vec<_>>();
    let report = json!({"configurations":8407,"buildsPerConfiguration":2,
        "javascriptComparison":"complete metadata/order/tables/bounds; source-role parity remains separate",
        "differences":failures});
    let path = std::env::temp_dir().join(format!(
        "bluets-ge5-declaration-report-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(
        failures.is_empty(),
        "{} G-E5 declaration / JavaScript metadata and bounds differences; full report: {}; first 16:\n{}",
        failures.len(),
        path.display(),
        failures
            .iter()
            .take(16)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
