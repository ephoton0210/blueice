// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::heap::BoundFunction;

impl Vm {
    pub(super) fn bind_function(
        &mut self,
        target: Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        if !self.is_callable(&target)? {
            return Err(RuntimeError::TypeError("bind requires a callable".into()));
        }
        let id = target.object_id().unwrap();
        let prototype = self.heap.prototype(id)?;
        let bound = BoundFunction {
            target: id,
            this: native::argument(args, 0).clone(),
            args: args.iter().skip(1).cloned().collect(),
            constructible: self.is_constructor(&target)?,
        };
        let count = bound.args.len();
        let function = self.with_roots(|heap| heap.alloc_bound_function(bound, prototype))?;
        self.stack.push(Value::Object(function));
        let mut length = 0.0;
        if self
            .object_get_own_property(id, &"length".into())?
            .is_some()
        {
            if let Value::Number(number) = self.get_property(&target, &"length".into())? {
                // ECMAScript's max(0, …) returns +0 for a -0 target length.
                // `f64::max` may retain the receiver's -0 sign for equal
                // operands, so make that observable zero explicitly.
                let candidate = number.trunc() - count as f64;
                length = if candidate.is_nan() || candidate <= 0.0 {
                    0.0
                } else {
                    candidate
                };
            }
        }
        self.define_data(
            function,
            "length",
            Value::Number(length),
            false,
            false,
            true,
        )?;
        let target_name = self.get_property(&target, &"name".into())?;
        let mut name = JsString::from("bound ");
        if let Value::String(target_name) = target_name {
            native::append(&mut name, &target_name, self.config.max_string_bytes)?;
        }
        self.check_string(&Value::String(name.clone()))?;
        self.define_data(function, "name", Value::String(name), false, false, true)?;
        Ok(Value::Object(function))
    }

    // `ordinary` selects the entry algorithm: direct @@hasInstance calls
    // start with OrdinaryHasInstance; the operator starts with hook lookup.
    pub(super) fn has_instance(
        &mut self,
        value: Value,
        mut target: Value,
        mut ordinary: bool,
    ) -> Result<bool, RuntimeError> {
        // `[[GetPrototypeOf]]` can invoke a Proxy trap.  Retain the value,
        // target, and constructor prototype while that arbitrary code runs.
        let base = self.stack.len();
        self.stack
            .extend([value.clone(), target.clone(), Value::Undefined]);
        let result = (|| {
            loop {
                self.charge_step()?;
                if !ordinary {
                    if !matches!(target, Value::Object(_)) {
                        return Err(RuntimeError::TypeError(
                            "instanceof target must be an object".into(),
                        ));
                    }
                    let method =
                        self.get_method(&target, &JsSymbol::well_known("hasInstance").into())?;
                    if let Value::Object(id) = method {
                        // The intrinsic can be tail-dispatched here. Custom hooks
                        // still use normal call rooting, error and depth handling.
                        // A Test262 facade around the foreign intrinsic must
                        // take this path too: its ordinary algorithm belongs
                        // to the current Realm's value and prototype chain.
                        let native = self
                            .heap
                            .native_function(id)?
                            .or(self.test262_foreign_native_function(id)?);
                        if native != Some(NativeFunction::HasInstance) {
                            let result = self.call_native(method, target, vec![value], false)?;
                            return self.to_boolean(&result);
                        }
                    } else if !self.is_callable(&target)? {
                        return Err(RuntimeError::TypeError(
                            "instanceof target must be callable".into(),
                        ));
                    }
                }
                if !self.is_callable(&target)? {
                    return Ok(false);
                }
                if let Some(bound) = self.heap.bound_function(target.object_id().unwrap())? {
                    target = Value::Object(bound.target);
                    self.stack[base + 1] = target.clone();
                    ordinary = false;
                    continue;
                }
                let Value::Object(mut object) = value else {
                    return Ok(false);
                };
                let prototype = self.get_property(&target, &"prototype".into())?;
                let Value::Object(prototype) = prototype else {
                    return Err(RuntimeError::TypeError(
                        "instanceof prototype must be an object".into(),
                    ));
                };
                self.stack[base + 2] = Value::Object(prototype);
                loop {
                    self.charge_step()?;
                    self.stack[base] = Value::Object(object);
                    let Some(parent) = self.object_get_prototype(object)? else {
                        return Ok(false);
                    };
                    if parent == prototype {
                        return Ok(true);
                    }
                    object = parent;
                }
            }
        })();
        self.stack.truncate(base);
        result
    }
}

