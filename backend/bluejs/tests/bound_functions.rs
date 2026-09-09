// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, HeapConfig, HeapError, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn check(sources: &[&str]) {
    for source in sources {
        assert_eq!(evaluate(source).unwrap_or_else(|e| panic!("{source}: {e}")), Value::Bool(true), "{source}");
    }
}

#[test]
fn bound_calls_preserve_receivers_and_prepend_arguments() {
    check(&[
        "let f=String.prototype.slice.bind('abcdef',1); f(4) === 'bcd' && f.call('wrong',3) === 'bc' && f.apply(null,[2]) === 'b'",
        "function f(a,b,c){return this.x+a+b+c;} let g=f.bind({x:'X'},'a').bind({x:'Y'},'b'); g('c') === 'Xabc'",
        "function f(){'use strict';return this;} f.bind()() === undefined && f.bind(null)() === null && f.bind(3)() === 3",
        "function f(){return this;} f.bind(null)() === globalThis && f.bind('a')().valueOf() === 'a'",
        "function f(){return (()=>this.x).bind({x:'wrong'});} f.call({x:'ok'})() === 'ok'",
        "function r(prefix,m,p,s){return this.x+prefix+p+s.length;} 'aba'.replaceAll('a',r.bind({x:'X'},'!')) === 'X!03bX!23'",
        "function r(prefix,m,a,p,s,g){return this.x+prefix+a+p+g.a;} 'aba'.replace(/(?<a>a)/g,r.bind({x:'X'},'!')) === 'X!a0abX!a2a'",
        "let f=String.prototype.charAt.bind('abc',1); String({toString:f}) === 'b' && Object.prototype.toString.call(f) === '[object Function]' && typeof f === 'function'",
        "let o={x:'s'}; function convert(p,h){return this.x+p+h;} String({[Symbol.toPrimitive]:convert.bind(o,':')}) === 's:string'",
        "let r=/a/g; r.constructor={[Symbol.species]:RegExp.bind(null)}; [...'aba'.matchAll(r)].length === 2",
    ]);
}

#[test]
fn bound_metadata_observes_length_name_and_original_prototype() {
    check(&[
        "function f(a,b,c){} let g=f.bind(null,1); g.length === 2 && g.name === 'bound f' && g.bind(null).name === 'bound bound f' && g.prototype === undefined",
        "function f(){} let g=f.bind(null); let l=Object.getOwnPropertyDescriptor(g,'length'); let n=Object.getOwnPropertyDescriptor(g,'name'); !l.writable && !l.enumerable && l.configurable && !n.writable && !n.enumerable && n.configurable && Reflect.ownKeys(g).join() === 'length,name'",
        "let p=Object.getPrototypeOf(String); let d=Object.getOwnPropertyDescriptor(p,'bind'); d.writable && !d.enumerable && d.configurable && p.bind.length === 1 && p.bind.name === 'bind'",
        "function f(){} let old={}; let next={}; Object.setPrototypeOf(f,old); let log=''; Object.defineProperty(f,'length',{get(){log+='l';Object.setPrototypeOf(f,next);return 3.9;}}); Object.defineProperty(f,'name',{get(){log+='n';return '\\ud800';}}); let g=String.bind.call(f,null,1); Object.getPrototypeOf(g) === old && g.length === 2 && g.name === 'bound \\ud800' && log === 'ln'",
        "function f(){} delete f.length; delete f.name; Object.setPrototypeOf(f,{get length(){throw 1;},name:'inherited'}); let g=String.bind.call(f,null); g.length === 0 && g.name === 'bound inherited'",
        "function f(){} Object.setPrototypeOf(f,null); let g=String.bind.call(f,null); Object.getPrototypeOf(g) === null && typeof g === 'function' && g() === undefined",
        "function f(){} Object.defineProperty(f,'name',{value:{toString(){throw 1;}}}); let g=f.bind(null); g.name === 'bound ' && g.toString().includes('[native code]')",
        "function f(){} Object.defineProperty(f,'length',{value:-0}); 1/f.bind(null).length === Infinity",
        "function f(){} let g=f.bind(null); Object.defineProperty(g,'name',{get(){throw 1;}}); g.toString().includes('[native code]')",
    ]);
    for (length, expected) in
        [("Infinity", "Infinity"), ("-Infinity", "0"), ("NaN", "0"), ("-0", "0"), ("-3.9", "0"), ("3.9", "2"), ("'3'", "0"), ("Symbol()", "0"), ("{valueOf(){throw 1;}}", "0")]
    {
        check(&[&format!("function f(){{}} Object.defineProperty(f,'length',{{value:{length}}}); f.bind(null,1).length === {expected}")]);
    }
}

