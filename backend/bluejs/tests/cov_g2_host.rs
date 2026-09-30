// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A host function installed by the embedder, called from a script.
use blueice_bluejs::{compile, parse, HostFunctionError, HostValue, RuntimeError, Value, Vm};

fn run(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn a_host_function_receives_primitive_arguments_and_returns_a_primitive() {
    let mut vm = Vm::default();
    vm.install_host_function("hostSum", 2, |args: &[HostValue]| {
        let numbers = args.iter().map(|arg| match arg {
            HostValue::Number(number) => Ok(*number),
            _ => Err(HostFunctionError::new("numbers only")),
        });
        Ok(HostValue::Number(
            numbers.sum::<Result<f64, HostFunctionError>>()?,
        ))
    })
    .unwrap();
    assert_eq!(run(&mut vm, "hostSum(1, 2, 3)"), Ok(Value::Number(6.0)));
    assert_eq!(
        run(
            &mut vm,
            "try { hostSum(1, 'x'); } catch (e) { e instanceof TypeError && e.message }"
        ),
        Ok(Value::String("numbers only".into()))
    );
}
