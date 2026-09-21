// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `var` object-pattern bindings inside `with`: KeyedBindingInitialization
//! (§8.6.3) resolves the binding (a HasBinding probe of every enclosing
//! object environment) after the property key is evaluated and before the
//! value is read from the source, so the probes are observable through a
//! Proxy's `has` trap and an assignment reaches the object that owned the name
//! when it was resolved.

use blueice_bluejs::{compile, parse, Value, Vm, VmConfig};

fn text(source: &str) -> String {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    for config in [VmConfig::default(), config] {
        let value = Vm::new(config)
            .unwrap()
            .execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap();
        match value {
            Value::String(text) => return text.to_utf8().unwrap(),
            other => panic!("{source}: expected a string, got {other:?}"),
        }
    }
    unreachable!()
}

const TRACE: &str = "var log = [];
     var env = new Proxy({}, { has(t, k) { log.push('binding::' + String(k)); return false; } });
     var source = { get p() { log.push('get source'); return undefined; } };
     var sourceKey = { toString() { log.push('sourceKey'); return 'p'; } };
     var defaultValue = 0; var varTarget;";

#[test]
fn a_computed_key_is_evaluated_before_the_target_is_resolved_and_the_target_before_the_get() {
    assert_eq!(
        text(&format!(
            "{TRACE}
             with (env) {{ var {{ [sourceKey]: varTarget = defaultValue }} = source; }}
             log.join()"
        )),
        "binding::source,binding::sourceKey,sourceKey,binding::varTarget,get source,binding::defaultValue"
    );
}

#[test]
fn a_plain_and_a_shorthand_property_resolve_their_target_before_the_get() {
    assert_eq!(
        text(&format!(
            "{TRACE}
             with (env) {{ var {{ p: varTarget }} = source; }}
             log.join()"
        )),
        "binding::source,binding::varTarget,get source"
    );
    assert_eq!(
        text(&format!(
            "{TRACE} var q;
             var source2 = {{ get q() {{ log.push('get q'); return 1; }} }};
             with (env) {{ var {{ q }} = source2; }}
             log.join()"
        )),
        "binding::source2,binding::q,get q"
    );
}

#[test]
fn every_property_of_the_pattern_resolves_in_order() {
    assert_eq!(
        text(
            "var log = [];
             var env = new Proxy({}, { has(t, k) { log.push('has ' + String(k)); return false; } });
             var src = { get a() { log.push('get a'); return 1; }, get b() { log.push('get b'); return 2; } };
             var a, b;
             with (env) { var { a, b } = src; }
             log.join()"
        ),
        "has src,has a,get a,has b,get b"
    );
}

#[test]
fn the_value_goes_to_the_object_environment_that_has_the_name() {
    assert_eq!(
        text(
            "var o = { t: 0, u: 0 }; var t, u;
             with (o) { var { p: t, q: u = 'dflt' } = { p: 5 }; }
             o.t + ',' + o.u + ',' + typeof t + ',' + typeof u"
        ),
        "5,dflt,undefined,undefined"
    );
    // A name no with object has is an ordinary var assignment.
    assert_eq!(
        text(
            "var o = {}; var t;
             with (o) { var { p: t } = { p: 7 }; }
             t + ',' + ('t' in o)"
        ),
        "7,false"
    );
}

#[test]
fn the_reference_is_fixed_before_a_default_can_change_the_environment() {
    // The default deletes the property the reference was resolved to; the
    // write still goes to that object.
    assert_eq!(
        text(
            "var o = { t: 1 }; var t;
             with (o) { var { p: t = (delete o.t, 'dflt') } = {}; }
             o.t + ',' + typeof t"
        ),
        "dflt,undefined"
    );
}

#[test]
fn nested_patterns_rests_and_arrays_keep_working_inside_with() {
    assert_eq!(
        text(
            "var o = { a: 0, r: 0 }; var a, r, x, y;
             with (o) { var { n: { a }, ...r } = { n: { a: 3 }, k: 4 }; var [x, y] = [5, 6]; }
             JSON.stringify([o.a, o.r, x, y])"
        ),
        "[3,{\"k\":4},5,6]"
    );
    assert_eq!(
        text("var log = []; with ({}) { for (var { p: k } of [{ p: 1 }, { p: 2 }]) log.push(k); } log.join()"),
        "1,2"
    );
}

#[test]
fn outside_with_the_binding_order_is_unchanged() {
    assert_eq!(
        text(
            "var log = []; var src = { get p() { log.push('get'); return 1; } };
             var { p: t } = src; let { p: u } = src; log.join() + t + u"
        ),
        "get,get11"
    );
}
