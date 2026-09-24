// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `%TypedArray%.prototype` algorithms: every method's argument coercion,
//! species construction, resizable-buffer and detachment behaviour, plus
//! resource-exhaustion sweeps through the same paths.

mod cov_g7_common;
use cov_g7_common::{failures, sweep_fuel_each, sweep_heap_each, sweep_strings, with_setup};

const PRELUDE: &str = r#"
var TA = Object.getPrototypeOf(Uint8Array.prototype);
var boom = {};
function throwsBoom(f) { try { f(); return false } catch (e) { return e === boom } }
function detach(t) { $262.detachArrayBuffer(t.buffer) }
"#;

fn run(body: &str) -> String {
    failures(&format!("{PRELUDE}{body}"))
}

#[test]
fn callback_methods_visit_elements_and_report_results() {
    assert_eq!(
        run(r#"
var t = new Int16Array([5, -3, 8, 0]);
eq('every true', t.every(function (x) { return x > -10 }), true);
eq('every false', t.every(function (x) { return x > 0 }), false);
eq('some true', t.some(function (x) { return x < 0 }), true);
eq('some false', t.some(function (x) { return x > 100 }), false);
var seen = [];
var self = {};
t.forEach(function (v, i, o) { seen.push(v + ':' + i + ':' + (o === t) + ':' + (this === self)) }, self);
eq('forEach', seen.join(), '5:0:true:true,-3:1:true:true,8:2:true:true,0:3:true:true');
eq('find', t.find(function (x) { return x < 0 }), -3);
eq('find none', t.find(function (x) { return x > 50 }), undefined);
eq('findIndex', t.findIndex(function (x) { return x === 8 }), 2);
eq('findIndex none', t.findIndex(function (x) { return x > 50 }), -1);
eq('findLast', t.findLast(function (x) { return x > 0 }), 8);
eq('findLast none', t.findLast(function (x) { return x > 50 }), undefined);
eq('findLastIndex', t.findLastIndex(function (x) { return x > 0 }), 2);
eq('findLastIndex none', t.findLastIndex(function (x) { return x > 50 }), -1);
eq('map', t.map(function (x) { return x * 2 }).join(), '10,-6,16,0');
eq('map this', t.map(function () { return this.k }, { k: 7 }).join(), '7,7,7,7');
eq('filter', t.filter(function (x) { return x > 0 }).join(), '5,8');
eq('filter none', t.filter(function () { return false }).length, 0);
var big = new BigInt64Array([1n, 2n]);
eq('bigint map', big.map(function (x) { return x + 1n }).join(), '2,3');
"#),
        ""
    );
}

#[test]
fn callback_methods_reject_bad_callbacks_receivers_and_throwing_callbacks() {
    assert_eq!(
        run(r#"
var names = ['every', 'some', 'forEach', 'find', 'findIndex', 'findLast', 'findLastIndex', 'map', 'filter', 'reduce', 'reduceRight'];
var t = new Uint8Array([1, 2, 3]);
names.forEach(function (name) {
  th(name + ' non-callable', function () { t[name](1) }, TypeError);
  th(name + ' missing', function () { t[name]() }, TypeError);
  th(name + ' receiver', function () { TA[name].call({}, function () {}) }, TypeError);
  th(name + ' receiver primitive', function () { TA[name].call(1, function () {}) }, TypeError);
  eq(name + ' throws', throwsBoom(function () { t[name](function () { throw boom }, 0) }), true);
});
var bigArray = new BigInt64Array(2);
th('map to number', function () { bigArray.map(function () { return 1 }) }, TypeError);
th('map to bigint', function () { t.map(function () { return 1n }) }, TypeError);
eq('map conversion throws', throwsBoom(function () { t.map(function () { return { valueOf: function () { throw boom } } }) }), true);
"#),
        ""
    );
}

#[test]
fn every_method_rejects_a_non_typed_array_receiver_or_a_detached_one() {
    assert_eq!(
        run(r#"
var names = ['at', 'copyWithin', 'every', 'fill', 'filter', 'find', 'findIndex', 'findLast', 'findLastIndex',
  'forEach', 'includes', 'indexOf', 'join', 'lastIndexOf', 'map', 'reduce', 'reduceRight', 'reverse', 'slice',
  'some', 'sort', 'toLocaleString', 'toReversed', 'toSorted', 'with'];
names.forEach(function (name) {
  th(name + ' object', function () { TA[name].call({}, function () {}, 0) }, TypeError);
  th(name + ' undefined', function () { TA[name].call(undefined, function () {}, 0) }, TypeError);
  th(name + ' array', function () { TA[name].call([1, 2], function () {}, 0) }, TypeError);
  var t = new Uint8Array(4);
  detach(t);
  th(name + ' detached', function () { t[name](function () {}, 0) }, TypeError);
});
var imm = new Uint8Array(new ArrayBuffer(4).sliceToImmutable());
th('immutable copyWithin', function () { imm.copyWithin(0, 1) }, TypeError);
th('immutable fill', function () { imm.fill(1) }, TypeError);
th('immutable reverse', function () { imm.reverse() }, TypeError);
th('immutable sort', function () { imm.sort() }, TypeError);
eq('immutable read', imm.slice(1).length + imm.includes(0) + imm.indexOf(0) + imm.at(0), 3 + 1 + 0 + 0);
"#),
        ""
    );
}

#[test]
fn species_construction_validates_the_constructor_and_its_result() {
    assert_eq!(
        run(r#"
function withSpecies(species) {
  var t = new Int16Array([1, 2, 3]);
  t.constructor = { [Symbol.species]: species };
  return t;
}
eq('constructor undefined',
   (function () { var t = new Int16Array([1, 2]); Object.defineProperty(t, 'constructor', { value: undefined }); return t.map(function (x) { return x + 1 }).join() })(), '2,3');
eq('species undefined', withSpecies(undefined).map(function (x) { return x }).join(), '1,2,3');
eq('species null', withSpecies(null).filter(function () { return true }).join(), '1,2,3');
eq('custom species', withSpecies(function (n) { return new Uint8Array(n) }).map(function (x) { return x + 255 }).join(), '0,1,2');
eq('species kind', withSpecies(function (n) { return new Uint8Array(n) }).map(function (x) { return x }).constructor, Uint8Array);
var noCtor = new Int16Array([1]);
Object.defineProperty(noCtor, 'constructor', { value: 1 });
th('constructor primitive map', function () { noCtor.map(function (x) { return x }) }, TypeError);
th('constructor primitive slice', function () { noCtor.slice() }, TypeError);
th('species primitive', function () { withSpecies(1).map(function (x) { return x }) }, TypeError);
th('species primitive slice', function () { withSpecies(1).slice() }, TypeError);
th('species arrow', function () { withSpecies(() => 1).map(function (x) { return x }) }, TypeError);
th('species native method', function () { withSpecies(Math.max).map(function (x) { return x }) }, TypeError);
th('species too small', function () { withSpecies(function () { return new Int16Array(0) }).map(function (x) { return x }) }, TypeError);
th('species too small filter', function () { withSpecies(function () { return new Int16Array(0) }).filter(function () { return true }) }, TypeError);
th('species not typed array', function () { withSpecies(function () { return {} }).map(function (x) { return x }) }, TypeError);
th('species immutable', function () { withSpecies(function (n) { return new Int16Array(new ArrayBuffer(n * 2).sliceToImmutable()) }).map(function (x) { return x }) }, TypeError);
eq('species throws', throwsBoom(function () { withSpecies(function () { throw boom }).map(function (x) { return x }) }), true);
var badCtor = new Int16Array([1]);
Object.defineProperty(badCtor, 'constructor', { get: function () { throw boom } });
eq('constructor getter throws', throwsBoom(function () { badCtor.map(function (x) { return x }) }), true);
var badSpecies = new Int16Array([1]);
badSpecies.constructor = {};
Object.defineProperty(badSpecies.constructor, Symbol.species, { get: function () { throw boom } });
eq('species getter throws', throwsBoom(function () { badSpecies.slice() }), true);
"#),
        ""
    );
}

#[test]
fn search_methods_normalize_their_from_index() {
    assert_eq!(
        run(r#"
var t = new Int8Array([1, 2, 3, 2, 1]);
eq('last default', t.lastIndexOf(2), 3);
eq('last from 2', t.lastIndexOf(2, 2), 1);
eq('last from -2', t.lastIndexOf(2, -2), 3);
eq('last from -3', t.lastIndexOf(2, -3), 1);
eq('last from -5 not found', t.lastIndexOf(2, -5), -1);
eq('last from -5 found', t.lastIndexOf(1, -5), 0);
eq('last from -6', t.lastIndexOf(1, -6), -1);
eq('last +Infinity', t.lastIndexOf(2, Infinity), 3);
eq('last -Infinity', t.lastIndexOf(2, -Infinity), -1);
eq('last NaN', t.lastIndexOf(1, NaN), 0);
eq('last beyond', t.lastIndexOf(2, 100), 3);
eq('last missing', t.lastIndexOf(9), -1);
eq('last wrong type', t.lastIndexOf('2'), -1);
eq('last empty', new Int8Array(0).lastIndexOf(1), -1);
eq('last coercion throws', throwsBoom(function () { t.lastIndexOf(1, { valueOf: function () { throw boom } }) }), true);
eq('includes default', t.includes(3), true);
eq('includes undefined from', t.includes(3, undefined), true);
eq('includes from 4', t.includes(1, 4), true);
eq('includes from 5', t.includes(1, 5), false);
eq('includes from -1', t.includes(1, -1), true);
eq('includes from -2', t.includes(3, -2), false);
eq('includes from -100', t.includes(1, -100), true);
eq('includes +Infinity', t.includes(1, Infinity), false);
eq('includes -Infinity', t.includes(1, -Infinity), true);
eq('includes NaN', t.includes(1, NaN), true);
eq('includes empty', new Int8Array(0).includes(1), false);
eq('includes NaN element', new Float32Array([NaN]).includes(NaN), true);
eq('includes coercion throws', throwsBoom(function () { t.includes(1, { valueOf: function () { throw boom } }) }), true);
eq('indexOf default', t.indexOf(2), 1);
eq('indexOf from 2', t.indexOf(2, 2), 3);
eq('indexOf from -1', t.indexOf(1, -1), 4);
eq('indexOf from -100', t.indexOf(3, -100), 2);
eq('indexOf beyond', t.indexOf(1, 5), -1);
eq('indexOf +Infinity', t.indexOf(1, Infinity), -1);
eq('indexOf -Infinity', t.indexOf(1, -Infinity), 0);
eq('indexOf NaN from', t.indexOf(1, NaN), 0);
eq('indexOf undefined from', t.indexOf(1, undefined), 0);
eq('indexOf NaN element', new Float32Array([NaN]).indexOf(NaN), -1);
eq('indexOf empty', new Int8Array(0).indexOf(1), -1);
eq('indexOf coercion throws', throwsBoom(function () { t.indexOf(1, { valueOf: function () { throw boom } }) }), true);
eq('at', t.at(0) + ':' + t.at(-1) + ':' + t.at(NaN) + ':' + t.at('1'), '1:1:1:2');
eq('at out of range', t.at(-6) + ':' + t.at(5) + ':' + t.at(Infinity) + ':' + t.at(-Infinity), 'undefined:undefined:undefined:undefined');
eq('at coercion throws', throwsBoom(function () { t.at({ valueOf: function () { throw boom } }) }), true);
"#),
        ""
    );
}

#[test]
fn join_and_reduce_cover_separators_initial_values_and_errors() {
    assert_eq!(
        run(r#"
var t = new Uint8Array([1, 2, 3]);
eq('join default', t.join(), '1,2,3');
eq('join undefined', t.join(undefined), '1,2,3');
eq('join separator', t.join('-'), '1-2-3');
eq('join object separator', t.join({ toString: function () { return '+' } }), '1+2+3');
eq('join empty', new Uint8Array(0).join(), '');
eq('join separator throws', throwsBoom(function () { t.join({ toString: function () { throw boom } }) }), true);
var rab = new ArrayBuffer(4, { maxByteLength: 8 });
var tracking = new Uint8Array(rab);
tracking.set([1, 2, 3, 4]);
eq('join after shrink', tracking.join({ toString: function () { rab.resize(2); return '-' } }), '1-2--');
eq('reduce sum', t.reduce(function (a, b) { return a + b }), 6);
eq('reduce init', t.reduce(function (a, b) { return a + b }, 10), 16);
eq('reduce index', t.reduce(function (a, b, i, o) { return a + i + (o === t ? 0 : 100) }, ''), '001020');
eq('reduce empty init', new Uint8Array(0).reduce(function () {}, 5), 5);
th('reduce empty', function () { new Uint8Array(0).reduce(function () {}) }, TypeError);
eq('reduceRight sum', t.reduceRight(function (a, b) { return a + '' + b }), '321');
eq('reduceRight init', t.reduceRight(function (a, b) { return a + b }, 10), 16);
eq('reduceRight empty init', new Uint8Array(0).reduceRight(function () {}, 5), 5);
th('reduceRight empty', function () { new Uint8Array(0).reduceRight(function () {}) }, TypeError);
eq('reduceRight single', new Uint8Array([7]).reduceRight(function () { throw 1 }), 7);
eq('reduce single', new Uint8Array([7]).reduce(function () { throw 1 }), 7);
"#),
        ""
    );
}

#[test]
fn slice_copies_bytes_between_species_results() {
    assert_eq!(
        run(r#"
var t = new Uint8Array([10, 20, 30, 40, 50, 60]);
eq('plain', t.slice(1, 4).join(), '20,30,40');
eq('negative', t.slice(-2).join(), '50,60');
eq('end undefined', t.slice(4, undefined).join(), '50,60');
eq('empty', t.slice(3, 3).length, 0);
eq('reversed range', t.slice(4, 2).length, 0);
eq('start throws', throwsBoom(function () { t.slice({ valueOf: function () { throw boom } }) }), true);
eq('end throws', throwsBoom(function () { t.slice(0, { valueOf: function () { throw boom } }) }), true);
var f = new Float32Array([NaN, 1.5]);
var g = f.slice();
eq('float bytes', g.length + ':' + g[1], '2:1.5');
var wide = new Int16Array([1, 2, 3]);
wide.constructor = { [Symbol.species]: function (n) { return new Uint8Array(n) } };
eq('other kind', wide.slice(1).join(), '2,3');
wide.constructor = { [Symbol.species]: function (n) { return new Float64Array(n) } };
eq('other kind wide', wide.slice().join(), '1,2,3');
var shared = new Uint8Array([1, 2, 3, 4]);
shared.constructor = { [Symbol.species]: function () { return new Uint8Array(shared.buffer, 1) } };
eq('shared buffer', shared.slice(0, 3).join(), '1,1,1');
var rab = new ArrayBuffer(4, { maxByteLength: 8 });
var tracking = new Uint8Array(rab);
tracking.set([1, 2, 3, 4]);
tracking.constructor = { [Symbol.species]: function (n) { rab.resize(2); return new Uint8Array(n) } };
eq('shrunk during species', tracking.slice(0, 4).join(), '1,2,0,0');
var fixed = new Uint8Array(new ArrayBuffer(4, { maxByteLength: 8 }), 0, 4);
fixed.constructor = { [Symbol.species]: function (n) { fixed.buffer.resize(2); return new Uint8Array(n) } };
th('fixed out of bounds during species', function () { fixed.slice(0, 4) }, TypeError);
var zero = new Uint8Array([1, 2, 3]);
zero.constructor = { [Symbol.species]: function (n) { detach(zero); return new Uint8Array(n) } };
eq('zero count skips copy', zero.slice(0, 0).length, 0);
"#),
        ""
    );
}

#[test]
fn slice_through_a_foreign_realm_species_constructor() {
    assert_eq!(
        run(r#"
var realm = $262.createRealm();
var other = realm.global;
var t = new Uint8Array([1, 2, 3]);
t.constructor = { [Symbol.species]: other.Uint8Array };
var copy = t.slice(1);
eq('foreign same kind', Array.prototype.join.call(copy, ','), '2,3');
eq('foreign prototype', Object.getPrototypeOf(copy) === other.Uint8Array.prototype, true);
t.constructor = { [Symbol.species]: other.Int16Array };
var wide = t.slice();
eq('foreign other kind', Array.prototype.join.call(wide, ','), '1,2,3');
t.constructor = { [Symbol.species]: other.Uint8Array };
eq('foreign empty', t.slice(1, 1).length, 0);
var shortCtor = realm.evalScript('(function (n) { return new Uint8Array(0) })');
t.constructor = { [Symbol.species]: shortCtor };
th('foreign too short', function () { t.slice() }, TypeError);
var tagged = new other.Uint8Array([9, 8]);
eq('foreign toLocaleString', TA.toLocaleString.call(tagged), '9,8');
th('foreign wrong receiver', function () { TA.toLocaleString.call(other.Object()) }, TypeError);
"#),
        ""
    );
}

#[test]
fn slice_species_across_realms_reports_bad_results_and_copies_through_reverse_facades() {
    assert_eq!(
        run(r#"
var realm = $262.createRealm();
var t = new Uint8Array([1, 2, 3]);
function speciesFrom(source) {
  t.constructor = { [Symbol.species]: realm.evalScript(source) };
}
speciesFrom('(function (n) { var r = new Uint8Array(n); Object.defineProperty(r, "buffer", { value: 1 }); return r })');
th('foreign buffer not an object', function () { t.slice(1) }, TypeError);
speciesFrom('(function (n) { return { get length() { throw 7 } } })');
eq('foreign length getter throws', (function () { try { t.slice(1) } catch (e) { return e } })(), 7);
speciesFrom('(function (n) { return { length: { valueOf() { throw 8 } } } })');
eq('foreign length coercion throws', (function () { try { t.slice(1) } catch (e) { return e } })(), 8);
speciesFrom('(function (n) { return { length: 0 } })');
th('foreign result too short', function () { t.slice(1) }, TypeError);
realm.global.parentInt16 = Int16Array;
realm.global.parentUint8 = Uint8Array;
eq('reverse different kind', realm.evalScript('var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: parentInt16 }; var r = t.slice(1); r.length + ":" + r[0] + ":" + r[1] + ":" + (r instanceof parentInt16)'), '2:2:3:true');
eq('reverse same kind', realm.evalScript('var t = new Uint8Array([1, 2, 3]); t.constructor = { [Symbol.species]: parentUint8 }; var r = t.slice(1); r.length + ":" + r[0] + ":" + r[1] + ":" + (r instanceof parentUint8)'), '2:2:3:true');
"#),
        ""
    );
}

#[test]
fn mismatched_species_content_types_and_throwing_arguments_are_reported() {
    assert_eq!(
        run(r#"
var t = new Int16Array([5, 1, 4]);
t.constructor = { [Symbol.species]: function (n) { return new BigInt64Array(n) } };
th('filter into bigint species', function () { t.filter(function () { return true }) }, TypeError);
th('slice into bigint species', function () { t.slice(1) }, TypeError);
var b = new BigInt64Array([1n, 2n]);
b.constructor = { [Symbol.species]: function (n) { return new Int16Array(n) } };
th('bigint slice into number species', function () { b.slice(0) }, TypeError);
var u = new Uint8Array(4);
var throwing = { valueOf: function () { throw boom } };
eq('copyWithin start throws', throwsBoom(function () { u.copyWithin(0, throwing) }), true);
eq('copyWithin end throws', throwsBoom(function () { u.copyWithin(0, 1, throwing) }), true);
eq('fill start throws', throwsBoom(function () { u.fill(1, throwing) }), true);
eq('fill end throws', throwsBoom(function () { u.fill(1, 0, throwing) }), true);
var realm = $262.createRealm();
realm.evalScript('Object.defineProperty(Int16Array.prototype, "set", { get: function () { throw 42 } })');
var source = new Uint8Array([1, 2, 3]);
source.constructor = { [Symbol.species]: realm.global.Int16Array };
eq('foreign set getter throws', (function () { try { source.slice() } catch (e) { return e } })(), 42);
"#),
        ""
    );
}

#[test]
fn sorting_orders_numbers_bigints_and_comparator_results() {
    assert_eq!(
        run(r#"
var f = new Float64Array([3, NaN, -0, 0, -Infinity, 2]);
f.sort();
eq('default float order', Array.prototype.map.call(f, function (x) { return Object.is(x, -0) ? '-0' : String(x) }).join(), '-Infinity,-0,0,2,3,NaN');
eq('default bigint', new BigInt64Array([3n, -1n, 2n]).sort().join(), '-1,2,3');
eq('toSorted default', new Int8Array([3, 1, 2]).toSorted().join(), '1,2,3');
var ascending = function (a, b) { return a - b };
var inputs = [[], [1], [2, 1], [1, 2], [1, 1, 1], [5, 4, 3, 2, 1], [1, 2, 3, 4, 5], [5, 1, 4, 2, 3, 9, 7, 8, 6, 0],
  [1, 3, 5, 2, 4, 6, 0], [3, 2, 1, 4, 5, 6, 0], [1, 2, 2, 1, 1, 2], [9, 8, 7, 1, 2, 3, 6, 5, 4, 0, 10]];
inputs.forEach(function (input) {
  var expected = input.slice().sort(ascending).join();
  eq('sort ' + input, new Int8Array(input).sort(ascending).join(), expected);
  eq('toSorted ' + input, new Int8Array(input).toSorted(ascending).join(), expected);
});
var stable = new Uint8Array([10, 21, 11, 20, 12, 22]);
stable.sort(function (a, b) { return (a / 10 | 0) - (b / 10 | 0) });
eq('stable', stable.join(), '10,11,12,21,20,22');
eq('comparator nan', new Uint8Array([3, 1, 2]).sort(function () { return NaN }).join(), '3,1,2');
th('sort non-callable', function () { new Uint8Array(2).sort(1) }, TypeError);
th('sort null', function () { new Uint8Array(2).sort(null) }, TypeError);
th('toSorted non-callable', function () { new Uint8Array(2).toSorted({}) }, TypeError);
eq('sort throws', throwsBoom(function () { new Uint8Array([2, 1]).sort(function () { throw boom }) }), true);
eq('sort conversion throws', throwsBoom(function () { new Uint8Array([2, 1]).sort(function () { return { valueOf: function () { throw boom } } }) }), true);
for (var n = 1; n <= 14; n++) {
  var t = new Int8Array([5, 1, 4, 2, 3, 9, 7, 8]);
  var calls = 0;
  var detached = false;
  var r = t.sort(function (a, b) { if (++calls === n) { detach(t); detached = true } return a - b });
  eq('detach at ' + n, r === t && t.length === (detached ? 0 : 8), true);
}
"#),
        ""
    );
}

#[test]
fn change_by_copy_methods_and_element_writers_handle_edges() {
    assert_eq!(
        run(r#"
var t = new Int8Array([1, 2, 3, 4, 5]);
eq('toReversed', t.toReversed().join(), '5,4,3,2,1');
eq('toReversed source', t.join(), '1,2,3,4,5');
eq('with', t.with(1, 9).join(), '1,9,3,4,5');
eq('with negative', t.with(-1, 9).join(), '1,2,3,4,9');
eq('with NaN', t.with(NaN, 9).join(), '9,2,3,4,5');
eq('with fraction', t.with(1.9, 9).join(), '1,9,3,4,5');
th('with too big', function () { t.with(5, 1) }, RangeError);
th('with too negative', function () { t.with(-6, 1) }, RangeError);
th('with infinity', function () { t.with(Infinity, 1) }, RangeError);
th('with huge', function () { t.with(1e300, 1) }, RangeError);
eq('with index throws', throwsBoom(function () { t.with({ valueOf: function () { throw boom } }, 1) }), true);
eq('with value throws', throwsBoom(function () { t.with(0, { valueOf: function () { throw boom } }) }), true);
th('with bigint mismatch', function () { new BigInt64Array(2).with(0, 1) }, TypeError);
var rab = new ArrayBuffer(4, { maxByteLength: 8 });
var u = new Uint8Array(rab);
th('with shrink invalidates index', function () { u.with(3, { valueOf: function () { rab.resize(2); return 1 } }) }, RangeError);
var rab2 = new ArrayBuffer(4, { maxByteLength: 8 });
var u2 = new Uint8Array(rab2);
th('with shrink then read', function () { u2.with(0, { valueOf: function () { rab2.resize(2); return 1 } }) }, TypeError);
var rab3 = new ArrayBuffer(2, { maxByteLength: 8 });
var u3 = new Uint8Array(rab3);
eq('with grow makes index valid', u3.with(3, { valueOf: function () { rab3.resize(4); return 7 } }).length, 2);
eq('copyWithin', new Int8Array([1, 2, 3, 4, 5]).copyWithin(0, 3).join(), '4,5,3,4,5');
eq('copyWithin range', new Int8Array([1, 2, 3, 4, 5]).copyWithin(1, 0, 3).join(), '1,1,2,3,5');
eq('copyWithin negative', new Int8Array([1, 2, 3, 4, 5]).copyWithin(-2, -4, -3).join(), '1,2,3,2,5');
eq('copyWithin nothing', new Int8Array([1, 2, 3]).copyWithin(0, 3).join(), '1,2,3');
eq('copyWithin end undefined', new Int8Array([1, 2, 3]).copyWithin(1, 0, undefined).join(), '1,1,2');
eq('copyWithin throws', throwsBoom(function () { t.copyWithin({ valueOf: function () { throw boom } }, 0) }), true);
var rab4 = new ArrayBuffer(4, { maxByteLength: 8 });
var u4 = new Uint8Array(rab4);
u4.set([1, 2, 3, 4]);
eq('copyWithin shrink', u4.copyWithin(0, { valueOf: function () { rab4.resize(3); return 1 } }).join(), '2,3,3');
var rab5 = new ArrayBuffer(4, { maxByteLength: 8 });
var fixed = new Uint8Array(rab5, 0, 4);
th('copyWithin fixed shrink', function () { fixed.copyWithin(0, { valueOf: function () { rab5.resize(2); return 1 } }) }, TypeError);
eq('fill', new Int8Array(4).fill(7).join(), '7,7,7,7');
eq('fill range', new Int8Array(4).fill(7, 1, 3).join(), '0,7,7,0');
eq('fill negative', new Int8Array(4).fill(7, -2).join(), '0,0,7,7');
eq('fill end undefined', new Int8Array(4).fill(7, 2, undefined).join(), '0,0,7,7');
eq('fill bigint', new BigInt64Array(2).fill(3n).join(), '3,3');
th('fill bigint mismatch', function () { new BigInt64Array(2).fill(3) }, TypeError);
eq('fill value throws', throwsBoom(function () { t.fill({ valueOf: function () { throw boom } }) }), true);
var rab6 = new ArrayBuffer(4, { maxByteLength: 8 });
var fixed6 = new Uint8Array(rab6, 0, 4);
th('fill fixed shrink', function () { fixed6.fill({ valueOf: function () { rab6.resize(2); return 1 } }) }, TypeError);
eq('reverse', new Int8Array([1, 2, 3]).reverse().join(), '3,2,1');
eq('reverse empty', new Int8Array(0).reverse().length, 0);
eq('toLocaleString', new Uint8Array([1, 2]).toLocaleString(), '1,2');
th('toLocaleString primitive', function () { TA.toLocaleString.call(1) }, TypeError);
th('toLocaleString object', function () { TA.toLocaleString.call({}) }, TypeError);
"#),
        ""
    );
}

const SETUP: &str =
    "var t = new Int16Array([5, 1, 4, 2, 3]); new Float64Array(1); new BigInt64Array(1);";
const FOREIGN_SETUP: &str = "var t = new Uint8Array([5, 1, 4]); var other = $262.createRealm().global; new other.Uint8Array(1); new other.Int16Array(1);";

const BODIES: &[&str] = &[
    "t.map(function (x) { return x + 1 });",
    "t.filter(function (x) { return x > 2 });",
    "t.slice(1);",
    "t.toSorted(); t.toReversed();",
    "t.with(1, 9);",
    "t.sort(function (a, b) { return a - b });",
    "t.join('-');",
    "t.reduce(function (a, b) { return a + b }); t.reduceRight(function (a, b) { return a + b });",
    "t.includes(2); t.indexOf(2); t.lastIndexOf(2);",
    "t.copyWithin(0, 2); t.fill(1, 1, 3); t.reverse(); t.at(-1);",
    "t.every(function () { return true }); t.some(function () { return false }); t.forEach(function () {});",
    "t.find(function () { return false }); t.findIndex(function () { return false }); t.findLast(function () { return true }); t.findLastIndex(function () { return true });",
    "t.constructor = { [Symbol.species]: function (n) { return new Uint8Array(n) } }; t.map(function (x) { return x }); t.slice(1);",
    "var v = new Uint8Array([5, 1, 4]); v.constructor = { [Symbol.species]: function (n) { return new Uint8Array(new ArrayBuffer(n + 1), 1) } }; v.slice(1);",
];
const FOREIGN_BODIES: &[&str] = &[
    "t.constructor = { [Symbol.species]: other.Uint8Array }; t.slice(1);",
    "t.constructor = { [Symbol.species]: other.Int16Array }; t.slice(1);",
];

#[test]
fn every_heap_allocation_failure_reports_the_heap_limit() {
    sweep_heap_each(&with_setup(SETUP, BODIES));
    sweep_heap_each(&with_setup(FOREIGN_SETUP, FOREIGN_BODIES));
}

#[test]
fn every_instruction_budget_exhaustion_reports_the_instruction_limit() {
    sweep_fuel_each(&with_setup(SETUP, BODIES));
    sweep_fuel_each(&with_setup(FOREIGN_SETUP, FOREIGN_BODIES));
}

#[test]
fn string_growth_in_join_reports_the_string_limit() {
    sweep_strings("new Uint16Array([1, 22, 333]).join('--')", 40);
}
