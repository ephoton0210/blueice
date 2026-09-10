// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Math namespace behavior through parse, compile and VM dispatch.
use blueice_bluejs::{HeapConfig, RuntimeError, Value, Vm, VmConfig, compile, parse};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn constants_descriptors_and_unary_functions_match_ecmascript_numbers() {
    for source in [
        "Math.E > 2 && Math.LN10 > 2 && Math.LN2 < 1 && Math.LOG10E < 1 && Math.LOG2E > 1 && Math.PI > 3 && Math.SQRT1_2 < 1 && Math.SQRT2 > 1",
        "Math.abs(-3) === 3 && Math.acos(1) === 0 && Math.acosh(1) === 0 && Math.asin(0) === 0 && Math.asinh(0) === 0 && Math.atan(0) === 0 && Math.atanh(0) === 0",
        "Math.ceil(0.1) === 1 && Math.cbrt(27) === 3 && Math.cos(0) === 1 && Math.cosh(0) === 1 && Math.exp(0) === 1 && Math.expm1(0) === 0",
        "Math.floor(0.9) === 0 && Math.fround(1.1) !== 1.1 && Math.log(1) === 0 && Math.log1p(0) === 0 && Math.log2(8) === 3 && Math.log10(100) === 2",
        "Math.sin(0) === 0 && Math.sinh(0) === 0 && Math.sqrt(9) === 3 && Math.tan(0) === 0 && Math.tanh(0) === 0 && Math.trunc(-1.9) === -1",
        "typeof Math === 'object' && Math.abs.length === 1 && Math.max.length === 2 && Math.random.length === 0 && Object.getPrototypeOf(Math) === Object.prototype",
        "Math.abs(NaN) !== Math.abs(NaN) && Math.sqrt(-1) !== Math.sqrt(-1) && Math.sign(NaN) !== Math.sign(NaN)",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn variadic_binary_and_integer_math_preserve_special_values() {
    for source in [
        "Math.atan2(1,0) > 1 && Math.pow(2,8) === 256 && Math.hypot(3,4) === 5",
        "Math.imul(0xffffffff,5) === -5 && Math.clz32(1) === 31 && Math.clz32() === 32",
        "Math.max() === -Infinity && Math.min() === Infinity && Math.max(2,1) === 2 && Math.min(1,2) === 1 && Math.max(-0,0) === 0 && 1/Math.max(-0,0) === Infinity && 1/Math.min(-0,0) === -Infinity",
        "Math.max(1,NaN) !== Math.max(1,NaN) && Math.hypot(NaN,Infinity) === Infinity && Math.hypot(NaN,1) !== Math.hypot(NaN,1)",
        "Math.round(1.5) === 2 && Math.round(-1.5) === -1 && 1/Math.round(-0.1) === -Infinity && 1/Math.round(-0) === -Infinity && Math.round(Infinity) === Infinity && Math.round(NaN) !== Math.round(NaN) && 1/Math.sign(-0) === -Infinity",
        "Math.random() >= 0 && Math.random() < 1",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn methods_coerce_arguments_in_left_to_right_order() {
    let source = "let log=''; let one={valueOf(){log+='a';return 3}}; let two={valueOf(){log+='b';return 4}}; Math.hypot(one,two) === 5 && log === 'ab'";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn math_initialization_releases_its_root_when_native_installation_exhausts_the_heap() {
    let code = compile(&parse("Math.PI").unwrap()).unwrap();
    let limit = 75_000;
    let mut vm = Vm::new(VmConfig { heap: HeapConfig { nursery_capacity: 1, major_threshold_bytes: limit, max_heap_bytes: limit }, ..VmConfig::default() }).unwrap();
    assert!(matches!(vm.execute(&code), Err(RuntimeError::Heap(_))));
    assert!(vm.heap().stats().managed_bytes < limit);
}
