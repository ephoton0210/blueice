// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{RuntimeError, Value, Vm, compile, parse};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn check(sources: &[&str]) {
    for source in sources {
        assert_eq!(evaluate(source).unwrap_or_else(|e| panic!("{source}: {e}")), Value::Bool(true), "{source}");
    }
}

#[test]
fn object_conversion_and_replacement_callbacks() {
    check(&[
        "String({}) === '[object Object]' && String(['a',null,'b']) === 'a,,b'",
        "String({toString: function(){return 'abc';}}) === 'abc'",
        "let s=new String('abc'); s.toString=function(){return 'xyz';}; s.slice(1) === 'yz' && s.valueOf() === 'abc'",
        "let log=''; let s={toString:()=>{log+='s';return 'abc';}}; let n={valueOf:()=>{log+='n';return 1;}}; String.prototype.slice.call(s,n) === 'bc' && log === 'sn'",
        "let n=0; let out='aba'.replaceAll('a',function(m,p,s){n++;return p+s.length;}); out === '3b5' && n === 2",
        "function make(x){return function(){x++;return x;};} let f=make(1); f() === 2 && f() === 3",
        "let obj={s:'abc', f:function(){return this.s.toUpperCase();}}; obj.f() === 'ABC'",
        "'x'.replace('x',()=>({toString:()=> 'done'})) === 'done'",
    ]);
    for source in ["String({toString:1,valueOf:2})", "String({toString:()=>({}),valueOf:()=>({})})"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn symbols_and_string_iteration() {
    check(&[
        "typeof Symbol.iterator === 'symbol' && Symbol('x') !== Symbol('x')",
        "let k=Symbol('x'); let o={[k]:3,x:4}; o[k] === 3 && o.x === 4",
        "String(Symbol('x')) === 'Symbol(x)'",
        "String({[Symbol.toPrimitive]:function(h){return h;}}) === 'string'",
        "let it='A😀\\ud800'[Symbol.iterator](); it[Symbol.iterator]() === it && it.next().value === 'A' && it.next().value === '😀' && it.next().value === '\\ud800' && it.next().done && it.next().value === undefined",
        "String.prototype[Symbol.iterator].name === '[Symbol.iterator]' && String.prototype[Symbol.iterator].length === 0",
        "let out=''; for(let x of 'A😀B'){out+=x.length;} out === '121'",
    ]);
    for source in ["new String(Symbol())", "''.concat(Symbol())", "String.prototype[Symbol.iterator].call(null)"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn string_symbol_protocols_observe_receiver_and_order() {
    check(&[
        "let o={[Symbol.match]:function(s){return s+'!';}}; 'abc'.match(o) === 'abc!'",
        "let o={[Symbol.search]:()=>42}; 'abc'.search(o) === 42",
        "let o={[Symbol.split]:function(s,n){return s+n;}}; 'abc'.split(o,2) === 'abc2'",
        "let o={[Symbol.replace]:function(s,r){return r+s;}}; 'abc'.replace(o,'!') === '!abc' && 'x'.replaceAll(o,'?') === '?x'",
        "let o={[Symbol.matchAll]:()=>42}; 'abc'.matchAll(o) === 42",
        "let o={toString:()=> 'b',[Symbol.match]:false}; 'abc'.includes(o)",
    ]);
    for source in ["'x'.includes({[Symbol.match]:true})", "'x'.startsWith({[Symbol.match]:true})", "'x'.endsWith({[Symbol.match]:true})", "'x'.match({[Symbol.match]:3})"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn regexp_backed_string_methods() {
    check(&[
        "'abc123'.search('[0-9]+') === 3 && 'abc'.search('z') === -1",
        "let m='abc123'.match('([0-9]+)'); m[0] === '123' && m[1] === '123' && m.index === 3 && m.input === 'abc123'",
        "let m='a1b2'.match(new RegExp('[0-9]','g')); m.length === 2 && m[1] === '2'",
        "'abc'.match('z') === null && 'abc'.match()[0] === ''",
        "'a1b22'.replace(new RegExp('([0-9]+)','g'),'[$1]') === 'a[1]b[22]'",
        "'a1b2'.replaceAll(new RegExp('[0-9]','g'),function(m,p){return p;}) === 'a1b3'",
        "let a='a1b2'.split(new RegExp('([0-9])')); a.length === 5 && a[1] === '1' && a[4] === ''",
        "let it='a1b2'.matchAll(new RegExp('([0-9])','g')); let a=it.next().value; let b=it.next().value; a[1] === '1' && a.index === 1 && b.index === 3 && it.next().done",
        "let r=new RegExp('a','g'); r.lastIndex=1; let it='aa'.matchAll(r); it.next().value.index === 1 && r.lastIndex === 1",
    ]);
    for source in ["'a'.matchAll(new RegExp('a'))", "'a'.replaceAll(new RegExp('a'),'b')", "'a'.includes(new RegExp('a'))"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn builtin_and_string_exotic_descriptors() {
    check(&[
        "let d=Object.getOwnPropertyDescriptor(String.prototype,'slice'); d.writable && !d.enumerable && d.configurable",
        "let d=Object.getOwnPropertyDescriptor(String.prototype.slice,'length'); d.value === 2 && !d.writable && !d.enumerable && d.configurable",
        "let d=Object.getOwnPropertyDescriptor(new String('😀'),'0'); d.value === '\\ud83d' && !d.writable && d.enumerable && !d.configurable",
        "let s=new String('abc'); Object.defineProperty(s,'0',{value:'a'}); s[0] === 'a'",
        "Object.keys(new String('abc')).length === 3 && Object.keys(String.prototype).length === 0",
        "let o={}; Object.defineProperty(o,'toString',{get:()=> ()=> 'ok'}); String(o) === 'ok'",
    ]);
    for source in ["'use strict'; let s=new String('a'); s[0]='b'", "Object.defineProperty(new String('a'),'0',{value:'b'})"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn ecma262_locale_methods() {
    check(&[
        "'ABCΣ'.toLocaleLowerCase() === 'abcς' && 'straße'.toLocaleUpperCase() === 'STRASSE'",
        "'e\\u0301'.localeCompare('é') === 0",
        "'a'.localeCompare('b') < 0 && 'b'.localeCompare('a') > 0",
        "String.prototype.localeCompare.call(123,123) === 0",
        "'\\ud800A'.toLocaleLowerCase() === '\\ud800a'",
    ]);
}

#[test]
fn observable_conversion_order_and_gc_pressure() {
    let sources = [
        "let log=''; let receiver={toString:()=>{log+='r';return 'abc';}}; let search={toString:()=>{log+='s';return 'b';}}; let pos={valueOf:()=>{log+='p';return 1;}}; String.prototype.includes.call(receiver,search,pos) && log === 'rsp'",
        "let s=new String('old'); s[Symbol.toPrimitive]=()=> 'new'; String(s) === 'new' && s.toString() === 'old' && s.slice(1) === 'ew'",
        "let s={toString:()=> 'abc'}; let search={[Symbol.match]:function(x){return x === s;}}; String.prototype.match.call(s,search)",
        "let o={}; Object.defineProperty(o,'toString',{get:()=> ()=> 'ok'}); String(o) === 'ok'",
        "let o={}; Object.defineProperty(o,'x',{value:1,writable:true}); o.x=2; Object.getOwnPropertyDescriptor(o,'x').value === 2",
        "function f(){let x={a:'ok'}; return ()=>x.a;} let g=f(); let garbage={}; g() === 'ok'",
        "let n={valueOf:()=>1}; let o={x:n}; o.x++ === 1 && o.x === 2",
        "let r=new RegExp('(?<x>a)(b)?','dg'); let m=r.exec('a'); m.groups.x === 'a' && m[2] === undefined && m.indices[0][1] === 1",
        "let r=new RegExp('','gu'); let it='😀'.matchAll(r); it.next().value.index === 0 && it.next().value.index === 2 && it.next().done",
        "let a=[]; for(let c of 'ab'){a[a.length]=()=>c;} a[0]() === 'a' && a[1]() === 'b'",
    ];
    for source in sources {
        let mut vm = Vm::new(blueice_bluejs::VmConfig {
            heap: blueice_bluejs::HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: 256 * 1024 },
            ..Default::default()
        })
        .unwrap();
        assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Bool(true), "{source}");
    }
}

#[test]
fn regexp_protocol_edge_cases() {
    check(&[
        "'abc'.replace(new RegExp('(?<x>b)'),'[$<x>][$1][$2]') === 'a[b][b][$2]c'",
        "'a'.replace(new RegExp('(a)(b)?'),'$01|$10|$2') === 'a|a0|'",
        "let a='😀'.match(new RegExp('.','g')); a.length === 2 && a[0] === '\\ud83d'",
        "let a='😀'.match(new RegExp('.','gu')); a.length === 1 && a[0] === '😀'",
        "'😀'.split(new RegExp('','u')).length === 1 && '😀'.split(new RegExp('')).length === 2",
        "let r=new RegExp('b','g'); r.lastIndex=7; 'abc'.search(r) === 1 && r.lastIndex === 7",
        "let r=new RegExp('a','y'); r.lastIndex=1; r.exec('ba').index === 1 && r.lastIndex === 2 && r.exec('ba') === null && r.lastIndex === 0",
        "let n=0; let r={flags:'g',global:true,unicode:false,exec:function(s){n++;return n===1?{0:'x',index:0,length:1}:null;}}; RegExp.prototype[Symbol.replace].call(r,'xx','y') === 'yx'",
        "let r=new RegExp('a','g'); let n=0; r.exec=function(s){n++;return null;}; 'aa'.match(r) === null && n === 1",
        "let o={flags:'', [Symbol.match]:true, [Symbol.replace]:()=>1}; o[Symbol.match]=false; 'x'.replaceAll(o,'') === 1",
    ]);
    for source in ["new RegExp('[')", "new RegExp('a','gg')", "new RegExp('a','uv')", "new RegExp('a','z')"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::SyntaxError(_))), "{source}");
    }
}

#[test]
fn regexp_literals_and_template_tags_use_parser_context() {
    check(&[
        "'ab12'.match(/\\d+/)[0] === '12'",
        "let r=/[a/]+/gi; r.test('A/a') && 8/2/2 === 2",
        "let r=/a/; let x=8; x/=2; r.test('a') && x === 4",
        "if(true) /a/.test('a'); else false",
        "function f(){return /a/;} f().test('a')",
        "let RegExp=1; /a/.test('a')",
        "String.raw`a\\nb${2}c` === 'a\\\\nb2c'",
        "let tag=String.raw; tag`\\u{not valid}` === '\\\\u{not valid}'",
        "function f(t){return t.raw[0]+t[0];} f`\\n` === '\\\\n\\n'",
    ]);
}

#[test]
fn boxing_construction_and_template_identity() {
    check(&[
        "String(new Number(12)) === '12' && String(new Boolean(false)) === 'false'",
        "let s=Symbol('x'); let o=Object(s); o.valueOf() === s && o.toString() === 'Symbol(x)'",
        "function C(){} let s=Reflect.construct(String,['abc'],C); Object.getPrototypeOf(s) === C.prototype && String.prototype.valueOf.call(s) === 'abc'",
        "function F(){this.x='a';} String(new F()) === '[object Object]'",
        "let seen; function tag(t){let same=seen===t; seen=t; return same;} function f(){return tag`x`;} !f() && f()",
        "function tag(t){t.raw[0]='wrong';t[0]='wrong';t.extra=1;return t.raw[0] === 'x' && t[0] === 'x' && t.extra === undefined;} tag`x`",
        "let iterator='x'[Symbol.iterator](); Object.getPrototypeOf(Object.getPrototypeOf(iterator))[Symbol.iterator].call(iterator) === iterator",
        "String.prototype.slice.apply('abc',[1,2]) === 'b'",
    ]);
}

#[test]
fn string_iterators_work_with_spread_and_throwing_callbacks() {
    check(&[
        "let a=[...'A😀B']; a.length === 3 && a[1] === '😀'",
        "String.fromCodePoint(...[65,128512]) === 'A😀'",
        "function f(...args){return String.fromCharCode(...args);} f(65,66) === 'AB'",
        "let a=[0,...'ab',,3]; a.length === 5 && a[1] === 'a' && a[3] === undefined",
        "let closed=0; let o={[Symbol.iterator]:function(){return {next:()=>({value:'a',done:false}),return:()=>{closed++;return {};}};}}; for(let c of o){break;} closed === 1",
    ]);
    assert!(evaluate("'x'.replace('x',function(){throw 42;})").is_err());
}

#[test]
fn accessor_literals_and_delete_follow_string_exotic_rules() {
    check(&[
        "let s={get toString(){return ()=> 'abc';}}; String(s) === 'abc'",
        "let s={toString(){return 'abc';}}; s.toString() === 'abc' && String(s) === 'abc'",
        "let o={get 'quoted'(){return 1},set 3(value){this.value=value}}; o.quoted === 1 && o[3] === undefined",
        "let s={[Symbol.match](value){return value+'!';}}; 'x'.match(s) === 'x!'",
        "let s=new String('abc'); !delete s[0] && !delete s.length && delete s.missing",
        "let s=new String('a'); s.x=1; delete s.x && s.x === undefined",
        "let n=0; let o={set value(v){n=v;},get value(){return n;}}; o.value=3; o.value === 3",
        "let p={}; Object.defineProperty(p,'x',{value:1}); let o={__proto__:p,x:2}; o.x === 2",
    ]);
    for source in ["'use strict'; delete 'a'[0]", "'use strict'; let s=new String('a'); delete s[0]"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn bootstrap_failures_are_transactional_at_every_allocation_stage() {
    use blueice_bluejs::{HeapConfig, HeapError, VmConfig};
    let code = compile(&parse("String").unwrap()).unwrap();
    for ceiling in (1024..80000).step_by(503) {
        let mut vm = Vm::new(VmConfig { heap: HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: ceiling }, ..Default::default() }).unwrap();
        let baseline = vm.heap().stats().managed_bytes;
        for _ in 0..2 {
            match vm.execute(&code) {
                Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => assert_eq!(vm.heap().stats().managed_bytes, baseline, "ceiling {ceiling}"),
                Ok(_) => break,
                other => panic!("unexpected bootstrap result {other:?}"),
            }
        }
    }
}

#[test]
fn edition_17_regexp_flags_and_species_order_are_observable() {
    check(&[
        "let log=''; let n=0; let r={get flags(){log+='f';return 'g';},get global(){throw 1;},exec(){n++;return n===1?{0:'a',index:0,length:1}:null;}}; let a=RegExp.prototype[Symbol.match].call(r,'a'); a[0] === 'a' && log === 'f' && n === 2",
        "let n=0; let r={flags:'g',get global(){throw 1;},exec(){n++;return n===1?{0:'a',index:0,length:1}:null;}}; RegExp.prototype[Symbol.replace].call(r,'a','b') === 'b' && n === 2",
        "let log=''; let r=/a/g; Object.defineProperty(r,'constructor',{get(){log+='c';return undefined;}}); Object.defineProperty(r,'flags',{get(){log+='f';return 'g';}}); RegExp.prototype[Symbol.matchAll].call(r,'a'); log === 'cf'",
        "let log=''; let r=/a/; Object.defineProperty(r,'constructor',{get(){log+='c';return undefined;}}); Object.defineProperty(r,'flags',{get(){log+='f';return '';}}); 'a'.split(r); log === 'cf'",
    ]);
}

#[test]
fn capture_identity_and_primitive_protocol_lookup() {
    check(&[
        "let m=/(?<outer>(?<inner>a))(?<absent>b)?/d.exec('a'); m.indices.groups.outer === m.indices[1] && m.indices.groups.inner === m.indices[2] && m.indices[1] !== m.indices[2] && m.indices.groups.absent === undefined",
        "let m=/(x)(?<\\u0061>a)/d.exec('xa'); m.indices.groups.a === m.indices[2]",
        "let m=/(?<x>a)|(?<x>b)/d.exec('b'); m.indices.groups.x === m.indices[2]",
        "Number.prototype[Symbol.match]=function(s){return s+this;}; 'a'.match(3) === 'a3'",
        "String.prototype[Symbol.search]=()=>42; 'a'.search('b') === 42",
        "Boolean.prototype[Symbol.matchAll]=()=>42; 'a'.matchAll(true) === 42",
    ]);
    let source = "let o={}; Object.defineProperty(o,'x',{get value(){return {x:'alive'};},get writable(){let a={};return true;},get configurable(){let a={};return true;}}); o.x.x === 'alive'";
    let mut vm =
        Vm::new(blueice_bluejs::VmConfig { heap: blueice_bluejs::HeapConfig { nursery_capacity: 1, major_threshold_bytes: 1, max_heap_bytes: 256 * 1024 }, ..Default::default() })
            .unwrap();
    assert_eq!(vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap(), Value::Bool(true));
}

#[test]
fn array_conversion_and_descriptor_creation_are_observable() {
    check(&[
        "let a=['x']; a.join=()=> 'custom'; String(a) === 'custom'",
        "let a=['x']; a.join=3; String(a) === '[object Array]'",
        "let a=['a',null,['b','c']]; a.join('|') === 'a||b,c'",
        "let a=[1]; a[1]=a; a[2]=2; String(a) === '1,,2'",
        "let log=''; let a={get length(){log+='l';return 2;},get 0(){log+='0';return 'a';},get 1(){log+='1';return 'b';}}; let sep={toString(){log+='s';return '!';}}; [].join.call(a,sep) === 'a!b' && log === 'ls01'",
        "let k=Symbol(); let o=Object.create(null,{x:{value:'a'},[k]:{get(){return 'b';}}}); String(o.x)+o[k] === 'ab' && Object.getPrototypeOf(o) === null",
        "Object.prototype.toString.call(new Number(1)) === '[object Number]' && Object.prototype.toString.call(new Boolean(1)) === '[object Boolean]' && Object.prototype.toString.call(/x/) === '[object RegExp]'",
    ]);
}

#[test]
fn conversion_iteration_and_descriptor_errors() {
    for source in [
        "+Symbol()",
        "String({[Symbol.toPrimitive](){return {};}})",
        "new Symbol()",
        "String.prototype.toString.call(null)",
        "String.prototype.match.call(null)",
        "String.prototype.replaceAll.call(null)",
        "'x'.matchAll({[Symbol.match]:true,flags:null})",
        "'x'.replaceAll({[Symbol.match]:true,flags:undefined})",
        "String.prototype.slice.apply.call({},null,[])",
        "String.prototype.slice.apply('',3)",
        "Reflect.construct(String,[],()=>{})",
        "Reflect.construct({},[])",
        "Reflect.construct(String,[],{})",
        "String.toString.call({})",
        "Number.prototype.toString.call({})",
        "Boolean.prototype.valueOf.call(3)",
        "Symbol.prototype.toString.call({})",
        "Symbol.prototype.valueOf.call(3)",
        "Object.create(3)",
        "Object.keys(null)",
        "Reflect.ownKeys('a')",
        "Object.defineProperty(3,'x',{})",
        "Object.defineProperty({},'x',3)",
        "Object.defineProperty({},'x',{get:3})",
        "Object.defineProperty({},'x',{set:3})",
        "Object.defineProperty({},'x',{get(){},value:1})",
        "Object.setPrototypeOf({},3)",
        "Object.defineProperty(new String('a'),'length',{writable:true})",
        "Object.defineProperty(new String('a'),'0',{get(){return 'a';}})",
        "'use strict'; let o={get x(){return 1;}}; o.x=2",
        "'use strict'; let a=[]; Object.defineProperty(a,'length',{writable:false}); a[0]=1",
        "let a=[]; Object.defineProperty(a,'length',{writable:false}); Object.defineProperty(a,'0',{value:1})",
        "'x'[Symbol.iterator]().next.call(3)",
        "'x'[Symbol.iterator]().next.call({})",
        "[][Symbol.iterator]().next.call(3)",
        "[][Symbol.iterator]().next.call({})",
        "'x'.matchAll(/x/g).next.call(3)",
        "'x'.matchAll(/x/g).next.call({})",
        "[...{[Symbol.iterator](){return 3;}}]",
        "[...{[Symbol.iterator](){return {next(){return 3;}};}}]",
        "for(let x of {[Symbol.iterator](){return {next(){return {done:false};},return(){return 3;}};}}){break;}",
        "RegExp.prototype.exec.call(3,'x')",
        "RegExp.prototype.exec.call({},'x')",
        "RegExp.prototype.test.call({},'x')",
        "let r=/x/; r.exec=()=>3; r.test('x')",
        "Object.getOwnPropertyDescriptor(RegExp.prototype,'source').get.call(3)",
        "Object.getOwnPropertyDescriptor(RegExp.prototype,'source').get.call({})",
        "let r=/x/g; r.constructor=3; 'x'.matchAll(r)",
        "let r=/x/g; r.constructor={[Symbol.species]:()=>{}}; 'x'.matchAll(r)",
        "let r=/x/g; Object.defineProperty(r,'lastIndex',{writable:false}); r.exec('x')",
    ] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}: {:?}", evaluate(source));
    }
    let error = evaluate("function recurse(){return recurse();} recurse()").unwrap_err();
    assert!(matches!(error, RuntimeError::RangeError(_)));
    assert!(evaluate("new RegExp('[')").unwrap_err().to_string().starts_with("SyntaxError:"));
    assert!(evaluate("throw {x:1}").unwrap_err().to_string().starts_with("uncaught JavaScript value:"));
}

#[test]
fn generic_regexp_and_function_boundaries() {
    check(&[
        "let r=/x/; RegExp(r) === r && new RegExp(r) !== r && RegExp(r,'i').ignoreCase",
        "new RegExp({[Symbol.match]:true,source:'x',flags:'i'}).test('X')",
        "new RegExp().source === '(?:)' && RegExp.prototype.source === '(?:)' && RegExp.prototype.global === undefined",
        "new RegExp('/\\n\\r\\u2028\\u2029').source === '\\\\/\\\\n\\\\r\\\\u2028\\\\u2029'",
        "let r=/x/dgimsuy; r.flags === 'dgimsuy' && r.hasIndices && r.global && r.ignoreCase && r.multiline && r.dotAll && r.unicode && r.sticky && !r.unicodeSets",
        "new RegExp('[a&&a]','v').unicodeSets && new RegExp('[a&&a]','v').test('a')",
        "RegExp.prototype.toString.call({source:'x',flags:'i'}) === '/x/i'",
        "let r=/x/g; r.lastIndex=100; r.exec('x') === null && r.lastIndex === 0",
        "let r=/x/g; delete r[Symbol.match]; r[Symbol.match]=undefined; 'X'.match(r) === null",
        "let it=RegExp.prototype[Symbol.matchAll].call(/x/,'xx'); it.next().value.index === 0 && it.next().done && it.next().done",
        "'x'.matchAll('x').next().value[0] === 'x'",
        "''.split(/x/).length === 1 && ''.split(/(?:)/).length === 0 && 'abc'.split(/b/,0).length === 0",
        "'abc'.split(/(b)/,1)[0] === 'a' && 'abc'.split(/(b)/,2)[1] === 'b'",
        "'abc'.replace(/b/,\"$$|$&|$`|$'|$0|$<missing>\") === 'a$|b|a|c|$0|$<missing>c'",
        "'abc'.replace(/(?<x>b)/,'$<missing>|$<x') === 'a|$<xc'",
        "'ab'.replace(/(?<x>a)(b)/,function(m,a,b,p,s,g){return g.x+b+p+s;}) === 'ab0ab'",
        "let a=[1]; let it=a[Symbol.iterator](); it.next().value === 1 && it.next().done && it.next().done",
        "for(var c of 'ab'){} c === 'b'",
        "let c=''; for(c of 'ab'){continue;} c === 'b'",
        "function f(){for(let x of 'ab'){return;}} f() === undefined",
        "let closed=0; function f(){for(let x of {[Symbol.iterator](){return {next(){return {value:3};},return(){closed++;return {};}};}}){return x;}} f() === 3 && closed === 1",
        "function f(x='a'){return x;} f() === 'a' && f('b') === 'b'",
        "String.fromCharCode(65,...[66],67) === 'ABC'",
        "let x=1; !delete x && delete missing && delete (x=2) && x === 2",
        "function f(){'use strict';return this;} f.call(3) === 3 && f.apply(null) === null",
        "function f(){} typeof f === 'function' && String(f).includes('function')",
        "function f(){} Object.defineProperty(f,'name',{value:3}); f.toString().includes('function')",
        "Object.defineProperty(String,'name',{get(){throw 1;}}); String.toString().includes('String')",
        "function f(){} Object.defineProperty(f,'name',{get(){throw 1;}}); typeof String(f) === 'string'",
        "Object.getPrototypeOf(String)() === undefined && Object.getPrototypeOf(String).name === ''",
        "Number() === 0 && Number('2') === 2 && Boolean() === false && Boolean(1) === true",
        "new Number(2).valueOf() === 2 && new Boolean(1).valueOf() === true",
        "Symbol.prototype.toString.call(Symbol()) === 'Symbol()'",
        "Object.prototype.toString.call(null) === '[object Null]' && Object.prototype.toString.call(undefined) === '[object Undefined]'",
        "Object.prototype.toString.call({[Symbol.toStringTag]:'Custom'}) === '[object Custom]'",
        "let o={}; Object.defineProperty(o,'x',{set(v){}}); o.x === undefined",
        "let o={get x(){return 1;}}; o.x=2; o.x === 1",
        "let f=()=>1; let o={}; Object.defineProperty(o,'x',{get:f}); Object.defineProperty(o,'x',{get:f}); Object.getOwnPropertyDescriptor(o,'x').get === f",
        "let o={get x(){return 1;}}; Object.defineProperty(o,'x',{value:2}); o.x === 2",
        "Object.getOwnPropertyDescriptor({},'x') === undefined",
        "let k=Symbol(); let s=new String('a'); s[k]=2; let keys=Reflect.ownKeys(s); keys[0] === '0' && keys[1] === 'length' && keys[2] === k && Object.getOwnPropertySymbols(s)[0] === k && Object.getOwnPropertyNames(s).length === 2",
        "let o={}; Object.setPrototypeOf(o,null) === o && Object.getPrototypeOf(o) === null",
        "Object() !== Object(null) && typeof Object(true) === 'object'",
        "let log=''; Object.defineProperty(Number.prototype,'raw',{get(){'use strict';log+=typeof this;return ['a'];}}); String.raw(3) === 'a' && log === 'object'",
    ]);
}

#[test]
fn abrupt_loop_completion_closes_iterators_and_preserves_thrown_objects() {
    let mut vm = Vm::default();
    let code = compile(
        &parse("globalThis.closed=0; for(let x of {[Symbol.iterator](){return {next(){return {value:1};},return(){globalThis.closed++;throw 2;}};}}){throw {message:'original'};}")
            .unwrap(),
    )
    .unwrap();
    let Err(RuntimeError::Thrown(Value::Object(thrown))) = vm.execute(&code) else { panic!("expected original thrown object") };
    assert_eq!(vm.heap().get(thrown, "message").unwrap(), Value::String("original".into()));
    assert_eq!(vm.execute(&compile(&parse("globalThis.closed").unwrap()).unwrap()).unwrap(), Value::Number(1.0));
    assert!(vm.heap().get(thrown, "message").is_err());
}

#[test]
fn templates_preserve_regexp_lexical_context() {
    check(&[
        "`${/}/.test('}')}` === 'true'",
        "String.raw`${/{/.source}` === '{'",
        "`${/['`{}]/.test('}')}` === 'true'",
        "`${8/2/2}` === '2'",
        "`${(()=>{return /}/.source;})()}` === '}'",
        "`${1 // } ignored\n+2}` === '3'",
        "`${`inner${/}/.source}`}` === 'inner}'",
    ]);
}

#[test]
fn primitive_string_setters_receive_the_original_receiver() {
    check(&[
        "let result=''; Object.defineProperty(String.prototype,'x',{set(v){'use strict';result=this+v;}}); 'a'.x=2; result === 'a2'",
        "'use strict'; let result=''; Object.defineProperty(String.prototype,'x',{set(v){'use strict';result=typeof this;}}); 'a'.x=2; result === 'string'",
    ]);
    for source in ["null.x=1", "undefined.x=1", "delete null.x", "delete undefined.x", "'use strict'; 'a'.x=1"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn lazy_global_and_iterator_initialization_recovers_from_heap_limits() {
    use blueice_bluejs::{HeapConfig, HeapError, VmConfig};
    let warm = compile(&parse("String").unwrap()).unwrap();
    let alive = compile(&parse("1+1").unwrap()).unwrap();
    for source in ["Object", "Symbol", "Number", "Boolean", "RegExp", "(3).x", "Object(3)", "'x'[Symbol.iterator]()", "[1][Symbol.iterator]()", "'x'.matchAll('x')"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        for ceiling in (64000..110000).step_by(251) {
            let mut vm = Vm::new(VmConfig { heap: HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: ceiling }, ..Default::default() }).unwrap();
            if vm.execute(&warm).is_err() {
                continue;
            }
            let mut failed_bytes = None;
            for _ in 0..2 {
                match vm.execute(&code) {
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => {
                        let bytes = vm.heap().stats().managed_bytes;
                        if let Some(previous) = failed_bytes {
                            assert_eq!(bytes, previous, "{source}, ceiling {ceiling}");
                        }
                        failed_bytes = Some(bytes);
                    }
                    Ok(_) => break,
                    other => panic!("{source}, ceiling {ceiling}: {other:?}"),
                }
            }
            assert_eq!(vm.execute(&alive).unwrap(), Value::Number(2.0));
        }
    }
}

#[test]
fn syntax_metadata_and_remaining_protocol_boundaries() {
    check(&[
        "let o={delete:3,'f'(){return 1;},2(){return 2;}}; o.delete+o.f()+o[2]() === 6",
        "delete (3).x && Symbol('s').missing === undefined && true.missing === undefined && (1).missing === undefined",
        "Object.prototype.toString.call('x') === '[object String]' && Object.prototype.toString.call(Symbol()) === '[object Symbol]' && Object.prototype.toString.call(1) === '[object Number]' && Object.prototype.toString.call(true) === '[object Boolean]'",
        "Object.prototype.toString.call(new String('x')) === '[object String]' && Object.prototype.toString.call(String) === '[object Function]'",
        "let a=[...'a',...'b']; a[0]+a[1] === 'ab' && [...a,...a].length === 4",
        "'ab'.matchAll(/a/g).next().value[0] === 'a' && 'b'.matchAll(/b/g).next().value[0] === 'b'",
        "let r=/a/g; r.constructor={[Symbol.species]:null}; 'a'.matchAll(r).next().value[0] === 'a'",
        "'😀'.match(/(?:)/gu).length === 2 && '😀'.replace(/(?:)/gu,'-') === '-😀-'",
        "let m=/(a)(b)?/d.exec('a'); m.indices.groups === undefined && m.indices[2] === undefined",
        "let m=/(?<\\u{61}>a)/du.exec('a'); m.indices.groups.a === m.indices[1]",
        "new RegExp('\\ud800','u').test('\\ud800')",
        "'null'.replace(null,'x') === 'x' && 'undefined'.replaceAll(undefined,'x') === 'x'",
        "Object.prototype.toString.call(Object(Symbol())) === '[object Symbol]'",
        "let r=/a/; r.constructor={}; RegExp(r) !== r",
        "let p={x:3}; let o=Object.create(p); Object.setPrototypeOf(o,p); o.x === 3",
        "let props={}; Object.defineProperty(props,'hidden',{value:{value:3}}); Object.create(null,props).hidden === undefined",
        "let n=0; let r={flags:'g',global:true,exec(){n++;return n===1?{0:'ab',length:1,index:0}:n===2?{0:'b',length:1,index:1}:null;}}; RegExp.prototype[Symbol.replace].call(r,'abc','X') === 'Xc'",
    ]);
    for source in ["'use strict'; let x; delete x", "({get x(a){}})", "({set x(){}})", "({'x'})", "/x\n/", "/x\\\n/", "/x\\", "/(/", "String.raw`abc", "String.raw`abc\\"] {
        assert!(parse(source).is_err() || compile(&parse(source).unwrap()).is_err(), "{source}");
    }
    check(&["String.raw`a\r\nb` === 'a\\nb'", "String.raw`a\\\r\nb` === 'a\\\\\\nb'", "function t(s){return s[0];} t`a\\\nb` === 'ab'"]);
    let mut vm = Vm::new(blueice_bluejs::VmConfig {
        heap: blueice_bluejs::HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: 256 * 1024 },
        ..Default::default()
    })
    .unwrap();
    let code = compile(&parse("function make(){return ()=>this.x;} let f=make.call({x:'alive'}); let garbage={}; f()").unwrap()).unwrap();
    assert_eq!(vm.execute(&code).unwrap(), Value::String("alive".into()));
    for source in ["[...null]", "new (()=>{})()"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))));
    }
}

