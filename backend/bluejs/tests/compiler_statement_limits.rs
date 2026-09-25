// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, compile_with_limit, parse, Bytecode, CompileError};

fn total_compiled_bytes(code: &Bytecode) -> usize {
    code.bytes().len()
        + code
            .child_code_units()
            .map(total_compiled_bytes)
            .sum::<usize>()
}

#[test]
fn statement_lowering_respects_each_byte_boundary() {
    let sources = [
        "label: while (false) { break label; }",
        "label: do { continue label; } while (false);",
        "label: switch (1) { case 1: break label; default: 2; }",
        "outer: for (let i = 0; i < 1; i++) { continue outer; }",
        "for (using resource = null; false;) {}",
        "async function f() { await using resource = null; }",
        "try { throw 1; } catch (error) { var value = error; } finally { 2; }",
        "with ({value: 1}) { var {value = 2} = {}; }",
        "class C { ['name'] = function() {}; #value = () => 1; }",
        "class C { @(() => function (v) { return v; }) value = 1; }",
        "function f(n) { 'use strict'; return n ? f(n - 1) : 1; }",
        "function f(n) { 'use strict'; return n && f(n - 1); }",
        "function f(n) { 'use strict'; return n || f(n - 1); }",
        "function f(n) { 'use strict'; return n ?? f(n - 1); }",
        "function f(n) { 'use strict'; return (f(n - 1)); }",
        "function f(n) { 'use strict'; return (0, f(n - 1)); }",
        "function f(n) { 'use strict'; return n ? Math.abs(n) : f(n - 1); }",
    ];
    for source in sources {
        let program = parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        let full = compile(&program).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        let upper = u32::try_from(total_compiled_bytes(&full)).unwrap();
        let mut first_success = None;
        for limit in 0..=upper {
            match compile_with_limit(&program, limit) {
                Ok(code) => {
                    first_success.get_or_insert(limit);
                    assert_eq!(code.bytes(), full.bytes(), "{source}: {limit}");
                }
                Err(CompileError::ProgramTooLarge) => {
                    assert!(first_success.is_none(), "{source}: {limit}");
                }
                Err(error) => panic!("{source}: {limit}: {error:?}"),
            }
        }
        assert!(first_success.is_some(), "{source}");
    }
}