/// Allocation-failure coverage for the built-ins whose allocations happen
/// after a lot of shared setup: each operation runs on a VM prepared by its
/// setup and collected, with only a few more bytes allowed, and the allowance
/// grows until the operation succeeds. Every allocation the
/// operation makes is therefore, in turn, the one that no longer fits, and each
/// must be reported as the heap limit (never a panic or another error).
#[cfg(test)]
mod allocation_exhaustion {
    use super::*;
    use crate::{compile, parse};

    /// Runs `operation` on a VM prepared by `setup`, with `extra` more bytes
    /// than the setup left managed, and reports how it ended: `Ok` when it
    /// succeeded, otherwise the error, which must be the heap limit.
    fn attempt(
        extra: usize,
        setup: &dyn Fn(&mut Vm),
        operation: &dyn Fn(&mut Vm) -> Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        let mut vm = Vm::default();
        setup(&mut vm);
        // Running anything releases the setup's result, which stays rooted
        // until the next execution; without this the operation's first
        // allocations would be paid for by that released room.
        execute(&mut vm, "0").unwrap();
        vm.with_roots(|heap| {
            heap.collect_major();
            Ok(())
        })
        .unwrap();
        let limit = vm.heap.allow_only(extra);
        let outcome = operation(&mut vm);
        if let Err(error) = &outcome {
            assert_eq!(
                format!("{error:?}"),
                format!("Heap(HeapLimitExceeded {{ limit: {limit} }})"),
                "extra {extra}"
            );
        }
        outcome
    }

    /// Runs `operation` after `setup` with the allowance growing from nothing
    /// until it succeeds, so every allocation it makes is in turn the one that
    /// no longer fits. The smallest amount of room any allocation needs is
    /// larger than the eight bytes the allowance grows by, so none is skipped.
    fn every_allocation_fails_cleanly(
        setup: &dyn Fn(&mut Vm),
        operation: &dyn Fn(&mut Vm) -> Result<Value, RuntimeError>,
    ) {
        let mut extra = 0;
        while attempt(extra, setup, operation).is_err() {
            extra += 8;
        }
    }

    fn execute(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
    }

    /// `operation` after `setup`, both scripts.
    fn sweep(setup: &str, operation: &str) {
        every_allocation_fails_cleanly(
            &|vm| {
                execute(vm, setup).unwrap();
            },
            &|vm| execute(vm, operation),
        );
    }

    #[test]
    fn collection_builtins() {
        for operation in [
            "Object.groupBy([1, 2, 3], x => x % 2 ? 'a' : 'b')",
            "Map.groupBy([1, 2, 3], x => x % 2)",
            "Object.fromEntries([['a', 1], ['b', 2]])",
            "Array.from({ length: 2, 0: 1, 1: 2 })",
            "Array.from(5)",
            "Array.from([1, 2], x => x)",
            "Array.from.call(function () {}, [1])",
            "Array.from.call(function () {}, { length: 1 })",
            "Array.of(1, 2)",
            "Array.of.call(function () {}, 1)",
            "Array.of.call(undefined, 1, 2)",
            "Array.from.call(undefined, [1])",
            "Array.from.call(undefined, { length: 1 })",
        ] {
            // Warmed by a first run of the same call, so only the call's own
            // allocations remain.
            sweep(operation, operation);
        }
    }

    /// Iterables whose iterators hand back results that already exist, so the
    /// iteration itself allocates nothing that could hide a failure of the
    /// allocation that follows it.
    const PREBUILT_ITERABLES: &str = "
        globalThis.numbers = [{ value: 1, done: false }, { value: 2, done: false }, { value: 3, done: false }, { value: undefined, done: true }];
        globalThis.numberIterable = { [Symbol.iterator]() { var i = 0; return { next() { return numbers[i++]; } }; } };
        globalThis.pairs = [{ value: ['a', 1], done: false }, { value: ['b', 2], done: false }, { value: undefined, done: true }];
        globalThis.pairIterable = { [Symbol.iterator]() { var i = 0; return { next() { return pairs[i++]; } }; } };";

