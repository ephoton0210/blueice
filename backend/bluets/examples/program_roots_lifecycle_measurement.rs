// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retains every actual library generation, work set, map and comparison.

#[path = "../tests/program_roots_lifecycle_support/mod.rs"]
mod program_roots_lifecycle_support;
use serde_json::{json, Value};
use std::io::{Seek, Write};

fn save(file: &mut std::fs::File, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    file.rewind().unwrap();
    file.set_len(0).unwrap();
    file.write_all(&bytes).unwrap();
    file.flush().unwrap();
}

fn main() {
    let path = std::env::args_os().nth(1).expect("fresh report filename");
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    let mut report = json!({"conclusion":"in_progress","inventoryCredit":0,
        "scope":"All 528 frozen same-process library lifecycle states, including mapped profiles. Complete CLI, consumers and delivery remain separate.",
        "cases":[]});
    save(&mut output, &report);
    program_roots_lifecycle_support::run(|row| {
        report["cases"].as_array_mut().unwrap().push(row);
        save(&mut output, &report);
    });
    let rows = report["cases"].as_array().unwrap();
    let counts = json!({"states":rows.len(),
        "nativeAgreements":rows.iter().filter(|row|row["nativeAgrees"]==true).count(),
        "coldWarmExact":rows.iter().filter(|row|row["coldWarmExact"]==true).count(),
        "workSetPartitions":rows.iter().filter(|row|row["parsedPartition"]==true&&row["checkedPartition"]==true).count(),
        "cacheContracts":rows.iter().filter(|row|row["cacheContract"]==true).count(),
        "cacheHits":rows.iter().filter(|row|row["cacheHit"]==true).count()});
    report["counts"] = counts.clone();
    report["conclusion"] = json!("measured-complete-library-lifecycle");
    save(&mut output, &report);
    println!("{counts}");
}
