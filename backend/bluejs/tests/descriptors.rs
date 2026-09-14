// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{
    compile, parse, Heap, HeapConfig, HeapError, JsString, JsSymbol, PropertyDescriptor as D,
    PropertyName, RuntimeError, Value, Vm,
};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn evaluate_test262_script(source: &str) -> Result<Value, RuntimeError> {
    let mut vm = Vm::default();
    vm.install_test262_harness()?;
    vm.execute_script(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn immutable_descriptors_and_nonextensible_objects() {
    let mut heap = Heap::default();
    let object = heap.alloc_object(None).unwrap();
    assert!(heap.is_extensible(object).unwrap());
    assert!(!heap
        .define_own_property(
            object,
            "mixed",
            D {
                get: Some(Value::Undefined),
                value: Some(Value::Null),
                ..D::default()
            }
        )
        .unwrap());
    heap.define_own_property(
        object,
        "fixed",
        D::data(Value::Number(f64::NAN), false, true, false),
    )
    .unwrap();
    assert!(heap
        .define_own_property(
            object,
            "fixed",
            D {
                value: Some(Value::Number(f64::NAN)),
                ..D::default()
            }
        )
        .unwrap());
    for descriptor in [
        D {
            enumerable: Some(false),
            ..D::default()
        },
        D {
            configurable: Some(true),
            ..D::default()
        },
        D {
            get: Some(Value::Undefined),
            ..D::default()
        },
    ] {
        assert!(!heap
            .define_own_property(object, "fixed", descriptor)
            .unwrap());
    }
    heap.define_own_property(
        object,
        "accessor",
        D {
            get: Some(Value::Undefined),
            ..D::default()
        },
    )
    .unwrap();
    assert!(!heap
        .define_own_property(
            object,
            "accessor",
            D {
                get: Some(Value::Null),
                ..D::default()
            }
        )
        .unwrap());
    assert!(!heap
        .define_own_property(
            object,
            "accessor",
            D {
                set: Some(Value::Null),
                ..D::default()
            }
        )
        .unwrap());
    assert_eq!(
        heap.enumerable_own_keys(object).unwrap(),
        vec![JsString::from("fixed")]
    );
    assert!(!heap.delete(object, "fixed").unwrap());
    assert_eq!(
        heap.set(object, "fixed", Value::Null),
        Err(HeapError::ReadOnlyProperty)
    );
    heap.prevent_extensions(object).unwrap();
    assert!(!heap.is_extensible(object).unwrap());
    assert!(!heap
        .define_own_property(object, "new", D::default())
        .unwrap());
    assert_eq!(
        heap.set(object, "new", Value::Null),
        Err(HeapError::ReadOnlyProperty)
    );
    assert!(heap.set_prototype(object, None).is_ok());
    let other = heap.alloc_object(None).unwrap();
    assert_eq!(
        heap.set_prototype(object, Some(other)),
        Err(HeapError::ReadOnlyProperty)
    );
}

#[test]
fn descriptors_trace_accessors_and_preserve_symbol_order() {
    let mut heap = Heap::new(HeapConfig {
        nursery_capacity: 1,
        major_threshold_bytes: 256,
        max_heap_bytes: 32768,
    })
    .unwrap();
    let object = heap.alloc_object(None).unwrap();
    heap.root(object).unwrap();
    let getter = heap.alloc_object(None).unwrap();
    let symbol = JsSymbol::new(Some("key".into()));
    heap.define_own_property(
        object,
        symbol.clone(),
        D {
            get: Some(Value::Object(getter)),
            configurable: Some(true),
            ..D::default()
        },
    )
    .unwrap();
    heap.set(object, "x", Value::Null).unwrap();
    heap.collect_major();
    assert!(heap
        .get_own_property_descriptor(getter, "missing")
        .unwrap()
        .is_none());
    assert_eq!(heap.own_keys(object).unwrap(), vec![JsString::from("x")]);
    assert_eq!(
        heap.own_property_keys(object).unwrap(),
        vec![PropertyName::from("x"), symbol.clone().into()]
    );
    assert!(heap.delete(object, symbol).unwrap());
    heap.collect_major();
    assert!(matches!(
        heap.get(getter, "x"),
        Err(HeapError::InvalidObject(_))
    ));
    let utf8 = "ab".to_owned();
    let left = JsString::from(&utf8);
    let copied = JsString::from(&left);
    assert_eq!(left, copied);
    assert!(left == "ab");
}

#[test]
fn array_truncation_stops_at_nonconfigurable_elements() {
    let mut heap = Heap::default();
    let array = heap.alloc_array(0, None).unwrap();
    let invalid_length = heap
        .define_own_property(
            array,
            "length",
            D {
                value: Some(Value::Number(-1.0)),
                ..D::default()
            },
        )
        .unwrap_err();
    assert_eq!(invalid_length, HeapError::InvalidArrayLength);
    assert!(matches!(
        blueice_bluejs::RuntimeError::from(invalid_length),
        blueice_bluejs::RuntimeError::RangeError(_)
    ));
    heap.define_own_property(array, "2", D::data(Value::Null, true, true, false))
        .unwrap();
    heap.set(array, "4", Value::Bool(true)).unwrap();
    assert_eq!(
        heap.set(array, "length", Value::Number(1.0)),
        Err(HeapError::ReadOnlyProperty)
    );
    assert_eq!(heap.get(array, "length").unwrap(), Value::Number(3.0));
    assert_eq!(heap.get(array, "4").unwrap(), Value::Undefined);
    assert_eq!(heap.get(array, "2").unwrap(), Value::Null);
    assert!(!heap
        .define_own_property(
            array,
            "length",
            D {
                value: Some(Value::Number(0.0)),
                writable: Some(false),
                ..D::default()
            }
        )
        .unwrap());
    assert_eq!(
        heap.get_own_property_descriptor(array, "length")
            .unwrap()
            .unwrap()
            .writable,
        Some(false)
    );
    assert_eq!(
        heap.set(array, "3", Value::Null),
        Err(HeapError::ReadOnlyProperty)
    );
}

#[test]
fn define_properties_coerces_array_length_after_collecting_descriptors() {
    assert_eq!(
        evaluate(
            "let log=[];let length={valueOf:function(){log.push('valueOf');return 2}};let array=[0,1,2];Object.defineProperties(array,{first:{value:1},length:{value:length}});array.length===2&&array[0]===0&&array[1]===1&&array[2]===undefined&&log.join(',')==='valueOf'",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let array=[0,1];Object.defineProperties(array,{length:{value:null}});array.length"
        )
        .unwrap(),
        Value::Number(0.0)
    );
    assert!(matches!(
        evaluate(
            "let length={valueOf:function(){return {}},toString:function(){return {}}};Object.defineProperties([],{length:{value:length}})"
        ),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn legacy_accessor_helpers_share_descriptor_and_proxy_boundaries() {
    assert_eq!(
        evaluate(
            "let getter=function(){return this.value};let setter=function(value){this.seen=value};let subject={value:1};subject.__defineGetter__('access',getter);subject.__defineSetter__('access',setter);subject.access=5;let inherited=Object.create(subject);let data=Object.create(subject);Object.defineProperty(data,'access',{value:0});let calls=[];let proxy=new Proxy(Object.create(subject),{getOwnPropertyDescriptor:function(target,key){calls.push('own');return Object.getOwnPropertyDescriptor(target,key)},getPrototypeOf:function(target){calls.push('prototype');return Object.getPrototypeOf(target)}});let marker={};let abrupt=false;try{new Proxy({}, {defineProperty:function(){throw marker}}).__defineGetter__('blocked',getter)}catch(error){abrupt=error===marker}let conversions=0;let key={toString:function(){conversions++;return 'access'}};let nonCallable=false;try{subject.__defineGetter__(key,0)}catch(error){nonCallable=error instanceof TypeError}subject.access===1&&subject.seen===5&&subject.__lookupGetter__('access')===getter&&inherited.__lookupSetter__('access')===setter&&data.__lookupGetter__('access')===undefined&&proxy.__lookupGetter__('access')===getter&&calls.join(',')==='own,prototype'&&abrupt&&nonCallable&&conversions===0&&Object.prototype.__defineGetter__.length===2&&Object.prototype.__lookupSetter__.length===1",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn legacy_proto_accessor_uses_object_internal_methods() {
    assert_eq!(
        evaluate(
            "let descriptor=Object.getOwnPropertyDescriptor(Object.prototype,'__proto__');let getter=descriptor.get;let setter=descriptor.set;let prototype={};let subject={};setter.call(subject,prototype);let ignored=setter.call(subject,1)===undefined&&setter.call(1,prototype)===undefined&&getter.call(subject)===prototype;let marker={};let getAbrupt=false;try{getter.call(new Proxy({}, {getPrototypeOf:function(){throw marker}}))}catch(error){getAbrupt=error===marker}let setAbrupt=false;try{setter.call(new Proxy({}, {setPrototypeOf:function(){throw marker}}),prototype)}catch(error){setAbrupt=error===marker}let cycleRoot={};let cycleLeaf=Object.create(cycleRoot);let cycle=false;try{setter.call(cycleRoot,cycleLeaf)}catch(error){cycle=error instanceof TypeError}let nullReceiver=false;try{setter.call(null,prototype)}catch(error){nullReceiver=error instanceof TypeError}descriptor.enumerable===false&&descriptor.configurable===true&&getter.name==='get __proto__'&&setter.name==='set __proto__'&&getter.length===0&&setter.length===1&&ignored&&getAbrupt&&setAbrupt&&cycle&&getter.call(cycleRoot)===Object.prototype&&nullReceiver",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_to_locale_string_uses_the_receiver_to_string_method() {
    assert_eq!(
        evaluate(
            "'use strict';let received;let subject={toString:function(){received=this;return 'subject'}};let proxy=new Proxy({toString:function(){return 'proxy'}},{get:function(target,key,receiver){if(key!=='toString')throw new Error('unexpected key');return target[key]}});let nonCallable=false;try{Object.prototype.toLocaleString.call({toString:0})}catch(error){nonCallable=error instanceof TypeError}let nullReceiver=false;try{Object.prototype.toLocaleString.call(null)}catch(error){nullReceiver=error instanceof TypeError}Boolean.prototype.toString=function(){return typeof this};Object.prototype.toLocaleString.call(subject)==='subject'&&received===subject&&Object.prototype.toLocaleString.call(proxy)==='proxy'&&true.toLocaleString()==='boolean'&&nonCallable&&nullReceiver&&Object.prototype.toLocaleString.length===0",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_to_string_uses_internal_slots_before_observing_to_string_tag() {
    assert_eq!(
        evaluate(
            "let arrayProxy=new Proxy([],{});let error=Error('message');let override=[];override[Symbol.toStringTag]='overridden';let generator=function*(){};let generatorProxy=new Proxy(generator,{});let generatorTag=Object.prototype.toString.call(generatorProxy);delete generatorProxy.constructor.prototype[Symbol.toStringTag];let generatorFallback=Object.prototype.toString.call(generatorProxy);let bigint=Object.prototype.toString.call(3n);Object.prototype.toString.call(arrayProxy)==='[object Array]'&&Object.prototype.toString.call(error)==='[object Error]'&&Object.prototype.toString.call(override)==='[object overridden]'&&generatorTag==='[object GeneratorFunction]'&&generatorFallback==='[object Function]'&&bigint==='[object BigInt]'",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate_test262_script(
            "'use strict';var custom;let results=[];function probe(value){try{custom=value;custom[Symbol.toStringTag]='overridden';results.push(Object.prototype.toString.call(custom))}catch(error){results.push(error.name)}}probe([]);probe(new String());probe((function(){return arguments})());probe(function(){});probe(new Error());probe(new Boolean());probe(new Number());probe(new Date());probe(/./);results.join(',')",
        )
        .unwrap(),
        Value::String("[object overridden],[object overridden],[object overridden],[object overridden],[object overridden],[object overridden],[object overridden],[object overridden],[object overridden]".into())
    );
}

#[test]
fn object_property_queries_coerce_keys_before_the_receiver() {
    assert_eq!(
        evaluate(
            "let marker={};let key={toString:function(){throw marker}};let own=false;try{Object.prototype.hasOwnProperty.call(null,key)}catch(error){own=error===marker}let enumerable=false;try{Object.prototype.propertyIsEnumerable.call(undefined,key)}catch(error){enumerable=error===marker}own&&enumerable",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_static_has_own_and_is_use_internal_property_and_same_value_contracts() {
    assert_eq!(
        evaluate(
            "let log=[];let key={toString:function(){log.push('key');return 'present'}};let proxy=new Proxy({present:1},{getOwnPropertyDescriptor:function(target,name){log.push('descriptor:'+name);return Object.getOwnPropertyDescriptor(target,name)}});let nullish=false;try{Object.hasOwn(null,key)}catch(error){nullish=error instanceof TypeError}Object.hasOwn(proxy,key)&&!Object.hasOwn(proxy,'missing')&&nullish&&log.join(',')==='key,descriptor:present,descriptor:missing'",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "Object.is(NaN,NaN)&&!Object.is(0,-0)&&Object.is(-0,-0)&&Object.is.length===2&&Object.hasOwn.length===2",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_returns_function_values_without_leaking_expression_names() {
    assert_eq!(
        evaluate(
            "let wrapped=Object(function hidden(){return 1});typeof hidden==='undefined'&&wrapped.constructor===Function&&wrapped()===1",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_constructor_uses_a_subclass_new_target_and_ignores_its_argument() {
    assert_eq!(
        evaluate(
            "class Subclass extends Object{}let direct=new Subclass({direct:true});let reflected=Reflect.construct(Object,[{reflected:true}],Subclass);direct.direct===undefined&&reflected.reflected===undefined&&Object.getPrototypeOf(direct)===Subclass.prototype&&Object.getPrototypeOf(reflected)===Subclass.prototype",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn reflective_own_keys_materialize_the_p0_global_realm_surface() {
    assert_eq!(
        evaluate(
            "let names=Object.getOwnPropertyNames(this);let expected=['NaN','Infinity','undefined','eval','parseInt','parseFloat','isNaN','isFinite','decodeURI','decodeURIComponent','encodeURI','encodeURIComponent','Object','Function','Array','String','Boolean','Number','Date','RegExp','Error','EvalError','RangeError','ReferenceError','SyntaxError','TypeError','URIError','Math','JSON'];let constructor=Object.getOwnPropertyDescriptor(Function.prototype,'constructor');expected.every(function(name){return names.includes(name)})&&constructor.value===Function&&constructor.writable===true&&constructor.enumerable===false&&constructor.configurable===true",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_group_by_uses_iterator_keys_and_closes_on_an_abrupt_callback() {
    assert_eq!(
        evaluate(
            "let calls=[];let iterable={i:0,next:function(){return this.i<3?{value:++this.i,done:false}:{done:true}},return:function(){calls.push('closed');return {done:true}},[Symbol.iterator]:function(){return this}};let groups=Object.groupBy(iterable,function(value,index){calls.push(index+':'+value);return value%2?'odd':'even'});let closed=false;try{Object.groupBy({[Symbol.iterator]:function(){return {next:function(){return {value:1,done:false}},return:function(){closed=true;return {done:true}}}}},function(){throw 1})}catch(error){}Object.getPrototypeOf(groups)===null&&groups.odd.join(',')==='1,3'&&groups.even.join(',')==='2'&&calls.join(',')==='0:1,1:2,2:3'&&closed",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn typed_array_integrity_levels_respect_resizable_and_length_tracking_views() {
    assert_eq!(
        evaluate(
            "function typeError(action){try{action();return false}catch(error){return error instanceof TypeError}}let rab=new ArrayBuffer(4,{maxByteLength:8});let rabFixed=new Uint8Array(rab,0,0);let rabTracking=new Uint8Array(rab);let rabPrevent=typeError(()=>Object.preventExtensions(rabFixed))&&typeError(()=>Object.preventExtensions(rabTracking));let rabIntegrity=typeError(()=>Object.seal(new Uint8Array(rab,0,0)))&&typeError(()=>Object.freeze(new Uint8Array(rab,0,0)));let gsab=new SharedArrayBuffer(4,{maxByteLength:8});let fixed=new Uint8Array(gsab,0,4);let fixedPrevent=Object.preventExtensions(fixed)===fixed&&!Object.isExtensible(fixed);let fixedSeal=typeError(()=>Object.seal(new Uint8Array(gsab,0,4)));let tracking=new Uint8Array(gsab);let trackingRejects=typeError(()=>Object.preventExtensions(tracking))&&typeError(()=>Object.seal(tracking));let emptyShared=new SharedArrayBuffer(0,{maxByteLength:8});let fixedEmpty=new Uint8Array(emptyShared,0,0);let trackingEmpty=new Uint8Array(emptyShared);rabPrevent&&rabIntegrity&&fixedPrevent&&fixedSeal&&trackingRejects&&Object.seal(fixedEmpty)===fixedEmpty&&Object.isSealed(fixedEmpty)&&typeError(()=>Object.seal(trackingEmpty))",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_brands_cover_promise_collections_iterators_and_aggregate_errors() {
    assert_eq!(
        evaluate(
            "let tag=Object.prototype.toString;let promise=Promise.resolve(1);let map=new Map();let set=new Set();let mapIterator=map[Symbol.iterator]();let setIterator=set[Symbol.iterator]();let tagged=tag.call(promise)==='[object Promise]'&&tag.call(map)==='[object Map]'&&tag.call(set)==='[object Set]'&&tag.call(mapIterator)==='[object Map Iterator]'&&tag.call(setIterator)==='[object Set Iterator]'&&mapIterator.next().done&&setIterator.next().done;delete Promise.prototype[Symbol.toStringTag];delete Map.prototype[Symbol.toStringTag];delete Set.prototype[Symbol.toStringTag];let untagged=tag.call(promise)==='[object Object]'&&tag.call(map)==='[object Object]'&&tag.call(set)==='[object Object]';let aggregate=new AggregateError([], 'aggregate');let aggregateOK=aggregate instanceof Error&&aggregate.name==='AggregateError'&&Object.seal(aggregate)===aggregate;let arrowConstructor=Object.getPrototypeOf(async()=>{}).constructor;let functionConstructor=Object.getPrototypeOf(async function(){}).constructor;let generatorConstructor=Object.getPrototypeOf(async function*(){}).constructor;function seals(Constructor){let value=new Constructor();return Object.seal(value)===value}tagged&&untagged&&aggregateOK&&seals(arrowConstructor)&&seals(functionConstructor)&&seals(generatorConstructor)",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_get_own_property_descriptors_uses_internal_key_and_descriptor_methods() {
    assert_eq!(
        evaluate(
            "let symbol=Symbol('key');let log=[];let target={visible:1,[symbol]:2};Object.defineProperty(target,'hidden',{value:3,enumerable:false});let proxy=new Proxy(target,{ownKeys:function(object){log.push('keys');return Reflect.ownKeys(object)},getOwnPropertyDescriptor:function(object,key){log.push('descriptor:'+String(key));return Reflect.getOwnPropertyDescriptor(object,key)}});let descriptors=Object.getOwnPropertyDescriptors(proxy);let stringDescriptors=Object.getOwnPropertyDescriptors('ab');Object.getPrototypeOf(descriptors)===Object.prototype&&descriptors.visible.value===1&&descriptors.hidden.enumerable===false&&descriptors[symbol].value===2&&stringDescriptors.length.value===2&&stringDescriptors[0].value==='a'&&log.join(',')==='keys,descriptor:visible,descriptor:hidden,descriptor:Symbol(key)'&&Object.getOwnPropertyDescriptors.length===1",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "function fakeObject(){};fakeObject.getOwnPropertyDescriptors=Object.getOwnPropertyDescriptors;fakeObject.keys=Object.keys;this.Object=fakeObject;Object.keys(Object.getOwnPropertyDescriptors('a')).length",
        )
        .unwrap(),
        Value::Number(2.0)
    );
    assert_eq!(
        evaluate(
            "function fakeObject(){throw new Error('not called')};fakeObject.getOwnPropertyDescriptors=Object.getOwnPropertyDescriptors;fakeObject.keys=Object.keys;var global=this;global.Object=fakeObject;Object===fakeObject",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "function fakeObject(){throw new Error('not called')};fakeObject.getOwnPropertyDescriptors=Object.getOwnPropertyDescriptors;fakeObject.keys=Object.keys;var global=this;global.Object=fakeObject;Object.keys(Object.getOwnPropertyDescriptors('a')).length",
        )
        .unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn object_assign_uses_enumerable_own_keys_and_proxy_internal_methods() {
    assert_eq!(
        evaluate(
            "let symbol=Symbol('symbol');let log=[];let source=new Proxy({visible:1,hidden:2,[symbol]:3},{ownKeys:function(target){log.push('keys');return Reflect.ownKeys(target)},getOwnPropertyDescriptor:function(target,key){log.push('descriptor:'+String(key));return Object.getOwnPropertyDescriptor(target,key)},get:function(target,key,receiver){log.push('get:'+String(key));return Reflect.get(target,key,receiver)}});Object.defineProperty(source,'hidden',{enumerable:false});let target={};let returned=Object.assign(target,null,undefined,source);returned===target&&target.visible===1&&target.hidden===undefined&&target[symbol]===3&&log.join(',')==='keys,descriptor:visible,get:visible,descriptor:hidden,descriptor:Symbol(symbol),get:Symbol(symbol)'",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert!(matches!(
        evaluate("Object.assign(Object.freeze({}),{value:1})"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn object_entries_and_values_skip_keys_deleted_by_a_previous_getter() {
    assert_eq!(
        evaluate(
            "let source={a:'A',get b(){delete this.c;return 'B'},c:'C'};let entries=Object.entries(source);let values=Object.values(source);entries.length===2&&entries[0][0]==='a'&&entries[0][1]==='A'&&entries[1][0]==='b'&&entries[1][1]==='B'&&values.length===2&&values[0]==='A'&&values[1]==='B'",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_keys_observes_proxy_own_keys_and_descriptor_traps_in_order() {
    assert_eq!(
        evaluate(
            "let log=[];let symbol=Symbol('key');let target={x:true};let keys={get length(){log.push('length');return 3},get 0(){log.push('0');return 'a'},get 1(){log.push('1');return symbol},get 2(){log.push('2');return 'b'}};let descriptors={a:{enumerable:true,configurable:true,value:1},b:{enumerable:false,configurable:true,value:2},[symbol]:{enumerable:true,configurable:true,value:3}};let handler={get ownKeys(){log.push('trap:keys');return function(){log.push('call:keys');return keys}},get getOwnPropertyDescriptor(){log.push('trap:descriptor');return function(target,key){log.push('call:descriptor:'+String(key));return descriptors[key]}}};let result=Object.keys(new Proxy(target,handler));log.join(',')+';'+result.join(',')",
        )
        .unwrap(),
        Value::String("trap:keys,call:keys,length,0,1,2,trap:descriptor,call:descriptor:a,trap:descriptor,call:descriptor:b;a".into())
    );
}

#[test]
fn object_from_entries_defines_entries_and_closes_on_an_abrupt_entry() {
    assert_eq!(
        evaluate(
            "let symbol=Symbol('entry');let result=Object.fromEntries([['first',1],[symbol,2]]);result.first===1&&result[symbol]===2&&Object.getPrototypeOf(result)===Object.prototype",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let closed=false;let iterable={};iterable[Symbol.iterator]=function(){return {next:function(){return {value:null,done:false}},return:function(){closed=true;return {done:true}}}};let abrupt=false;try{Object.fromEntries(iterable)}catch(error){abrupt=error instanceof TypeError}abrupt&&closed",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn object_prototype_rejects_distinct_prototypes() {
    assert_eq!(
        evaluate(
            "let root=Object.prototype;let replacement=Object.create(null);let threw=false;try{Object.setPrototypeOf(root,replacement)}catch(error){threw=error instanceof TypeError}Reflect.setPrototypeOf(root,null)&&!Reflect.setPrototypeOf(root,replacement)&&Object.getPrototypeOf(root)===null&&threw",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn date_utc_normalizes_components_and_exposes_the_date_tag() {
    assert_eq!(
        evaluate("Date.UTC(1970,0,1,80063993375,29,1,-288230376151711740)").unwrap(),
        Value::Number(29312.0)
    );
    assert_eq!(
        evaluate("Date.UTC(1970,0,213503982336,0,0,0,-18446744073709552000)").unwrap(),
        Value::Number(34447360.0)
    );
    assert_eq!(
        evaluate(
            "Date.UTC(1970,0,1)===0&&Date.UTC(99,0,1)===Date.UTC(1999,0,1)&&Date.UTC(2000,1,29)===951782400000&&Date.UTC()!==Date.UTC()&&Date.UTC.length===7&&Object.prototype.toString.call(new Date())==='[object Date]'",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn date_instances_store_timeclip_values_and_expose_iso_json_and_parse_contracts() {
    assert_eq!(
        evaluate(
            "let date=new Date(Date.UTC(2000,1,29,12,34,56,789));let invalid=new Date(NaN);let globalDate=Object.getOwnPropertyDescriptor(globalThis,'Date');date.getTime()===951827696789&&date.valueOf()===951827696789&&date.getUTCFullYear()===2000&&date.getUTCMonth()===1&&date.getUTCDate()===29&&date.getUTCDay()===2&&date.getUTCHours()===12&&date.getUTCMinutes()===34&&date.getUTCSeconds()===56&&date.getUTCMilliseconds()===789&&date.getTimezoneOffset()===0&&date.setTime(-0)===0&&1/date.getTime()===Infinity&&date.setTime(0)===0&&date.toISOString()==='1970-01-01T00:00:00.000Z'&&date.toUTCString()==='Thu, 01 Jan 1970 00:00:00 GMT'&&date.toDateString()==='Thu Jan 01 1970'&&date.toTimeString()==='00:00:00 GMT+0000 (Coordinated Universal Time)'&&date.toString()==='Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)'&&date.toJSON()==='1970-01-01T00:00:00.000Z'&&invalid.toJSON()===null&&Date.parse('1970-01-01T01:00:00+01:00')===0&&new Date('1970-01-01').getTime()===0&&Date.parse.length===1&&date instanceof Date&&date[Symbol.toPrimitive]('number')===0&&globalDate.value===Date&&globalDate.writable&&globalDate.configurable&&!globalDate.enumerable",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn date_setters_normalize_utc_components_and_invalid_times() {
    assert_eq!(
        evaluate(
            "let date=new Date(Date.UTC(2000,0,31,23,59,59,900));date.setUTCMonth(1)===952041599900&&date.getUTCMonth()===2&&date.getUTCDate()===2&&date.setUTCSeconds(61,5)===952041601005&&date.getUTCMinutes()===0&&date.getUTCSeconds()===1&&date.getUTCMilliseconds()===5&&date.setUTCFullYear(1999,11,31)===946598401005&&date.getUTCFullYear()===1999&&date.getUTCMonth()===11&&date.getUTCDate()===31&&date.setYear(99)===946598401005&&date.getFullYear()===1999&&((new Date(NaN).setUTCDate(1))!==(new Date(NaN).setUTCDate(1)))",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn weak_collections_accept_object_keys_and_reject_primitive_insertions() {
    assert_eq!(
        evaluate("typeof globalThis.WeakMap").unwrap(),
        Value::String("function".into())
    );
    assert_eq!(
        evaluate("let prepared=0;typeof WeakMap").unwrap(),
        Value::String("function".into())
    );
    assert_eq!(
        evaluate(
            "let key={};let value={};let map=new WeakMap([[key,value]]);let set=new WeakSet([key]);let mapInsert=false;let setInsert=false;try{map.set(1,value)}catch(error){mapInsert=error instanceof TypeError}try{set.add(1)}catch(error){setInsert=error instanceof TypeError}map.get(key)===value&&map.has(key)&&map.delete(key)&&!map.has(key)&&set.has(key)&&set.delete(key)&&!set.has(key)&&mapInsert&&setInsert&&Object.prototype.toString.call(map)==='[object WeakMap]'&&Object.prototype.toString.call(set)==='[object WeakSet]'",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn map_entries_preserve_strong_key_identity_and_same_value_zero() {
    assert_eq!(
        evaluate(
            "let key={};let map=new Map();let iterable=new Map([['first',1],['second',2],['first',3]]);map.set(key,'object').set(-0,'zero').set(NaN,undefined);map.get(key)==='object'&&map.has(key)&&map.get(0)==='zero'&&map.has(NaN)&&map.get(NaN)===undefined&&map.size===3&&map.delete(key)&&!map.has(key)&&map.size===2&&iterable.size===2&&iterable.get('first')===3&&iterable.get('second')===2",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert!(matches!(
        evaluate("Map.prototype.get.call({}, 0)"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn set_has_an_internal_collection_and_size_getter() {
    assert_eq!(
        evaluate(
            "let set=new Set();let empty=set.size===0;let first=set.add(-0);set.add(NaN).add(NaN);let iterable=new Set(['one','two','one']);empty&&first===set&&set.size===2&&set.has(0)&&set.has(NaN)&&set.delete(0)&&set.size===1&&!set.has(-0)&&iterable.size===2&&iterable.has('one')&&iterable.has('two')",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert!(matches!(
        evaluate("Object.getOwnPropertyDescriptor(Set.prototype, 'size').get.call({})"),
        Err(RuntimeError::TypeError(_))
    ));
}

#[test]
fn weak_map_upserts_preserve_symbol_identity_and_callback_order() {
    assert_eq!(
        evaluate(
            "let symbol=Symbol('key');let registered=Symbol.for('registered');let map=new WeakMap();let first=map.getOrInsert(symbol,1);let second=map.getOrInsert(symbol,2);let computedKey={};let calls=0;let computed=map.getOrInsertComputed(computedKey,function(key){calls+=key===computedKey?1:100;map.set(key,'intermediate');return 'final'});let existing=map.getOrInsertComputed(computedKey,function(){calls+=100;return 'wrong'});let invalidCallback=false;try{map.getOrInsertComputed(symbol,0)}catch(error){invalidCallback=error instanceof TypeError}let registeredRejected=false;try{map.set(registered,1)}catch(error){registeredRejected=error instanceof TypeError}let set=new WeakSet([symbol]);first===1&&second===1&&computed==='final'&&existing==='final'&&calls===1&&invalidCallback&&registeredRejected&&map.get(registered)===undefined&&!map.has(registered)&&set.has(symbol)&&Symbol.keyFor(registered)==='registered'",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn weak_ref_exposes_weak_targets_and_rejects_non_weakly_held_values() {
    assert_eq!(
        evaluate(
            "let object={};let symbol=Symbol('target');let reference=new WeakRef(object);let symbolReference=new WeakRef(symbol);let rejected=false;let registeredRejected=false;try{new WeakRef(1)}catch(error){rejected=error instanceof TypeError}try{new WeakRef(Symbol.for('registered'))}catch(error){registeredRejected=error instanceof TypeError}typeof WeakRef==='function'&&WeakRef.length===1&&reference.deref()===object&&symbolReference.deref()===symbol&&rejected&&registeredRejected&&WeakRef.prototype.deref.length===0&&Object.prototype.toString.call(reference)==='[object WeakRef]'",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn finalization_registry_has_real_slots_and_weak_registration_validation() {
    assert_eq!(
        evaluate(
            "let target={};let token={};let registry=new FinalizationRegistry(function(){});let targetRejected=false;let selfRejected=false;try{registry.register(1,'held')}catch(error){targetRejected=error instanceof TypeError}try{registry.register(target,target)}catch(error){selfRejected=error instanceof TypeError}registry.register(target,'held',token)===undefined&&registry.unregister(token)===true&&registry.unregister(token)===false&&targetRejected&&selfRejected&&FinalizationRegistry.length===1&&FinalizationRegistry.prototype.register.length===2&&FinalizationRegistry.prototype.unregister.length===1&&Object.prototype.toString.call(registry)==='[object FinalizationRegistry]'",
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn finalization_registry_delivers_collected_holdings_in_a_later_job() {
    let mut vm = Vm::default();
    vm.execute_script(
        &compile(
            &parse(
                "
                    globalThis.cleaned=[];
                    globalThis.registry=new FinalizationRegistry(holding=>cleaned.push(holding));
                    (function(){let target={};registry.register(target,'holding');})();
                ",
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    vm.run_promise_jobs().unwrap();
    assert_eq!(
        vm.execute_script(
            &compile(&parse("cleaned.length===1&&cleaned[0]==='holding'").unwrap()).unwrap()
        ),
        Ok(Value::Bool(true))
    );
}

#[test]
fn weak_collection_and_finalization_entries_participate_in_managed_byte_accounting() {
    let mut vm = Vm::default();
    vm.execute_script(
        &compile(
            &parse(
                "
                    globalThis.key={};globalThis.value={};globalThis.token={};
                    globalThis.map=new WeakMap;
                    globalThis.registry=new FinalizationRegistry(function(){});
                ",
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let empty = vm.heap().stats().managed_bytes;
    vm.execute_script(
        &compile(&parse("map.set(key,value);registry.register(key,value,token);").unwrap())
            .unwrap(),
    )
    .unwrap();
    let charged = vm.heap().stats().managed_bytes;
    assert!(charged > empty);
    vm.execute_script(
        &compile(&parse("map.delete(key);registry.unregister(token);").unwrap()).unwrap(),
    )
    .unwrap();
    assert!(vm.heap().stats().managed_bytes < charged);
}

#[test]
fn reflect_and_proxy_operations_keep_the_explicit_receiver_and_trap_contract() {
    assert_eq!(
        evaluate(
            "let received;function target(a,b){received=this;return a+b};let receiver={marker:1};Reflect.apply(target,receiver,{0:2,1:3,length:2})===5&&received===receiver&&Reflect.apply.length===3",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert!(matches!(
        evaluate("Reflect.apply({},null,[])").unwrap_err(),
        RuntimeError::TypeError(_)
    ));
    assert!(matches!(
        evaluate("Reflect.apply(function(){},null,null)").unwrap_err(),
        RuntimeError::TypeError(_)
    ));
    assert_eq!(
        evaluate(
            "let errors=0;for(let value of [1,null,undefined,'']){try{Reflect.getPrototypeOf(value)}catch(error){errors++};try{Reflect.isExtensible(value)}catch(error){errors++};try{Reflect.setPrototypeOf(value,{})}catch(error){errors++}}let target={p:42};let receiver='receiver is a string';errors===12&&Reflect.set(target,'p',43,receiver)===false&&target.p===42&&!receiver.hasOwnProperty('p')&&Reflect[Symbol.toStringTag]==='Reflect'",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let throws=false;try{Reflect.construct(function(){},[],1)}catch(error){throws=error instanceof TypeError};throws",
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let target={get value(){return this.marker}};let receiver={marker:7};Reflect.get(target,'value',receiver)"
        )
        .unwrap(),
        Value::Number(7.0)
    );
    assert_eq!(
        evaluate(
            "let target={};let receiver={};Reflect.set(target,'value',9,receiver)&&target.value===undefined&&receiver.value===9"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let log=[];let target={};let proxy=new Proxy(target,{get(t,k,r){log.push('get:'+k);return k==='value'?r.marker:undefined},set(t,k,v,r){log.push('set:'+k);r[k]=v;return true},deleteProperty(t,k){log.push('delete:'+k);return true},ownKeys(){log.push('keys');return ['listed']}});let receiver={marker:3};let got=Reflect.get(proxy,'value',receiver);Reflect.set(proxy,'stored',4,receiver);let deleted=Reflect.deleteProperty(proxy,'gone');let keys=Reflect.ownKeys(proxy);got===3&&receiver.stored===4&&deleted&&keys[0]==='listed'&&log.join(',')==='get:value,set:stored,delete:gone,keys'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let proxy=new Proxy(function(value){return value+1},{apply(target,thisArgument,args){return target(args[0])+1}});proxy(3)"
        )
        .unwrap(),
        Value::Number(5.0)
    );
    assert_eq!(
        evaluate(
            "let target={};let proxy=new Proxy(target,{defineProperty(t,k,d){t[k]=d.value;return true},getOwnPropertyDescriptor(t,k){return {value:t[k],writable:true,enumerable:true,configurable:true}}});Reflect.defineProperty(proxy,'kept',{value:6,writable:true,enumerable:true,configurable:true})&&Object.getOwnPropertyDescriptor(proxy,'kept').value===6"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let target={};let descriptors={visible:{value:1}};Object.defineProperty(descriptors,'hidden',{value:{value:2},enumerable:false});Object.defineProperties(target,descriptors);target.visible===1&&target.hidden===undefined"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let log=[];let source=new Proxy({value:3},{ownKeys(){log.push('keys');return ['value']},getOwnPropertyDescriptor(target,key){log.push('descriptor');return {value:target[key],writable:true,enumerable:true,configurable:true}},get(target,key){log.push('get');return target[key]}});let copy={...source};copy.value===3&&log.join(',')==='keys,descriptor,get'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let log=[];let properties=new Proxy({item:{value:4}},{ownKeys(){log.push('keys');return ['item']},getOwnPropertyDescriptor(target,key){log.push('descriptor');return {value:target[key],writable:true,enumerable:true,configurable:true}},get(target,key){log.push('get');return target[key]}});let object=Object.create(null,properties);object.item===4&&log.join(',')==='keys,descriptor,get'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let log=[];let proxy=new Proxy({item:1},{ownKeys(){log.push('keys');return ['item']},getOwnPropertyDescriptor(target,key){log.push('descriptor');return {value:target[key],writable:true,enumerable:true,configurable:true}},getPrototypeOf(){log.push('prototype');return null}});let names=[];for(let key in proxy){names.push(key)}names.length===1&&names[0]==='item'&&log.join(',')==='keys,descriptor,prototype'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let calls=0;let descriptor=new Proxy({value:5},{has(target,key){if(key==='value')calls++;return key in target},get(target,key){return target[key]}});let target={};Object.defineProperty(target,'value',descriptor);target.value===5&&calls===1"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let log=[];let target={};let proxy=new Proxy(target,{ownKeys(){log.push('keys');return []},preventExtensions(target){log.push('prevent');Object.preventExtensions(target);return true}});Object.freeze(proxy)===proxy&&!Object.isExtensible(proxy)&&log.join(',')==='prevent,keys'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let calls=0;let proxy=new Proxy({0:4,length:1},{has(target,key){if(key==='0')calls++;return key in target},get(target,key){return target[key]}});let sum=0;Array.prototype.forEach.call(proxy,value=>sum+=value);sum===4&&calls===1"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let log=[];let proxy=new Proxy({shown:2},{ownKeys(){log.push('keys');return ['shown']},getOwnPropertyDescriptor(target,key){log.push('descriptor');return {value:target[key],writable:true,enumerable:true,configurable:true}},get(target,key){log.push('get');return target[key]}});JSON.stringify(proxy)==='{\"shown\":2}'&&log.join(',')==='keys,descriptor,get'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let target={};let prototype={};let proxy=new Proxy(target,{getPrototypeOf(){return prototype},setPrototypeOf(t,p){return p===prototype},isExtensible(){return true},preventExtensions(t){Object.preventExtensions(t);return true}});Object.getPrototypeOf(proxy)===prototype&&Reflect.setPrototypeOf(proxy,prototype)&&Reflect.isExtensible(proxy)&&Reflect.preventExtensions(proxy)&&!Object.isExtensible(target)"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let object={};let prototype={};Object.setPrototypeOf(object,prototype)===object&&Object.getPrototypeOf(object)===prototype&&!Reflect.setPrototypeOf(object,object)&&Object.getPrototypeOf(object)===prototype"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert!(matches!(
        evaluate("let object={};Object.setPrototypeOf(object,object)"),
        Err(RuntimeError::TypeError(_))
    ));
    assert_eq!(
        evaluate(
            "let record=Proxy.revocable({value:1},{});let before=record.proxy.value;record.revoke();let revoked=false;try{record.proxy.value}catch(error){revoked=error instanceof TypeError}before===1&&revoked&&record.revoke()===undefined"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let record=Proxy.revocable(function(){},{});record.revoke();typeof new Proxy(record.proxy,{})==='function'"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let record=Proxy.revocable({},{});let retained=[];for(let index=0;index<400;index++){retained.push({index})}record.revoke();let revoked=false;try{Reflect.ownKeys(record.proxy)}catch(error){revoked=error instanceof TypeError}revoked"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let target={};Object.defineProperty(target,'value',{value:1,writable:false,enumerable:false,configurable:false});let proxy=new Proxy(target,{getOwnPropertyDescriptor(){return {value:1}}});let descriptor=Object.getOwnPropertyDescriptor(proxy,'value');descriptor.value===1&&descriptor.writable===false&&descriptor.enumerable===false&&descriptor.configurable===false"
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        evaluate(
            "let observed=false;let proxy=new Proxy(function(){},{construct(target,args,newTarget){observed=args.length===1&&args[0]===4&&newTarget===proxy;return {}}});new proxy(4);observed"
        )
        .unwrap(),
        Value::Bool(true)
    );
}
