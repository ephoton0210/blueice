// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};
use std::time::{Duration, Instant};

#[test]
fn catastrophic_match_is_killed_and_the_vm_recovers() {
    let code = compile(&parse("new RegExp('(a+)+$').test('a'.repeat(40)+'!')").unwrap()).unwrap();
    let mut vm = Vm::new(VmConfig {
        regex_timeout: Duration::from_millis(40),
        ..Default::default()
    })
    .unwrap();
    for _ in 0..2 {
        let start = Instant::now();
        assert_eq!(vm.execute(&code), Err(RuntimeError::RegexTimeout));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "matcher exceeded its deadline without termination"
        );
        assert_eq!(
            vm.execute(&compile(&parse("/a/.test('a')").unwrap()).unwrap())
                .unwrap(),
            Value::Bool(true)
        );
    }
}

#[test]
fn timeout_preserves_state_and_covers_string_protocols() {
    let mut vm = Vm::new(VmConfig {
        regex_timeout: Duration::from_millis(40),
        ..Default::default()
    })
    .unwrap();
    for (operation, last_index) in [
        ("r.exec(s)", 1.0),
        ("s.match(r)", 0.0),
        ("s.replace(r,'x')", 0.0),
        ("s.split(r)", 1.0),
        ("s.search(r)", 0.0),
        ("s.matchAll(r).next()", 1.0),
    ] {
        let source = format!("globalThis.r=new RegExp('(a+)+$','g'); let r=globalThis.r; r.lastIndex=1; let s='a'.repeat(40)+'!'; {operation}");
        assert_eq!(
            vm.execute(&compile(&parse(&source).unwrap()).unwrap()),
            Err(RuntimeError::RegexTimeout),
            "{operation}"
        );
        assert_eq!(
            vm.execute(&compile(&parse("globalThis.r.lastIndex").unwrap()).unwrap())
                .unwrap(),
            Value::Number(last_index),
            "{operation}"
        );
        assert_eq!(
            vm.execute(&compile(&parse("1+1").unwrap()).unwrap())
                .unwrap(),
            Value::Number(2.0)
        );
    }
}