#[test]
fn array_length_descriptors_coerce_twice_and_reject_invalid_lengths() {
    check(&[
        "let n=0; let a=[]; a.length={valueOf(){n++;return 2;}}; a.length === 2 && n === 2",
        "let n=0; let a=[]; Object.defineProperty(a,'length',{value:{valueOf(){n++;return 2;}}}); a.length === 2 && n === 2",
        "let a=[]; Object.defineProperty(a,'length',{value:'2'}); a.length === 2",
    ]);
    for source in ["Object.defineProperty([],'length',{value:1.5})", "let n=0; Object.defineProperty([],'length',{value:{valueOf(){return n++;}}})"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::RangeError(_))), "{source}");
    }
    assert!(matches!(evaluate("let o={}; Object.setPrototypeOf(o,o)"), Err(RuntimeError::TypeError(_))));
}

#[test]
fn complete_string_surface_has_specified_property_attributes() {
    check(&[
        "Object.getOwnPropertyNames(String.prototype).length === 52 && Object.getOwnPropertySymbols(String.prototype).length === 1",
        "let ok=true; for(let k of Reflect.ownKeys(String.prototype)){if(k!=='length' && k!=='constructor'){let d=Object.getOwnPropertyDescriptor(String.prototype,k); ok=ok && typeof d.value === 'function' && d.writable && !d.enumerable && d.configurable;}} ok",
        "String.prototype.trimLeft === String.prototype.trimStart && String.prototype.trimRight === String.prototype.trimEnd",
    ]);
}

#[test]
fn nested_functions_share_the_total_compiled_byte_budget() {
    use blueice_bluejs::{CompileError, compile_with_limit};
    let body = "1;".repeat(100);
    let single = parse(&format!("function a(){{function b(){{{body}}}}}")).unwrap();
    let required = (1..2000).find(|&limit| compile_with_limit(&single, limit).is_ok()).unwrap();
    let doubled = parse(&format!("function a(){{function b(){{{body}}}}} function c(){{function d(){{{body}}}}}")).unwrap();
    assert!(matches!(compile_with_limit(&doubled, required + required / 2), Err(CompileError::ProgramTooLarge)));
    assert!(compile_with_limit(&doubled, required * 2).is_ok());
}
