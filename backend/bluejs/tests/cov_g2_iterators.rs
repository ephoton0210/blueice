// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Iterator helper (`Iterator.prototype.*`, `Iterator.concat/zip/zipKeyed`)
//! state machines and the shared iterator record operations: rarely taken
//! completions (re-entrancy, close errors, exhausted helpers, invalid
//! receivers) and every allocation failing in turn.
mod cov_g2_support;

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};
use cov_g2_support::{expect_true, run};

/// A helper prototype's methods reject receivers that are not helpers.
#[test]
fn helper_next_and_return_reject_non_helper_receivers() {
    expect_true(
        "var proto = Object.getPrototypeOf(Iterator.from([1]).map(x => x));
         function rejects(f) { try { f(); return false; } catch (e) { return e instanceof TypeError; } }
         rejects(() => proto.next.call(5)) && rejects(() => proto.next.call({})) &&
           rejects(() => proto.return.call(5)) && rejects(() => proto.return.call({}))",
    );
}

/// An exhausted helper keeps answering done, and calling `return` on it is a
/// harmless no-op.
#[test]
fn exhausted_helpers_stay_done_for_every_kind() {
    expect_true(
        "function drained(it) { it.next(); it.next(); it.next();
           var again = it.next(); var ret = it.return();
           return again.done === true && again.value === undefined && ret.done === true; }
         drained([1].values().map(x => x)) &&
           drained(Iterator.concat([1])) &&
           drained(Iterator.zip([[1]])) &&
           drained(Iterator.zipKeyed({a: [1]})) &&
           drained([1].values().chunks(1)) &&
           drained([1].values().windows(1)) &&
           drained([1].values().take(1)) &&
           drained([1].values().flatMap(x => [x]))",
    );
}

/// Calling `next` or `return` on a helper from inside its own source's `next`
/// is a TypeError, for every state machine.
#[test]
fn helpers_reject_reentrant_next_and_return() {
    expect_true(
        "function reentrant(make, method) {
           var it, caught;
           function source() {
             return { next() { try { it[method](); } catch (e) { caught = e; } return { done: true }; },
                      return() { return {}; } };
           }
           it = make(source);
           it.next();
           return caught instanceof TypeError;
         }
         var makers = [
           s => Iterator.from(s()).map(x => x),
           s => Iterator.concat({ [Symbol.iterator]: s }),
           s => Iterator.zip([{ [Symbol.iterator]: s }]),
           s => Iterator.from(s()).chunks(2),
           s => Iterator.from(s()).windows(2),
         ];
         makers.every(m => reentrant(m, 'next')) &&
           reentrant(makers[0], 'return') && reentrant(makers[1], 'return') && reentrant(makers[2], 'return')",
    );
}

/// `take` closes its source once the limit is reached, and a failing `return`
/// surfaces as that call's error.
#[test]
fn take_reports_a_failing_close_when_the_limit_is_reached() {
    expect_true(
        "var source = { next() { return { value: 1, done: false }; }, return() { throw 'boom'; } };
         var it = Iterator.from(source).take(1);
         it.next();
         try { it.next(); false } catch (e) { e === 'boom' }",
    );
}

#[test]
fn flat_map_reports_errors_from_its_mapper_and_inner_iterators() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         var badInner = { [Symbol.iterator]() { return { next() { throw 'inner'; } }; } };
         thrown(() => [1].values().flatMap(x => badInner).next()) === 'inner' &&
           thrown(() => [1].values().flatMap(x => { throw 'mapper'; }).next()) === 'mapper' &&
           thrown(() => [1].values().flatMap(x => 5).next()) instanceof TypeError &&
           thrown(() => [1].values().map(x => { throw 'map'; }).next()) === 'map' &&
           thrown(() => [1].values().filter(x => { throw 'filter'; }).next()) === 'filter'",
    );
}

#[test]
fn concat_reports_bad_openers_and_exhausts_after_the_last_iterable() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         var values = [];
         var it = Iterator.concat([1], [2]);
         for (var r = it.next(); !r.done; r = it.next()) values.push(r.value);
         values.join() === '1,2' &&
           thrown(() => Iterator.concat({ [Symbol.iterator]() { return 5; } }).next()) instanceof TypeError &&
           thrown(() => Iterator.concat({ [Symbol.iterator]() { throw 'open'; } }).next()) === 'open' &&
           thrown(() => Iterator.concat({ [Symbol.iterator]() { return { get next() { throw 'next'; } }; } }).next()) === 'next'",
    );
}