    #[test]
    fn collection_builtins_over_prebuilt_results() {
        for operation in [
            "Object.groupBy(numberIterable, Math.abs)",
            "Map.groupBy(numberIterable, Math.abs)",
            "Object.fromEntries(pairIterable)",
        ] {
            every_allocation_fails_cleanly(
                &|vm| {
                    execute(vm, PREBUILT_ITERABLES).unwrap();
                    execute(vm, operation).unwrap();
                },
                &|vm| execute(vm, operation),
            );
        }
    }

    #[test]
    fn collection_iteration() {
        // Cold: the empty collections were built without the iterator protocol,
        // so creating an iterator also creates the iterator prototypes.
        for operation in [
            "m.keys()",
            "m.entries(); m.values()",
            "s.values()",
            "s.entries()",
            "s.keys()",
            "m.keys(); m.keys()",
            "[...m]",
        ] {
            sweep(
                "globalThis.m = new Map(); globalThis.s = new Set();",
                operation,
            );
        }
        // Warm: each iterator already exists.
        for operation in [
            "mk.next()",
            "mv.next()",
            "me.next()",
            "sk.next()",
            "sv.next()",
            "se.next()",
            "me.next(); me.next()",
            "se.next(); se.next()",
            "m.forEach(() => 0)",
            "s.forEach(() => 0)",
        ] {
            sweep(
                "globalThis.m = new Map([[1, 2]]); globalThis.s = new Set([1]); \
                 globalThis.mk = m.keys(); globalThis.mv = m.values(); globalThis.me = m.entries(); \
                 globalThis.sk = s.keys(); globalThis.sv = s.values(); globalThis.se = s.entries();",
                operation,
            );
        }
    }

    #[test]
    fn immutable_array_buffers() {
        for operation in [
            "b.sliceToImmutable(0, 4)",
            "b.transferToImmutable()",
            "new ArrayBuffer(4).transferToImmutable(8)",
        ] {
            sweep("globalThis.b = new ArrayBuffer(8);", operation);
        }
    }

    #[test]
    fn dates() {
        for operation in [
            "new Date(2020, 0, 1, 2, 3, 4, 5)",
            "Reflect.construct(Date, [0], Object)",
            "new Date('2020-03-04')",
            "new Date(Date.UTC(2020, 1)).toISOString()",
        ] {
            sweep(operation, operation);
        }
    }

    #[test]
    fn bound_functions() {
        sweep("globalThis.f = function (a, b) {};", "f.bind(null, 1)");
        sweep("globalThis.C = class {};", "C.bind()");
    }

    #[test]
    fn intl_services() {
        for (setup, operation) in [
            (
                "Intl.DisplayNames",
                "new Intl.DisplayNames('en', { type: 'language', languageDisplay: 'standard', fallback: 'none', style: 'short' })",
            ),
            (
                "globalThis.names = new Intl.DisplayNames('en', { type: 'region' });",
                "names.resolvedOptions()",
            ),
            (
                "globalThis.names = new Intl.DisplayNames('en', { type: 'language' });",
                "names.resolvedOptions()",
            ),
            (
                "globalThis.names = new Intl.DisplayNames('en', { type: 'region' });",
                "names.of('US')",
            ),
            ("Intl.DurationFormat", "new Intl.DurationFormat('en')"),
            (
                "Intl.RelativeTimeFormat",
                "new Intl.RelativeTimeFormat('en', { numeric: 'auto', style: 'short' })",
            ),
            (
                "globalThis.rtf = new Intl.RelativeTimeFormat('en');",
                "rtf.formatToParts(2, 'hour')",
            ),
            (
                "globalThis.rtf = new Intl.RelativeTimeFormat('en');",
                "rtf.resolvedOptions()",
            ),
            (
                "Intl.Collator",
                "new Intl.Collator('en', { numeric: true, caseFirst: 'upper', usage: 'search', sensitivity: 'base', ignorePunctuation: true, collation: 'phonebk' })",
            ),
            ("Intl.Collator", "new Intl.Collator('en')"),
            ("Intl.Collator", "Intl.Collator.supportedLocalesOf(['en', 'de'])"),
            ("Intl.getCanonicalLocales", "Intl.getCanonicalLocales(['en-us', 'de-at'])"),
            (
                "Intl.getCanonicalLocales",
                "Intl.getCanonicalLocales([new Intl.Locale('en'), 'de'])",
            ),
            ("Intl.supportedValuesOf", "Intl.supportedValuesOf('calendar')"),
            (
                "Intl.DisplayNames",
                "Intl.DisplayNames.supportedLocalesOf('en')",
            ),
            (
                "Intl.RelativeTimeFormat",
                "Intl.RelativeTimeFormat.supportedLocalesOf('en')",
            ),
            ("Intl.Segmenter", "new Intl.Segmenter('en').segment('a')"),
            (
                "globalThis.segmenter = new Intl.Segmenter('en');",
                "segmenter.segment('a')",
            ),
            (
                "globalThis.segments = new Intl.Segmenter('en').segment('ab');",
                "segments[Symbol.iterator]()",
            ),
            ("Intl.DisplayNames", "try { new Intl.DisplayNames('en') } catch (e) { e instanceof TypeError }"),
        ] {
            sweep(setup, operation);
        }
    }

