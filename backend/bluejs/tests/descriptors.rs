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
            "let errors=0;for(let value of [1,null,undefined,'']){try{Reflect.getPrototypeOf(value)}catch(error){errors++};try{Reflect.isExtensible(value)}catch(error){errors++};try{Reflect.setPrototypeOf(value,{})}catch(error){errors++}}errors===12&&Reflect.set({p:42},'p',43,'receiver')===false&&Reflect[Symbol.toStringTag]==='Reflect'",
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
            "let target={};let proxy=new Proxy(target,{ownKeys(){return ['transient']},getOwnPropertyDescriptor(){return undefined},preventExtensions(target){Object.preventExtensions(target);return true}});Object.freeze(proxy)===proxy&&!Object.isExtensible(proxy)"
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