/// An error from the active source ends the concatenation; a result that
/// cannot be built (heap exhausted) also closes the active source.
#[test]
fn concat_stops_on_source_errors() {
    expect_true(
        "var log = [];
         var it = Iterator.concat({ [Symbol.iterator]() {
           return { next() { throw 'step'; }, return() { log.push('closed'); return {}; } }; } });
         var caught; try { it.next(); } catch (e) { caught = e; }
         var after = it.next();
         caught === 'step' && after.done === true",
    );
}

#[test]
fn concat_return_reports_a_failing_active_source_close() {
    expect_true(
        "var it = Iterator.concat({ [Symbol.iterator]() {
           return { next() { return { value: 1, done: false }; }, return() { throw 'close'; } }; } });
         it.next();
         try { it.return(); false } catch (e) { e === 'close' }",
    );
}

#[test]
fn zip_strict_mode_reports_length_mismatches_and_source_failures() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         var logs = [];
         function closing(name, values) {
           var i = 0;
           return { [Symbol.iterator]() { return this; },
                    next() { return i < values.length ? { value: values[i++], done: false } : { done: true }; },
                    return() { logs.push(name); return {}; } };
         }
         var first = thrown(() => { var it = Iterator.zip([closing('a', [1, 2]), closing('b', [1])], { mode: 'strict' });
                                    it.next(); it.next(); });
         var second = thrown(() => { var it = Iterator.zip([closing('c', [1]), closing('d', [1, 2])], { mode: 'strict' });
                                     it.next(); it.next(); });
         var third = thrown(() => { var it = Iterator.zip([[1], { next() { throw 'later'; } }], { mode: 'strict' });
                                    it.next(); it.next(); });
         first instanceof TypeError && second instanceof TypeError && third === 'later'",
    );
}

#[test]
fn zip_shortest_mode_reports_a_failing_close() {
    expect_true(
        "var it = Iterator.zip([[1], { next() { return { value: 1, done: false }; }, return() { throw 'close'; } }]);
         it.next();
         try { it.next(); false } catch (e) { e === 'close' }",
    );
}

#[test]
fn zip_return_closes_every_source_started_or_not() {
    expect_true(
        "var log = [];
         function src(name, fail) {
           return { [Symbol.iterator]() { return this; },
                    next() { return { value: name, done: false }; },
                    return() { log.push(name); if (fail) throw 'fail ' + name; return {}; } };
         }
         var suspended = Iterator.zip([src('a'), src('b')]);
         suspended.next();
         var r1 = suspended.return();
         var fresh = Iterator.zip([src('c'), src('d')]);
         var r2 = fresh.return();
         var failing = Iterator.zipKeyed({ x: src('e', true), y: src('f', true) });
         failing.next();
         var caught; try { failing.return(); } catch (e) { caught = e; }
         var freshFailing = Iterator.zip([src('g', true)]);
         var caught2; try { freshFailing.return(); } catch (e) { caught2 = e; }
         r1.done && r2.done && caught === 'fail f' && caught2 === 'fail g' &&
           log.join() === 'b,a,d,c,f,e,g'",
    );
}

#[test]
fn zip_keyed_and_chunks_and_windows_report_source_failures() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         var broken = { next() { throw 'source'; } };
         thrown(() => Iterator.from(broken).chunks(2).next()) === 'source' &&
           thrown(() => Iterator.from(broken).windows(2).next()) === 'source' &&
           thrown(() => { var it = Iterator.from([1, 2, 3].values()).windows(2); it.next();
                          var w = Iterator.from({ i: 0, next() { if (this.i++ < 2) return { value: this.i, done: false }; throw 'slide'; } }).windows(2);
                          w.next(); w.next(); }) === 'slide' &&
           [].values().windows(2, 'allow-partial').next().done === true",
    );
}

