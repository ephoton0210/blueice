// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, CompileError, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn labelled_transfers_select_their_target_and_preserve_finally_completion() {
    for source in [
        "let n=0; outer: for(let i=0;i<3;i++){for(let j=0;j<3;j++){n++;continue outer;}} n === 3",
        "let n=0; outer: for(let i=0;i<3;i++){for(let j=0;j<3;j++){n++;break outer;}} n === 1",
        "let n=0; outer: inner: for(;n<3;n++){if(n===1) continue outer;} n === 3",
        "let n=0; outer: try { n=1; break outer; } finally { n+=2; } n === 3",
        "eval('target: { 5; break target; 9; }') === 5",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn labelled_breaks_close_the_iterators_they_leave() {
    let source = "let closed=0; let iterator={next(){return {value:1,done:false}},return(){closed++;return {done:true}}}; let iterable={[Symbol.iterator](){return iterator}}; outer: for(let value of iterable){break outer;} closed === 1";
    assert_eq!(evaluate(source), Ok(Value::Bool(true)));
}

#[test]
fn labelled_early_errors_and_sloppy_let_asi_are_classified() {
    for source in [
        "label: let value=1",
        "label: class Value {}",
        "label: async function value() {}",
        "label: function* value() {}",
        "L: let\n[a] = 0",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
    assert!(matches!(
        compile(&parse("label: { continue label; }").unwrap()),
        Err(CompileError::InvalidSyntax(_))
    ));
    assert!(matches!(
        compile(&parse("break missing;").unwrap()),
        Err(CompileError::InvalidSyntax(_))
    ));
    assert!(matches!(
        compile(&parse("\"use strict\"; yield: 1").unwrap()),
        Err(CompileError::InvalidSyntax(_))
    ));
    assert!(matches!(
        compile(&parse("\"use strict\"; L: let\n{} ").unwrap()),
        Err(CompileError::InvalidSyntax(_))
    ));
    assert!(compile(&parse("if(false) { L: let\n{} }").unwrap()).is_ok());
    assert!(compile(&parse("if(false) { L: let\nvalue = 1; }").unwrap()).is_ok());
}
