// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array-like walks: the length read, each index probe and their budgets.

mod cov_g4_common;
use cov_g4_common::{failures, sweep_fuel_each, with_setup};

const PRELUDE: &str = r#"
var boom = {};
function throwsBoom(f) { try { f(); return false } catch (e) { return e === boom } }
var walks = {
  forEach: function (o) { Array.prototype.forEach.call(o, function () {}) },
  map: function (o) { Array.prototype.map.call(o, function (x) { return x }) },
  filter: function (o) { Array.prototype.filter.call(o, function () { return true }) },
  reduce: function (o) { Array.prototype.reduce.call(o, function (a, b) { return a + b }, 0) },
  reduceRight: function (o) { Array.prototype.reduceRight.call(o, function (a, b) { return a + b }, 0) },
  some: function (o) { Array.prototype.some.call(o, function () { return false }) },
  every: function (o) { Array.prototype.every.call(o, function () { return true }) },
  indexOf: function (o) { Array.prototype.indexOf.call(o, 99) },
  lastIndexOf: function (o) { Array.prototype.lastIndexOf.call(o, 99) },
};
"#;

fn run(body: &str) -> String {
    failures(&format!("{PRELUDE}{body}"))
}

#[test]
fn a_walk_reports_a_throwing_length_or_index_probe() {
    assert_eq!(
        run(r#"
Object.keys(walks).forEach(function (name) {
  var walk = walks[name];
  eq(name + ' length getter', throwsBoom(function () { walk({ get length() { throw boom } }) }), true);
  eq(name + ' length conversion', throwsBoom(function () { walk({ length: { valueOf: function () { throw boom } } }) }), true);
  eq(name + ' element getter', throwsBoom(function () { walk({ length: 3, get 1() { throw boom } }) }), true);
  eq(name + ' has trap', throwsBoom(function () { walk(new Proxy({ length: 3 }, { has: function () { throw boom } })) }), true);
});
"#),
        ""
    );
}

#[test]
fn a_walk_over_a_huge_sparse_array_like_visits_only_what_is_stored() {
    assert_eq!(
        run(r#"
var sparse = { length: 4294967295, 5: 'a', 4000000000: 'b' };
var seen = [];
Array.prototype.forEach.call(sparse, function (value, index) { seen.push(index + value) });
eq('forEach', seen.join(), '5a,4000000000b');
eq('lastIndexOf', Array.prototype.lastIndexOf.call(sparse, 'a'), 5);
eq('reduceRight', Array.prototype.reduceRight.call(sparse, function (a, b) { return a + b }), 'ba');
var proto = { 70000: 'p' };
var child = Object.create(proto);
child.length = 100000; child[3] = 'c';
var keys = [];
Array.prototype.forEach.call(child, function (value, index) { keys.push(index) });
eq('inherited', keys.join(), '3,70000');
var changing = { length: 100000, 1: 'x' };
var order = [];
Array.prototype.forEach.call(changing, function (value, index) { order.push(index); if (index === 1) changing[50000] = 'late' });
eq('added mid-walk', order.join(), '1,50000');
var shifting = { length: 100000, 1: 'x' };
var visited = [];
Array.prototype.forEach.call(shifting, function (value, index) { visited.push(index); if (index === 1) Object.setPrototypeOf(shifting, { 90000: 'y' }) });
eq('prototype swapped mid-walk', visited.join(), '1,90000');
"#),
        ""
    );
}

const SETUP: &str = "var sparse = { length: 100000, 5: 'a', 99999: 'b' }; var dense = { length: 4, 1: 'x', 2: 'y' };";

const BODIES: &[&str] = &[
    "Array.prototype.forEach.call(dense, function () {});",
    "Array.prototype.forEach.call(sparse, function () {});",
    "Array.prototype.reduceRight.call(sparse, function (a, b) { return a + b }, '');",
    "Array.prototype.map.call(dense, function (x) { return x });",
    "Array.prototype.indexOf.call(dense, 'y'); Array.prototype.lastIndexOf.call(dense, 'x');",
];

#[test]
fn every_instruction_budget_exhaustion_reports_the_instruction_limit() {
    sweep_fuel_each(&with_setup(SETUP, BODIES));
}
