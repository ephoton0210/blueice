// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The grammar and the statement compiler on a corpus of programs: every
//! prefix of each program is parsed (so each error path of the parser runs),
//! and each program is compiled under every bytecode limit (so each emitted
//! instruction is in turn the one that does not fit).

mod cov_g8_common;

use blueice_bluejs::{parse, parse_module};
use cov_g8_common::{check_parse_cases, compile_limit_sweep, parse_prefixes};

include!("cov_g8_corpus/scripts.in");
include!("cov_g8_corpus/modules.in");
include!("cov_g8_tables/parse.in");

/// Contexts a script is also parsed in: strict code, functions of each kind
/// and a class static block, each of which changes what a binding may be.
const CONTEXTS: &[(&str, &str)] = &[
    ("", ""),
    ("'use strict'; ", ""),
    ("async function wrapper() { ", " }"),
    ("function* wrapper() { ", " }"),
    ("async function* wrapper() { ", " }"),
    ("class Wrapper { static { ", " } }"),
    ("class Wrapper { m() { ", " } }"),
];

#[test]
fn every_prefix_of_every_script_is_parsed_in_every_context() {
    let mut rejected = 0;
    for source in SCRIPTS {
        for (before, after) in CONTEXTS {
            rejected += parse_prefixes(&format!("{before}{source}{after}"), false);
        }
    }
    assert!(rejected > 10_000, "{rejected}");
}

#[test]
fn every_prefix_of_every_module_is_parsed() {
    let mut rejected = 0;
    for source in MODULES.iter().chain(SCRIPTS) {
        rejected += parse_prefixes(source, true);
    }
    assert!(rejected > 2_000, "{rejected}");
}

#[test]
fn every_script_compiles_under_every_bytecode_limit() {
    let mut compiled = 0;
    let mut instructions = 0;
    for source in SCRIPTS {
        for (before, after) in CONTEXTS.iter().take(3) {
            if let Some(size) = compile_limit_sweep(&format!("{before}{source}{after}"), false) {
                compiled += 1;
                instructions += size;
            }
        }
    }
    assert!(compiled > 100, "{compiled}");
    assert!(instructions > 10_000, "{instructions}");
}

#[test]
fn every_module_compiles_under_every_bytecode_limit() {
    let compiled = MODULES
        .iter()
        .chain(SCRIPTS)
        .filter(|source| compile_limit_sweep(source, true).is_some())
        .count();
    assert!(compiled > 50, "{compiled}");
}

#[test]
fn the_corpus_is_mostly_valid() {
    let scripts = SCRIPTS
        .iter()
        .filter(|source| parse(source).is_ok())
        .count();
    let modules = MODULES
        .iter()
        .filter(|source| parse_module(source).is_ok())
        .count();
    assert!(scripts * 10 > SCRIPTS.len() * 8, "{scripts}");
    assert!(modules * 10 > MODULES.len() * 6, "{modules}");
}

#[test]
fn syntax_errors_report_the_reference_messages() {
    check_parse_cases(PARSE_CASES);
}
