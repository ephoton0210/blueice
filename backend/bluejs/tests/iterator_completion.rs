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
fn iterator_from_exposes_the_shared_iterator_protocol_without_public_slots() {
    let source = r#"
        let calls = 0;
        let source = {
            next() { calls++; return calls < 3 ? { value: calls, done: false } : { done: true }; },
            return() { return { value: 'closed', done: true }; },
        };
        let wrapped = Iterator.from(source);
        let iteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()));
        let descriptor = Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag);
        let first = wrapped.next();
        let closed = wrapped.return();
        Iterator.prototype === iteratorPrototype &&
          Object.getPrototypeOf(wrapped) !== Iterator.prototype &&
          wrapped[Symbol.iterator]() === wrapped &&
          first.value === 1 && !first.done &&
          closed.value === 'closed' && closed.done &&
          calls === 1 &&
          typeof descriptor.get === 'function' && typeof descriptor.set === 'function' &&
          Iterator.prototype[Symbol.toStringTag] === 'Iterator' &&
          Object.keys(wrapped).length === 0
    "#;
    assert_eq!(execute(&mut Vm::default(), source), Ok(Value::Bool(true)));
}

#[test]
fn iterator_from_validates_inputs_and_disposal_calls_return() {
    let source = r#"
        let disposed = 0;
        let iterator = { return() { disposed++; return { done: true }; } };
        let wrapper = Iterator.from(iterator);
        let invalid = false;
        try { Iterator.from(1); } catch (error) { invalid = error instanceof TypeError; }
        let abstract = false;
        try { new Iterator(); } catch (error) { abstract = error instanceof TypeError; }
        wrapper[Symbol.dispose]();
        invalid && abstract && disposed === 1
    "#;
    assert_eq!(execute(&mut Vm::default(), source), Ok(Value::Bool(true)));
}

#[test]
fn iterator_terminal_helpers_share_step_and_early_close_protocol() {
    let source = r#"
        let closes = 0;
        let source = {
            index: 0,
            next() { return this.index < 4 ? { value: this.index++, done: false } : { done: true }; },
            return() { closes++; return { done: true }; },
        };
        let values = Iterator.from(source);
        let mapped = [0, 1, 2][Symbol.iterator]();
        let array = mapped.toArray();
        let every = values.every(value => value < 2);
        let some = Iterator.from([2, 4, 5]).some(value => value % 2);
        let found = Iterator.from([3, 6, 8]).find(value => value % 2 === 0);
        let sum = Iterator.from([1, 2, 3]).reduce((total, value) => total + value, 0);
        let seen = '';
        Iterator.from(['a', 'b']).forEach((value, index) => { seen += value + index; });
        array.length === 3 && array[2] === 2 && !every && some && found === 6 &&
          sum === 6 && seen === 'a0b1' && closes === 1
    "#;
    assert_eq!(execute(&mut Vm::default(), source), Ok(Value::Bool(true)));
}

#[test]
fn iterator_map_and_filter_are_lazy_close_on_abrupt_completion_and_have_private_state() {
    let source = r#"
        let nextCalls = 0;
        let closes = 0;
        let source = {
            next() { return nextCalls++ < 2 ? { value: nextCalls, done: false } : { done: true }; },
            return() { closes++; return { done: true }; },
        };
        let mapped = Iterator.from(source).map((value, index) => value * 10 + index);
        let first = mapped.next();
        let second = mapped.next();
        let done = mapped.next();
        let closedAgain = mapped.return();
        let filtered = Iterator.from([1, 2, 3, 4]).filter((value, index) => value % 2 && index === 0);
        let filteredFirst = filtered.next();
        let filteredDone = filtered.next();
        let mapperClosed = Iterator.from({
          next() { return { value: 1, done: false }; },
          return() { closes++; return {}; },
        }).map(() => { throw 1; });
        let abrupt = false;
        try { mapperClosed.next(); } catch (error) { abrupt = error === 1; }
        let validationClosed = {
          __proto__: Iterator.prototype,
          get next() { throw 'next must stay unobserved'; },
          return() { closes++; return {}; },
        };
        let invalid = false;
        try { validationClosed.map({}); } catch (error) { invalid = error instanceof TypeError; }
        nextCalls === 3 && first.value === 10 && !first.done &&
          second.value === 21 && !second.done && done.done && closedAgain.done &&
          filteredFirst.value === 1 && !filteredFirst.done && filteredDone.done &&
          Object.getPrototypeOf(mapped)[Symbol.toStringTag] === 'Iterator Helper' &&
          Object.keys(mapped).length === 0 && abrupt && invalid && closes === 2
    "#;
    assert_eq!(execute(&mut Vm::default(), source), Ok(Value::Bool(true)));
}