    #[test]
    fn temporal_durations() {
        for operation in [
            "Temporal.Duration.compare(d, d)",
            "d.toString()",
            "d.round('day')",
            "d.total('day')",
            "d.round({ largestUnit: 'day', relativeTo: { year: 2020, month: 1, day: 1 } })",
        ] {
            sweep(
                "globalThis.d = Temporal.Duration.from({ hours: 40 });",
                operation,
            );
        }
    }

    #[test]
    fn test262_host() {
        // The intrinsics the installer would otherwise create first.
        let precreate = "Error; TypeError; RangeError; SyntaxError; ReferenceError; EvalError; URIError; isNaN; isFinite; parseInt; parseFloat; JSON; String";
        every_allocation_fails_cleanly(
            &|vm| {
                execute(vm, precreate).unwrap();
            },
            &|vm| vm.install_test262_harness().map(|()| Value::Undefined),
        );
        every_allocation_fails_cleanly(
            &|vm| {
                execute(vm, precreate).unwrap();
            },
            &|vm| {
                vm.install_test262_is_html_dda()?;
                Ok(Value::Undefined)
            },
        );
        for operation in [
            "assert.deepEqual([{ a: [1] }], [{ a: [1] }])",
            "$262.detachArrayBuffer(buffer)",
            "$262.evalScript('var evaluated = 1')",
            "__bluejsTest262NumberFormatPrecisionMatrix(['en'], ['latn'], {}, { '1': '1', '1.500': '1.5', '1.625': '1.625', '1.750': '1.75', '1.875': '1.875', '2.000': '2' })",
            "__bluejsTest262NumberFormatPrecisionMatrix(['en'], ['thai'], {}, { '1': '1', '1.500': '1.5', '1.625': '1.625', '1.750': '1.75', '1.875': '1.875', '2.000': '2' })",
        ] {
            every_allocation_fails_cleanly(
                &|vm| {
                    vm.install_test262_harness().unwrap();
                    execute(vm, "globalThis.buffer = new ArrayBuffer(8);").unwrap();
                },
                &|vm| execute(vm, operation),
            );
        }
    }
}

/// Native loops charge the instruction budget as they go: under every budget
/// from one upward each operation must either finish or stop with the
/// instruction limit, from wherever the budget happens to run out.
#[cfg(test)]
mod instruction_budgets {
    use crate::{compile, parse, RuntimeError, Vm, VmConfig};

    fn stops_cleanly_at_every_budget(source: &str) {
        let program = compile(&parse(source).unwrap()).unwrap();
        let mut budget = 1;
        loop {
            let mut vm = Vm::new(VmConfig {
                instruction_budget: budget,
                ..VmConfig::default()
            })
            .unwrap();
            vm.install_test262_harness().unwrap();
            match vm.execute(&program) {
                Ok(_) => break,
                Err(error) => assert_eq!(error, RuntimeError::InstructionLimit, "{source}"),
            }
            budget += 1;
        }
        assert!(budget > 3, "{source}");
    }

    #[test]
    fn array_from_over_an_array_like() {
        stops_cleanly_at_every_budget("Array.from({ length: 3, 0: 1, 1: 2, 2: 3 }).length");
    }