#[test]
fn includes_covers_skips_and_every_completion() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         var log = [];
         function src(values, retFail) {
           var i = 0;
           return { next() { return i < values.length ? { value: values[i++], done: false } : { done: true }; },
                    return() { log.push('ret'); if (retFail) throw 'ret'; return {}; } };
         }
         Iterator.from(src([1, 2, 3])).includes(3) === true &&
           Iterator.from(src(['a', NaN, 3])).includes('a') === true &&
           Iterator.from(src([1, 2])).includes(9) === false &&
           Iterator.from(src([1, 2, 3])).includes(1, 1) === false &&
           thrown(() => Iterator.prototype.includes.call(1, 1)) instanceof TypeError &&
           thrown(() => Iterator.from(src([1])).includes(1, 2 ** 53)) instanceof RangeError &&
           thrown(() => Iterator.from(src([1])).includes(1, -1)) instanceof RangeError &&
           thrown(() => Iterator.from(src([1])).includes(1, 1.5)) instanceof TypeError &&
           thrown(() => Iterator.prototype.includes.call({ get next() { throw 'next'; } }, 1)) === 'next' &&
           thrown(() => Iterator.from({ next() { throw 'step'; } }).includes(1)) === 'step' &&
           thrown(() => Iterator.from(src([1], true)).includes(1)) === 'ret'",
    );
}

#[test]
fn join_covers_its_error_completions() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         thrown(() => Iterator.prototype.join.call(1)) instanceof TypeError &&
           thrown(() => [1].values().join({ toString() { throw 'sep'; } })) === 'sep' &&
           thrown(() => Iterator.prototype.join.call({ get next() { throw 'next'; } }, '-')) === 'next' &&
           thrown(() => Iterator.from({ next() { throw 'step'; } }).join()) === 'step' &&
           thrown(() => [1, { toString() { throw 'elem'; } }].values().join()) === 'elem' &&
           [1, null, undefined, 'x'].values().join('-') === '1---x'",
    );
}

#[test]
fn join_limits_the_result_length() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 64,
        ..VmConfig::default()
    })
    .unwrap();
    for source in [
        "['x'.repeat(30), 'z'].values().join('yyy')",
        "['x'.repeat(20), 'y'.repeat(20)].values().join('--')",
    ] {
        assert_eq!(
            run(&mut vm, source),
            Err(RuntimeError::StringLimit { limit: 64 }),
            "{source}"
        );
    }
    assert_eq!(
        run(&mut vm, "['a', 'b'].values().join('-')"),
        Ok(Value::String("a-b".into()))
    );
}

#[test]
fn terminal_helpers_cover_their_ordinary_and_failing_completions() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         var seen = [];
         [1, 2].values().forEach((v, i) => seen.push(v + ':' + i));
         seen.join() === '1:0,2:1' &&
           [1, 2].values().every(x => x > 0) === true &&
           [1, 2].values().every(x => x > 1) === false &&
           [1, 2].values().some(x => x > 5) === false &&
           [1, 2].values().some(x => x > 1) === true &&
           [1, 2].values().find(x => x > 5) === undefined &&
           [1, 2].values().find(x => x > 1) === 2 &&
           [1, 2, 3].values().reduce((a, b) => a + b) === 6 &&
           [1, 2, 3].values().reduce((a, b) => a + b, 10) === 16 &&
           thrown(() => [].values().reduce((a, b) => a + b)) instanceof TypeError &&
           thrown(() => [1].values().reduce((a, b) => { throw 'reduce'; }, 0)) === 'reduce' &&
           thrown(() => [1].values().reduce(7)) instanceof TypeError &&
           thrown(() => Iterator.from({ next() { throw 'step'; } }).toArray()) === 'step' &&
           thrown(() => Iterator.from({ next() { throw 'step'; } }).forEach(x => x)) === 'step' &&
           thrown(() => Iterator.from({ next() { throw 'step'; } }).every(x => x)) === 'step' &&
           thrown(() => Iterator.from({ next() { throw 'step'; } }).some(x => x)) === 'step' &&
           thrown(() => Iterator.from({ next() { throw 'step'; } }).find(x => x)) === 'step' &&
           thrown(() => [1].values().every(x => { throw 'cb'; })) === 'cb' &&
           thrown(() => [1].values().some(x => { throw 'cb'; })) === 'cb' &&
           thrown(() => [1].values().find(x => { throw 'cb'; })) === 'cb' &&
           thrown(() => [1].values().forEach(x => { throw 'cb'; })) === 'cb'",
    );
}

#[test]
fn early_exits_report_a_failing_close() {
    expect_true(
        "function thrown(f) { try { f(); return 'none'; } catch (e) { return e; } }
         function src() { return { next() { return { value: 1, done: false }; }, return() { throw 'ret'; } }; }
         thrown(() => Iterator.from(src()).every(x => false)) === 'ret' &&
           thrown(() => Iterator.from(src()).some(x => true)) === 'ret' &&
           thrown(() => Iterator.from(src()).find(x => true)) === 'ret'",
    );
}