#[test]
fn iterator_take_and_drop_coerce_before_reading_next_and_preserve_lazy_close() {
    let source = r#"
        let effects = '';
        let index = 0;
        let source = {
          get next() {
            effects += 'next';
            return () => index < 3 ? { value: index++, done: false } : { done: true };
          },
          return() { effects += 'return'; return {}; },
        };
        let taken = Iterator.prototype.take.call(source, {
          valueOf() { effects += 'number'; return 2; },
        });
        let first = taken.next();
        let second = taken.next();
        let done = taken.next();
        let dropped = Iterator.from([0, 1, 2, 3]).drop(2);
        let droppedFirst = dropped.next();
        let droppedSecond = dropped.next();
        let rangeNext = 0;
        let rangeClosed = 0;
        let rangeSource = {
          get next() { rangeNext++; return () => ({ done: true }); },
          return() { rangeClosed++; return {}; },
        };
        let rangeError = false;
        try { Iterator.prototype.take.call(rangeSource, undefined); }
        catch (error) { rangeError = error instanceof RangeError; }
        effects === 'numbernextreturn' &&
          first.value === 0 && !first.done && second.value === 1 && !second.done && done.done &&
          droppedFirst.value === 2 && !droppedFirst.done && droppedSecond.value === 3 && !droppedSecond.done &&
          rangeError && rangeNext === 0 && rangeClosed === 1
    "#;
    assert_eq!(execute(&mut Vm::default(), source), Ok(Value::Bool(true)));
}

#[test]
fn iterator_includes_uses_same_value_zero_and_validates_skip_without_coercion() {
    let source = r#"
        let closes = 0;
        let sourceIndex = 0;
        let source = {
          next() {
            return sourceIndex++ < 3
              ? { value: [0, NaN, 2][sourceIndex - 1], done: false }
              : { done: true };
          },
          return() { closes++; return {}; },
        };
        let found = Iterator.prototype.includes.call(source, NaN, 1);
        let invalidNextGets = 0;
        let invalidCloses = 0;
        let invalid = {
          get next() { invalidNextGets++; return () => ({ done: true }); },
          return() { invalidCloses++; return {}; },
        };
        let rangeError = false;
        try { Iterator.prototype.includes.call(invalid, 0, -1); }
        catch (error) { rangeError = error instanceof RangeError; }
        let coerced = false;
        let typeError = false;
        try {
          Iterator.prototype.includes.call(invalid, 0, {
            valueOf() { coerced = true; return 0; },
          });
        } catch (error) { typeError = error instanceof TypeError; }
        found && closes === 1 && rangeError && typeError && !coerced &&
          invalidNextGets === 0 && invalidCloses === 2
    "#;
    assert_eq!(execute(&mut Vm::default(), source), Ok(Value::Bool(true)));
}

#[test]
fn iterator_join_coerces_separator_before_next_and_closes_on_content_errors() {
    let source = r#"
        let effects = '';
        let index = 0;
        let iterator = {
          get next() {
            effects += 'next';
            return () => index++ < 2 ? { value: ['one', null][index - 1], done: false } : { done: true };
          },
        };
        let separator = { toString() { effects += 'separator'; return '&&'; } };
        let joined = Iterator.prototype.join.call(iterator, separator);
        let closed = 0;
        let contentError = false;
        let throwing = {
          next() { return { value: { toString() { throw 1; } }, done: false }; },
          return() { closed++; return {}; },
        };
        try { Iterator.prototype.join.call(throwing); }
        catch (error) { contentError = error === 1; }
        joined === 'one&&' && effects === 'separatornext' && contentError && closed === 1
    "#;
    assert_eq!(execute(&mut Vm::default(), source), Ok(Value::Bool(true)));
}

#[test]
fn iterator_flat_map_is_lazy_flattens_one_level_and_closes_active_inner_iterator() {
    let source = r#"
        let outerSteps = 0;
        let mapperCalls = 0;
        let flattened = Iterator.from({
          next() { return outerSteps++ < 2 ? { value: outerSteps, done: false } : { done: true }; },
          return() { return {}; },
        }).flatMap((value, index) => { mapperCalls++; return [value, index]; });
        let first = flattened.next();
        let second = flattened.next();
        let third = flattened.next();
        let innerCloses = 0;
        let outerCloses = 0;
        let active = Iterator.from({
          next() { return { value: 1, done: false }; },
          return() { outerCloses++; return {}; },
        }).flatMap(() => ({
          next() { return { value: 9, done: false }; },
          return() { innerCloses++; return {}; },
        }));
        let activeValue = active.next();
        active.return();
        mapperCalls === 2 && outerSteps === 2 && first.value === 1 && second.value === 0 &&
          third.value === 2 && activeValue.value === 9 && innerCloses === 1 && outerCloses === 1
    "#;
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
