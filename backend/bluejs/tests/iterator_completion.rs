// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-pipeline regressions for the shared iterator completion model.
use blueice_bluejs::{compile, parse, HeapConfig, RuntimeError, Value, Vm, VmConfig};

fn execute(vm: &mut Vm, source: &str) -> Result<Value, RuntimeError> {
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn elisions_step_without_reading_values_and_stop_after_done() {
    for pattern in [
        "let [,]=source;",
        "var [,]=source;",
        "[,]=source;",
        "function f([,]){} f(source);",
        "for(let [,] of [source]){}",
    ] {
        let source = format!(
            "let steps=0;let closed=0;let source={{[Symbol.iterator](){{return {{
             next(){{steps++;return {{done:false,get value(){{throw 99;}}}};}},
             return(){{closed++;return {{}};}}}};}}}};
             {pattern} steps===1&&closed===1"
        );
        assert_eq!(
            execute(&mut Vm::default(), &source),
            Ok(Value::Bool(true)),
            "{pattern}"
        );
    }
    let source = "let steps=0;let source={[Symbol.iterator](){return {
        next(){steps++;return {done:true,get value(){throw 99;}};},
        return(){throw 88;}};}};let [,,,x,...rest]=source;
        steps===1&&x===undefined&&rest.length===0";
    assert_eq!(execute(&mut Vm::default(), source), Ok(Value::Bool(true)));
}

#[test]
fn undeclared_for_heads_use_assignment_patterns_and_close_on_abrupt_assignment() {
    for source in [
        "let first=0;let second=0;let rest;for([first,second=3,...rest] of [[1,undefined,4,5]]){}first===1&&second===3&&rest.length===2&&rest[0]===4&&rest[1]===5",
        "let target={};for({value:target.value,missing=3,...target.rest} of [{value:7,extra:9}]){}target.value===7&&target.missing===undefined&&target.rest.extra===9",
        "let target={};for(target.value of [1,2,3]){}target.value===3",
        "let initial='';for([initial] in {alpha:1}){}initial==='a'",
        "let value=0;for(value of [2,4]){}value===4",
    ] {
        assert_eq!(
            execute(&mut Vm::default(), source),
            Ok(Value::Bool(true)),
            "{source}"
        );
    }

    let source = "globalThis.closed=0;let values={[Symbol.iterator](){return {
        next(){return {value:null};},return(){globalThis.closed++;return {};}};}};
        try{for({value:target} of values){}}catch(error){}globalThis.closed===1";
    assert_eq!(
        execute(&mut Vm::default(), source),
        Ok(Value::Bool(true)),
        "{source}"
    );
}

#[test]
fn iterator_origin_errors_do_not_close_rest_iterators() {
    for next in [
        "throw 17;",
        "return {get done(){throw 17;}};",
        "return {done:false,get value(){throw 17;}};",
    ] {
        for pattern in [
            "let [...rest]=source;",
            "var rest;[...rest]=source;",
            "function f([...rest]){} f(source);",
        ] {
            let source = format!(
                "globalThis.closed=0;let source={{[Symbol.iterator](){{return {{
                 next(){{{next}}},return(){{globalThis.closed++;throw 88;}}}};}}}};{pattern}"
            );
            let mut vm = Vm::default();
            assert_eq!(
                execute(&mut vm, &source),
                Err(RuntimeError::Thrown(Value::Number(17.0))),
                "{source}"
            );
            assert_eq!(
                execute(&mut vm, "globalThis.closed"),
                Ok(Value::Number(0.0)),
                "{source}"
            );
        }
    }
    let mut vm = Vm::default();
    assert!(matches!(
        execute(
            &mut vm,
            "globalThis.closed=0;let source={[Symbol.iterator](){return {
        next(){return 1;},return(){globalThis.closed++;return {};}};}};let [...rest]=source;"
        ),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(
        execute(&mut vm, "globalThis.closed"),
        Ok(Value::Number(0.0))
    );
}

#[test]
fn nested_abrupt_completions_close_only_active_iterators_in_reverse_order() {
    let mut vm = Vm::default();
    let source = "globalThis.trace='';
        let inner={[Symbol.iterator](){return {next(){throw 17;},return(){globalThis.trace+='i';return {};}};}};
        let outer={[Symbol.iterator](){return {next(){return {value:inner};},return(){globalThis.trace+='o';throw 88;}};}};
        let [[...rest]]=outer;";
    assert_eq!(
        execute(&mut vm, source),
        Err(RuntimeError::Thrown(Value::Number(17.0)))
    );
    assert_eq!(
        execute(&mut vm, "globalThis.trace"),
        Ok(Value::String("o".into()))
    );

    let source = "globalThis.trace='';
        let inner={[Symbol.iterator](){return {next(){return {value:undefined};},return(){globalThis.trace+='i';throw 88;}};}};
        let outer={[Symbol.iterator](){return {next(){return {value:inner};},return(){globalThis.trace+='o';throw 99;}};}};
        function fail(){throw {original:17};}let [[x=fail()]]=outer;";
    let Err(RuntimeError::Thrown(Value::Object(thrown))) = execute(&mut vm, source) else {
        panic!("expected thrown object")
    };
    assert_eq!(
        vm.heap().get(thrown, "original").unwrap(),
        Value::Number(17.0)
    );
    assert_eq!(
        execute(&mut vm, "globalThis.trace"),
        Ok(Value::String("io".into()))
    );
}

#[test]
fn generator_return_propagates_iterator_close_errors_from_suspended_destructuring() {
    for close in ["throw marker;", "return null;"] {
        let source = format!(
            "let closed=0;let marker={{}};let iterator={{next(){{return {{value:undefined,done:false}};}},return(){{closed++;{close}}}}};
            let iterable={{[Symbol.iterator](){{return iterator;}}}};
            function* values(){{let target;[target=yield]=iterable;}}
            let valuesIterator=values();valuesIterator.next();
            let caught=false;try{{valuesIterator.return();}}catch(error){{caught={caught};}}
            caught&&closed===1",
            caught = if close == "throw marker;" {
                "error===marker"
            } else {
                "error.name==='TypeError'"
            },
        );
        assert_eq!(
            execute(&mut Vm::default(), &source),
            Ok(Value::Bool(true)),
            "{source}"
        );
    }

    let source = "
        let closes=[];let marker={};
        let inner={next(){return {value:undefined,done:false}},return(){closes.push('inner');throw marker;}};
        let innerIterable={[Symbol.iterator](){return inner;}};
        let outer={next(){return {value:innerIterable,done:false}},return(){closes.push('outer');return {};}};
        let outerIterable={[Symbol.iterator](){return outer;}};
        function* values(){let target;[[target=yield]]=outerIterable;}
        let valuesIterator=values();valuesIterator.next();
        let caught=false;try{valuesIterator.return();}catch(error){caught=error===marker;}
        caught&&closes.join(',')==='inner,outer'
    ";
    assert_eq!(
        execute(&mut Vm::default(), source),
        Ok(Value::Bool(true)),
        "{source}"
    );
}

#[test]
fn rest_values_survive_collection_during_subsequent_steps() {
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            ..HeapConfig::default()
        },
        ..VmConfig::default()
    })
    .unwrap();
    let source = "let count=0;let source={[Symbol.iterator](){return {
        next(){count++;let garbage={payload:'x'.repeat(32768)};
        return count<40?{value:{index:count}}:{done:true};}};}};
        let [...rest]=source;let sum=0;for(let item of rest){sum+=item.index;}
        rest.length===39&&sum===780";
    assert_eq!(execute(&mut vm, source), Ok(Value::Bool(true)));
}
