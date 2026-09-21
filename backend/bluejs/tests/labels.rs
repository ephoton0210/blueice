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
fn labelled_breaks_cross_switch_finally_and_nested_iterator_cleanup_in_order() {
    let source = "let trace=[];let outerIterator={next(){return {value:1,done:false};},return(){trace.push('outer-close');return {done:true};}};let innerIterator={next(){return {value:1,done:false};},return(){trace.push('inner-close');return {done:true};}};let outerIterable={[Symbol.iterator](){return outerIterator;}};let innerIterable={[Symbol.iterator](){return innerIterator;}};outer:for(let outerValue of outerIterable){try{switch(1){case 1:for(let innerValue of innerIterable){break outer;}}}finally{trace.push('finally');}}trace.join(',')==='inner-close,finally,outer-close'";
    assert_eq!(evaluate(source), Ok(Value::Bool(true)));
}

#[test]
fn labelled_break_close_error_replaces_the_break_after_finally() {
    let source = "let trace=[];let outerIterator={next(){return {value:1,done:false};},return(){trace.push('outer-close');return {done:true};}};let innerIterator={next(){return {value:1,done:false};},return(){trace.push('inner-close');throw 'inner-error';}};let outerIterable={[Symbol.iterator](){return outerIterator;}};let innerIterable={[Symbol.iterator](){return innerIterator;}};let error;try{outer:for(let outerValue of outerIterable){try{for(let innerValue of innerIterable){break outer;}}finally{trace.push('finally');}}}catch(caught){error=caught;}trace.join(',')==='inner-close,finally,outer-close'&&error==='inner-error'";
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
        // A strict-reserved word is not a valid LabelIdentifier, a syntax
        // error the parser reports itself.
        "\"use strict\"; yield: 1",
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
        compile(&parse("\"use strict\"; L: let\n{} ").unwrap()),
        Err(CompileError::InvalidSyntax(_))
    ));
    assert!(compile(&parse("if(false) { L: let\n{} }").unwrap()).is_ok());
    assert!(compile(&parse("if(false) { L: let\nvalue = 1; }").unwrap()).is_ok());
}
