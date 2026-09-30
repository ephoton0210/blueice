// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lazily materialized intrinsics failing on the heap ceiling. Each script
//! runs in a VM that has done nothing yet, so the first operation to need an
//! intrinsic is the one that builds it under the swept ceiling.
mod cov_g2_sweep;

use cov_g2_sweep::{sweep_cold_true, Mode};

#[test]
fn cold_property_lookups_on_primitives_and_plain_objects() {
    for script in [
        "(function () { return 1; })() === 1",
        "({}) + '' === '[object Object]'",
        "'x'.propertyIsEnumerable('a') === false",
        "'x'.hasOwnProperty('a') === false",
        "'x'.foo === undefined",
        "(1).foo === undefined",
        "true.foo === undefined",
        "1n.foo === undefined",
        "({}).at === undefined",
        "!('at' in {})",
        "({}).propertyIsEnumerable('a') === false",
        "({}).hasOwnProperty('a') === false",
        "(function () {}).constructor === Function",
        "Math.max.constructor === Function",
        "(() => { try { Math.max.caller; } catch (e) { return e instanceof TypeError; } })()",
        "(() => { try { Math.max.caller = 1; } catch (e) { return e instanceof TypeError; } })()",
        "globalThis.Array !== undefined",
        "(globalThis.Array = 1, globalThis.Array === 1)",
    ] {
        sweep_cold_true(Mode::PLAIN, script, 16);
    }
}

#[test]
fn cold_error_and_iterator_intrinsics() {
    for script in [
        "(() => { try { decodeURIComponent('%'); } catch (e) { return e instanceof URIError; } })()",
        "''[Symbol.iterator]().next().done === true",
        "(() => { try { Reflect.construct(Object, [], new Proxy(function () {}, { get(t, k) { if (k === 'prototype') throw 1; return t[k]; } })); } catch (e) { return e === 1; } })()",
    ] {
        sweep_cold_true(Mode::PLAIN, script, 16);
    }
    for script in [
        "(async function () {})() instanceof Promise",
        "(async () => { for await (var x of [1]) {} })() instanceof Promise",
        "new Promise(r => r(1)) instanceof Promise",
        "Promise.resolve(1).finally(() => {}) instanceof Promise",
        "Promise.resolve(1).then(x => x) instanceof Promise",
    ] {
        sweep_cold_true(Mode::JOBS, script, 16);
    }
}