    #[test]
    fn the_number_format_precision_matrix() {
        stops_cleanly_at_every_budget(
            "__bluejsTest262NumberFormatPrecisionMatrix(['en', 'de'], ['latn', 'thai'], {}, { '1': '1', '1.500': '1.5', '1.625': '1.625', '1.750': '1.75', '1.875': '1.875', '2.000': '2' })",
        );
    }
}

/// The reference tables of `tests/cov_g1_tables` run inside the crate too. The
/// integration tests exercise the library as a dependency, while this test
/// binary compiles the library again, and coverage is measured per compiled
/// copy of each function, so the in-crate copy needs the same behaviours.
#[cfg(test)]
mod reference_tables {
    use crate::{compile, parse, RuntimeError, Value, Vm, VmConfig};

    /// Evaluates `expr` and reports its `String` conversion, or the error it
    /// threw as `throws Name` (with `: message` when `with_message`; object handles, whose heap serial
    /// numbers vary per run, are cut from the message).
    fn observe(vm: &mut Vm, expr: &str, with_message: bool) -> Value {
        let detail = if with_message {
            " + ': ' + e.message.split(' ObjectId {')[0]"
        } else {
            ""
        };
        let source = format!(
            "(function(){{ try {{ return String({expr}) }} catch (e) {{ return 'throws ' + e.constructor.name{detail} }} }})()"
        );
        vm.execute(&compile(&parse(&source).unwrap()).unwrap())
            .expect("the wrapper catches every error")
    }

    fn check(cases: &[(&str, &str)], with_message: bool, new_vm: &dyn Fn() -> Vm) {
        let mismatches: Vec<_> = cases
            .iter()
            .map(|(expr, expected)| (*expr, *expected, observe(&mut new_vm(), expr, with_message)))
            .filter(|(_, expected, actual)| *actual != Value::String((*expected).into()))
            .collect();
        assert_eq!(mismatches, Vec::<(&str, &str, Value)>::new());
    }

    macro_rules! table {
        ($module:ident, $file:literal, $cases:ident) => {
            mod $module {
                include!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/cov_g1_tables/",
                    $file
                ));
                pub(super) fn cases() -> &'static [(&'static str, &'static str)] {
                    $cases
                }
            }
        };
    }

    table!(collections, "collections.in", COLL_CASES);
    table!(functions, "functions.in", FN_CASES);
    table!(intl, "intl.in", INTL_CASES);
    table!(date, "date.in", DATE_CASES);
    table!(duration_relative, "duration_relative.in", CASES);
    table!(immutable_array_buffers, "immutable_array_buffers.in", CASES);
    table!(test262_host, "test262_host.in", CASES);

    #[test]
    fn a_bound_function_name_is_bounded_by_the_string_limit() {
        let tiny = || {
            Vm::new(VmConfig {
                max_string_bytes: 8,
                ..VmConfig::default()
            })
            .unwrap()
        };
        // A string name is appended to "bound ", which is already too long;
        // a name that is not a string leaves "bound " alone, still too long.
        for source in [
            "(function () {}).bind()",
            "(function () { class C { static name() {} } return C.bind() })()",
        ] {
            assert_eq!(
                tiny().execute(&compile(&parse(source).unwrap()).unwrap()),
                Err(RuntimeError::StringLimit { limit: 8 }),
                "{source}"
            );
        }
    }

    #[test]
    fn collections_match() {
        check(collections::cases(), false, &Vm::default);
    }

    #[test]
    fn functions_match() {
        check(functions::cases(), false, &Vm::default);
    }

    #[test]
    fn intl_matches() {
        check(intl::cases(), false, &Vm::default);
    }

    #[test]
    fn dates_match() {
        check(date::cases(), false, &Vm::default);
    }

    #[test]
    fn temporal_durations_match() {
        check(duration_relative::cases(), true, &Vm::default);
    }

    #[test]
    fn immutable_array_buffers_match() {
        check(immutable_array_buffers::cases(), true, &Vm::default);
    }

    #[test]
    fn the_test262_host_matches() {
        check(test262_host::cases(), true, &|| {
            let mut vm = Vm::new(VmConfig::default()).unwrap();
            vm.install_test262_harness().unwrap();
            vm.install_test262_is_html_dda().unwrap();
            vm
        });
    }
}
