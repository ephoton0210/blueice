// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Test262's Temporal fixtures for the plain, zoned and duration types.

mod cov_g4_common;
use cov_g4_common::test262::run_directory;

fn check(type_name: &str) {
    let mut failures = run_directory(&format!("built-ins/Temporal/{type_name}"), &[]);
    failures.extend(run_directory(&format!("intl402/Temporal/{type_name}"), &[]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn plain_date() {
    check("PlainDate");
}

#[test]
fn plain_date_time() {
    check("PlainDateTime");
}

#[test]
fn plain_year_month() {
    check("PlainYearMonth");
}

#[test]
fn plain_month_day() {
    check("PlainMonthDay");
}

#[test]
fn zoned_date_time() {
    check("ZonedDateTime");
}

#[test]
fn duration() {
    check("Duration");
}

#[test]
fn instant() {
    check("Instant");
}