#[test]
fn bound_constructors_substitute_new_target_by_identity() {
    check(&[
        "function F(a,b){this.x=a+b;} let ignored={}; let B=F.bind(ignored,2).bind(null,3); B.prototype={wrong:true}; let o=new B(); o.x === 5 && Object.getPrototypeOf(o) === F.prototype && ignored.x === undefined",
        "let B=String.bind(null,'abc'); let s=new B(); s.valueOf() === 'abc' && Object.getPrototypeOf(s) === String.prototype",
        "let B=RegExp.bind(null,'a'); let r=new B('g'); r.test('a') && r.flags === 'g' && Object.getPrototypeOf(r) === RegExp.prototype",
        "function F(a,b){this.x=a+b;} function N(){} let B=F.bind(null,2); let o=Reflect.construct(B,[3],N); o.x === 5 && Object.getPrototypeOf(o) === N.prototype",
        "function F(){} let B=F.bind(null); let o=Reflect.construct(F,[],B); Object.getPrototypeOf(o) === Object.prototype",
        "function F(){} let B=F.bind(null); B.prototype={x:1}; Object.getPrototypeOf(Reflect.construct(F,[],B)) === B.prototype && Object.getPrototypeOf(Reflect.construct(B,[],B)) === F.prototype",
        "function F(){return {x:1};} let B=F.bind(null); (new B()).x === 1",
    ]);
    for source in
        ["let B=(()=>{}).bind(null); new B()", "let B=String.prototype.slice.bind('a'); new B()", "Reflect.construct(String,[],(()=>{}).bind(null))", "new String.bind(null)"]
    {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn instanceof_uses_custom_protocols_and_bound_targets() {
    check(&[
        "let B=String.bind(null); let s=new B('a'); s instanceof B && s instanceof String && s instanceof Object && !('a' instanceof B) && !({} instanceof B)",
        "function F(){} let B=F.bind(null).bind(null); B.prototype={}; let o=new F(); o instanceof B && !(F.prototype instanceof F)",
        "function F(){} let B=F.bind(null); Object.defineProperty(F,Symbol.hasInstance,{value(v){return v === 'yes';}}); 'yes' instanceof B && !(new F() instanceof B)",
        "let seen=false; let target={get [Symbol.hasInstance](){seen=true;return function(v){return this === target && v === 3 ? {} : 0;};}}; 3 instanceof target && !(4 instanceof target) && seen",
        "function F(){} let B=F.bind(null); Object.defineProperty(B,Symbol.hasInstance,{value(v){return v === 1;}}); 1 instanceof B && !(new F() instanceof B)",
        "let has=String[Symbol.hasInstance]; !has.call({}, {}) && !has.call(null,{}) && !has.call(()=>{},1)",
        "function F(){} Object.setPrototypeOf(F,null); let B=String.bind.call(F,null); (new B()) instanceof B && (new F()) instanceof F",
        "function F(){} Object.defineProperty(F,Symbol.hasInstance,{value:undefined}); (new F()) instanceof F",
        "function F(){} Object.defineProperty(F,Symbol.hasInstance,{value:null}); (new F()) instanceof F",
        "function F(){} let B=F.bind(null); Object.defineProperty(F,Symbol.hasInstance,{value:()=>true}); String[Symbol.hasInstance].call(B,1)",
        "let n=0; let f=()=>{}; Object.defineProperty(f,'prototype',{get(){n++;return {};}}); !(1 instanceof f) && n === 0 && !({} instanceof f) && n === 1",
        "let p=Object.getPrototypeOf(String); let d=Object.getOwnPropertyDescriptor(p,Symbol.hasInstance); !d.writable && !d.enumerable && !d.configurable && d.value.name === '[Symbol.hasInstance]' && d.value.length === 1",
        "let f=()=>{}; f.prototype={}; Object.create(f.prototype) instanceof f && !({} instanceof f)",
    ]);
    for source in ["1 instanceof 1", "({}) instanceof {}", "1 instanceof {[Symbol.hasInstance]:1}", "({}) instanceof (()=>{})", "function F(){} F.prototype=1; ({}) instanceof F"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn bound_getter_errors_and_target_throws_propagate() {
    for (source, thrown) in [
        ("function f(){} Object.defineProperty(f,'length',{get(){throw 'length';}}); f.bind(null)", "length"),
        ("function f(){} Object.defineProperty(f,'name',{get(){throw 'name';}}); f.bind(null)", "name"),
        ("function f(){throw 'call';} f.bind(null)()", "call"),
        ("function F(){throw 'construct';} let B=F.bind(null); new B()", "construct"),
        ("1 instanceof {get [Symbol.hasInstance](){throw 'getter';}}", "getter"),
        ("1 instanceof {[Symbol.hasInstance](){throw 'method';}}", "method"),
    ] {
        assert_eq!(evaluate(source), Err(RuntimeError::Thrown(Value::String(thrown.into()))), "{source}");
    }
    for source in ["String.bind.call(null)", "String.bind.call({get length(){throw 1;}})", "String.bind.call(1)"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn bound_internal_edges_survive_collection_and_are_reclaimed() {
    let mut vm = Vm::new(VmConfig { heap: HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: 256 * 1024 }, ..Default::default() }).unwrap();
    let code = compile(&parse("String; Object; globalThis; 0").unwrap()).unwrap();
    vm.execute(&code).unwrap();
    let baseline = vm.heap().stats().managed_bytes;
    let code = compile(&parse("let target=function(a){return this.x+a.y;}; let receiver={x:'A'}; let arg={y:'B'}; globalThis.bound=target.bind(receiver,arg); globalThis.ids=[target,receiver,arg]; target=null;receiver=null;arg=null;globalThis.ids").unwrap()).unwrap();
    let Value::Object(ids) = vm.execute(&code).unwrap() else { panic!("expected handle array") };
    let retained: Vec<_> = (0..3)
        .map(|i| match vm.heap().get(ids, i.to_string()).unwrap() {
            Value::Object(id) => id,
            value => panic!("expected retained object: {value:?}"),
        })
        .collect();
    let code = compile(&parse("delete globalThis.ids; for(let i=0;i<40;i++){let garbage={};} globalThis.bound()").unwrap()).unwrap();
    assert_eq!(vm.execute(&code).unwrap(), Value::String("AB".into()));
    assert!(retained.iter().all(|id| vm.heap().contains(*id)));
    assert!(!vm.heap().contains(ids));
    let code = compile(&parse("delete globalThis.bound; 0").unwrap()).unwrap();
    vm.execute(&code).unwrap();
    assert!(retained.iter().all(|id| !vm.heap().contains(*id)));
    assert_eq!(vm.heap().stats().managed_bytes, baseline);
    let code = compile(&parse("let f=function(a){throw a;}.bind(null,{message:'kept'}); f()").unwrap()).unwrap();
    let Err(RuntimeError::Thrown(Value::Object(thrown))) = vm.execute(&code) else { panic!("expected thrown object") };
    assert_eq!(vm.heap().get(thrown, "message").unwrap(), Value::String("kept".into()));
    vm.execute(&compile(&parse("0").unwrap()).unwrap()).unwrap();
    assert!(!vm.heap().contains(thrown));
    assert_eq!(vm.heap().stats().managed_bytes, baseline);
}

#[test]
fn retained_arguments_are_charged_and_failed_bindings_release_roots() {
    let mut vm = Vm::default();
    let code = compile(&parse("String.bind(null)").unwrap()).unwrap();
    vm.execute(&code).unwrap();
    let empty_bytes = vm.heap().stats().managed_bytes;
    for source in ["String.bind('a'.repeat(1000))", "String.bind(null,'a'.repeat(1000))", "String.bind(null,Symbol('a'.repeat(1000)))"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        vm.execute(&code).unwrap();
        assert!(vm.heap().stats().managed_bytes >= empty_bytes + 2000, "{source}");
    }
    let mut vm = Vm::new(VmConfig { max_string_bytes: 64, ..Default::default() }).unwrap();
    let code = compile(&parse("function f(){} Object.defineProperty(f,'name',{value:'x'.repeat(32)}); f.bind(null)").unwrap()).unwrap();
    assert_eq!(vm.execute(&code), Err(RuntimeError::StringLimit { limit: 64 }));
    let heap = HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: 128 * 1024 };
    let mut vm = Vm::new(VmConfig { heap, ..Default::default() }).unwrap();
    vm.execute(&compile(&parse("String; 0").unwrap()).unwrap()).unwrap();
    let baseline = vm.heap().stats().managed_bytes;
    let code = compile(&parse("String.bind(null, 'x'.repeat(128*1024))").unwrap()).unwrap();
    for _ in 0..2 {
        assert!(matches!(vm.execute(&code), Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. }))));
        assert_eq!(vm.heap().stats().managed_bytes, baseline);
    }
    assert_eq!(vm.execute(&compile(&parse("String.bind(null,'ok')()").unwrap()).unwrap()).unwrap(), Value::String("ok".into()));
}

#[test]
fn bound_chains_use_fuel_without_adding_execution_frames() {
    check(&[
        "function F(a,b){this.x=a+b;return a+b;} let B=F; for(let i=0;i<80;i++){B=B.bind(null);} B(2,3) === 5 && (new B(2,3)).x === 5 && (new F()) instanceof B",
        "function F(){} Object.setPrototypeOf(F,null); let B=F; for(let i=0;i<80;i++){B=String.bind.call(B,null);} (new B()) instanceof B",
        "function f(...args){return args.join('');} let g=f; for(let i=0;i<80;i++){g=g.bind(null,i%10);} g('!') === '0123456789'.repeat(8)+'!'",
    ]);
    let mut vm = Vm::new(VmConfig { instruction_budget: 1000, ..Default::default() }).unwrap();
    // Setup uses several executions so the eventual chain traversal, rather
    // than setup's bytecode, is what exhausts the per-execution fuel budget.
    vm.execute(&compile(&parse("globalThis.f=String; globalThis.o=new String('x'); 0").unwrap()).unwrap()).unwrap();
    let code = compile(&parse("for(let i=0;i<20;i++){globalThis.f=globalThis.f.bind(null);} 0").unwrap()).unwrap();
    for _ in 0..55 {
        vm.execute(&code).unwrap();
    }
    for source in ["globalThis.f()", "new globalThis.f()", "globalThis.o instanceof globalThis.f"] {
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()), Err(RuntimeError::InstructionLimit), "{source}");
    }
    assert_eq!(vm.execute(&compile(&parse("'ok'").unwrap()).unwrap()).unwrap(), Value::String("ok".into()));
}