#[test]
fn flat_map_return_closes_the_inner_iterator_then_the_outer_one() {
    expect_true(
        "var log = [];
         function inner(fail) {
           return { [Symbol.iterator]() { return this; },
                    next() { return { value: 1, done: false }; },
                    return() { log.push('inner'); if (fail) throw 'inner fail'; return {}; } };
         }
         var outer = { next() { return { value: 1, done: false }; }, return() { log.push('outer'); return {}; } };
         var it = Iterator.from(outer).flatMap(x => inner(false));
         it.next(); it.return();
         var failing = Iterator.from(outer).flatMap(x => inner(true));
         failing.next();
         var caught; try { failing.return(); } catch (e) { caught = e; }
         var badOuter = Iterator.from({ next() { return { value: 1, done: false }; }, return() { throw 'outer fail'; } }).map(x => x);
         var caught2; try { badOuter.return(); } catch (e) { caught2 = e; }
         log.join() === 'inner,outer,inner,outer' && caught === 'inner fail' && caught2 === 'outer fail'",
    );
}

#[test]
fn iterator_prototype_accessors_ignore_the_prototype_and_reject_bad_receivers() {
    expect_true(
        "function rejects(f) { try { f(); return false; } catch (e) { return e instanceof TypeError; } }
         var tag = Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag);
         var ctor = Object.getOwnPropertyDescriptor(Iterator.prototype, 'constructor');
         var plain = Object.create(Iterator.prototype);
         tag.set.call(plain, 'own tag');
         var withOwn = Object.create(Iterator.prototype, { [Symbol.toStringTag]: { value: 'old', writable: true, configurable: true } });
         tag.set.call(withOwn, 'new tag');
         var withCtor = Object.create(Iterator.prototype, { constructor: { value: 'old', writable: true, configurable: true } });
         ctor.set.call(withCtor, 'new ctor');
         Object.getOwnPropertyDescriptor(plain, Symbol.toStringTag).value === 'own tag' &&
           withOwn[Symbol.toStringTag] === 'new tag' && withCtor.constructor === 'new ctor' &&
           rejects(() => tag.set.call(1, 'x')) && rejects(() => tag.set.call(Iterator.prototype, 'x')) &&
           rejects(() => ctor.set.call(1, 'x')) && rejects(() => ctor.set.call(Iterator.prototype, 'x'))",
    );
}

#[test]
fn async_iteration_reports_bad_iterator_methods() {
    let mut vm = Vm::default();
    let source = "globalThis.log = [];
        async function attempt(name, iterable) {
          try { for await (var x of iterable) {} log.push(name + ':none'); }
          catch (e) { log.push(name + ':' + (e instanceof TypeError ? 'TypeError' : e)); }
        }
        attempt('getter', { get [Symbol.asyncIterator]() { throw 'getter'; } });
        attempt('call', { [Symbol.asyncIterator]() { throw 'call'; } });
        attempt('nonobject', { [Symbol.asyncIterator]() { return 5; } });
        attempt('next', { [Symbol.asyncIterator]() { return { get next() { throw 'next'; } }; } });
        attempt('syncnext', { [Symbol.iterator]() { return { get next() { throw 'syncnext'; } }; } });
        attempt('result', { [Symbol.asyncIterator]() { return { next() { return 5; } }; } });
        attempt('syncresult', { [Symbol.iterator]() { return { next() { return 5; } }; } });
        attempt('done', { [Symbol.asyncIterator]() { return { next() { return { get done() { throw 'done'; } }; } }; } });
        attempt('syncdone', { [Symbol.iterator]() { return { next() { return { get done() { throw 'syncdone'; } }; } }; } });";
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
        .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        run(&mut vm, "log.sort().join()"),
        Ok(Value::String(
            "call:call,done:done,getter:getter,next:next,nonobject:TypeError,result:TypeError,\
             syncdone:syncdone,syncnext:syncnext,syncresult:TypeError"
                .into()
        ))
    );
}

#[test]
fn tagged_templates_report_oversized_strings() {
    let mut vm = Vm::new(VmConfig {
        max_string_bytes: 64,
        ..VmConfig::default()
    })
    .unwrap();
    let source = format!("(function tag(s) {{ return s; }})`{}`", "x".repeat(40));
    assert_eq!(
        run(&mut vm, &source),
        Err(RuntimeError::StringLimit { limit: 64 })
    );
    assert_eq!(
        run(&mut vm, "(function tag(s) { return s.raw[0]; })`ok`"),
        Ok(Value::String("ok".into()))
    );
}
