// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn detaching_a_buffer_invalidates_its_views_through_the_test262_host_hook() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let buffer=new ArrayBuffer(2);let view=new DataView(buffer);let typed=new Uint8Array(buffer);view.setUint8(0,1);$262.detachArrayBuffer(buffer);let detached=false;let definition=false;try{view.getUint8(0)}catch(error){detached=error instanceof TypeError}try{Object.defineProperty(typed,'0',{value:1})}catch(error){definition=error instanceof TypeError}let orderedBuffer=new ArrayBuffer(1);$262.detachArrayBuffer(orderedBuffer);let converted=false;let ordered=false;try{new DataView(orderedBuffer,{valueOf:function(){converted=true;return 0}})}catch(error){ordered=error instanceof TypeError}let conversionBuffer=new ArrayBuffer(1);let conversionView=new DataView(conversionBuffer);$262.detachArrayBuffer(conversionBuffer);let valueConverted=false;let valueOrdered=false;try{conversionView.setUint8(0,{valueOf:function(){valueConverted=true;return 1}})}catch(error){valueOrdered=error instanceof TypeError}let setBuffer=new ArrayBuffer(1);let setTyped=new Uint8Array(setBuffer);let setValue={valueOf:function(){$262.detachArrayBuffer(setBuffer);return 7}};let defineBuffer=new ArrayBuffer(1);let defineTyped=new Uint8Array(defineBuffer);let defineValue={valueOf:function(){$262.detachArrayBuffer(defineBuffer);return 7}};detached&&definition&&ordered&&converted&&valueOrdered&&valueConverted&&view.buffer===buffer&&buffer.byteLength===0&&typed.length===0&&typed.byteLength===0&&typed.byteOffset===0&&typed[0]===undefined&&Reflect.set(typed,'0',1)===true&&Reflect.deleteProperty(typed,'0')===true&&Reflect.set(setTyped,'0',setValue)&&setTyped[0]===undefined&&Reflect.defineProperty(defineTyped,'0',{value:defineValue})&&defineTyped[0]===undefined";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn typed_array_set_converts_its_offset_before_validating_a_detached_target() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let buffer=new ArrayBuffer(4);let typed=new Int32Array(buffer);let marker={};$262.detachArrayBuffer(buffer);let caught=false;try{typed.set(null,{valueOf:function(){throw marker}})}catch(error){caught=error===marker}caught";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn typed_array_set_validates_a_detached_source_before_target_bounds() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let target=new Int32Array(1);let source=new Int32Array(1);let buffer=source.buffer;let caught=false;try{target.set(source,{valueOf:function(){$262.detachArrayBuffer(buffer);return 1000000}})}catch(error){caught=error instanceof TypeError}caught";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn native_getter_to_string_uses_its_immutable_accessor_initial_name() {
    let source = "let getter=Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;getter.name==='get byteLength'&&Function.prototype.toString.call(getter)==='function get byteLength() { [native code] }'";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn typed_array_from_constructs_an_array_like_target_before_reading_elements() {
    let source = "let log='';let marker={};function C(length){log+='C';return new Uint8Array(length)}let source={get length(){log+='l';return 1},get 0(){log+='0';return 7}};try{Uint8Array.from.call(C,source,function(){throw marker})}catch(error){}log==='lC0'";
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn typed_array_from_foreign_callback_writes_through_an_imported_parent_global() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let g=$262.createRealm().global;let h=$262.createRealm().global;h.mainGlobal=this;h.eval(\"function f(){mainGlobal.result=this}\");g.Uint8Array.from.call(Uint8Array,[5],h.f);this.globalName='main';h.globalName='h';result.globalName==='h'";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn test262_host_hook_detaches_array_buffers_owned_by_another_realm() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let other=$262.createRealm().global;let typed=new other.Uint8Array(1);let buffer=typed.buffer;$262.detachArrayBuffer(buffer);typed[0]===undefined&&buffer.byteLength===0&&typed.length===0";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn typed_array_generic_methods_route_to_the_receivers_test262_realm() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let other=$262.createRealm().global;let local=new Uint8Array([1,2]);let remote=new other.Uint8Array(2);remote[0]=3;Object.defineProperty(remote,'length',{get:function(){throw new Error('foreign length should not be read')}});let copied=new Uint8Array(2);copied.set(remote);let foreignBuffer=new other.ArrayBuffer(2);let mirrored=new Uint8Array(foreignBuffer);let exposedBuffer=mirrored.buffer;other.$262.detachArrayBuffer(exposedBuffer);let detached=false;try{mirrored.set([1])}catch(error){detached=error instanceof TypeError}let localEntries=other.Uint8Array.prototype.entries.call(local);let remoteEntries=Uint8Array.prototype.entries.call(remote);let iterator=new Uint8Array([9])[Symbol.iterator]();iterator.next=other.Array.prototype[Symbol.iterator]().next;let fromRemote=Uint8Array.from.call(other.Uint8Array,[5,6]);let C=new other.Function();C.prototype=null;let constructed=Reflect.construct(Int8Array,[0],C);localEntries.next().value.join()==='0,1'&&remoteEntries.next().value.join()==='0,3'&&iterator.next().value===9&&iterator.next().done&&copied[0]===3&&copied[1]===0&&mirrored.length===0&&exposedBuffer===foreignBuffer&&detached&&fromRemote instanceof other.Uint8Array&&fromRemote[1]===6&&Object.getPrototypeOf(constructed)===other.Int8Array.prototype";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn parent_array_buffers_remain_live_when_constructing_foreign_typed_arrays() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let other=$262.createRealm().global;let buffer=new ArrayBuffer(2);let remote=new other.Uint8Array(buffer);remote[0]=7;let local=new Uint8Array(buffer);let visible=remote[0]===7&&local[0]===7;other.$262.detachArrayBuffer(buffer);visible&&buffer.byteLength===0&&local.length===0&&remote.length===0";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn typed_array_sort_ends_cleanly_when_a_comparator_detaches_the_buffer() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let typed=new Int8Array(4);let buffer=typed.buffer;let called=false;typed.sort(function(){$262.detachArrayBuffer(buffer);return {[Symbol.toPrimitive]:function(){called=true}}});called";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn fixed_length_array_buffers_views_and_typed_indices_share_backing_bytes() {
    for (source, expected) in [
        (
            "let buffer=new ArrayBuffer(6);buffer.byteLength===6&&new Uint8Array(buffer)[0]===0",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4);let view=new DataView(buffer);view.setUint16(0,0x1234);view.getUint8(0)===0x12&&view.getUint8(1)===0x34&&view.getUint16(0)===0x1234",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4);let bytes=new Uint8Array(buffer);bytes[0]=257;bytes[1]=3;let signed=new Int16Array(buffer);bytes[0]===1&&signed[0]===769&&bytes.length===4&&bytes.byteLength===4&&bytes.byteOffset===0&&bytes.buffer===buffer",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4);let bytes=new Uint8Array(buffer);bytes.set([7,8],1);let copy=buffer.slice(1,3);let copied=new Uint8Array(copy);copy.byteLength===2&&copied[0]===7&&copied[1]===8&&ArrayBuffer.isView(bytes)&&ArrayBuffer.isView(new DataView(buffer))&&!ArrayBuffer.isView(buffer)",
            Value::Bool(true),
        ),
        (
            "let bytes=new Uint8Array([257,2]);let copied=new Int16Array(bytes);bytes.length===2&&bytes[0]===1&&copied.length===2&&copied[0]===1&&Uint8Array.BYTES_PER_ELEMENT===1&&Int16Array.BYTES_PER_ELEMENT===2",
            Value::Bool(true),
        ),
        (
            "Int8Array.length===3&&Uint8Array.length===3&&Float64Array.length===3&&BigInt64Array.length===3",
            Value::Bool(true),
        ),
        (
            "let typed=new Uint8Array([1,2]);Object.defineProperty(typed,'length',{get:function(){throw new Error('unexpected length get')}});typed.toLocaleString()==='1,2'&&Object.getPrototypeOf(Int8Array).prototype.toString===Array.prototype.toString",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4,{maxByteLength:4});let fixed=new Uint8Array(buffer,0,4);let keys=fixed.keys();let values=fixed.values();keys.next();values.next();buffer.resize(3);let keyThrows=false;let valueThrows=false;try{keys.next()}catch(error){keyThrows=error instanceof TypeError}try{values.next()}catch(error){valueThrows=error instanceof TypeError}keyThrows&&valueThrows",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(8);let view=new DataView(buffer);view.setFloat32(0,1.5,true);view.setFloat64(0,Math.PI,true);view.getFloat64(0,true)===Math.PI",
            Value::Bool(true),
        ),
        (
            "ArrayBuffer[Symbol.species]===ArrayBuffer&&new ArrayBuffer(NaN).byteLength===0",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(2);let view=new DataView(buffer,0,-0.5);view.byteLength===0&&view.byteOffset===0",
            Value::Bool(true),
        ),
        (
            "let buffer=Reflect.construct(ArrayBuffer,[2],Object);function NewTarget(){}let prototype={};NewTarget.prototype=prototype;let view=Reflect.construct(DataView,[new ArrayBuffer(2),0],NewTarget);Object.getPrototypeOf(buffer)===Object.prototype&&Object.getPrototypeOf(view)===prototype",
            Value::Bool(true),
        ),
        (
            "function NewTarget(){}let prototype={};NewTarget.prototype=prototype;let typed=Reflect.construct(Uint8Array,[2],NewTarget);Object.getPrototypeOf(typed)===prototype&&typed[0]===0",
            Value::Bool(true),
        ),
        (
            "let source=new ArrayBuffer(2);new Uint8Array(source).set([5,6]);let holder={};let expected;holder[Symbol.species]=function(length){return expected=new ArrayBuffer(length+1)};source.constructor=holder;let copy=source.slice();copy===expected&&copy.byteLength===3&&new Uint8Array(copy)[0]===5&&new Uint8Array(copy)[1]===6",
            Value::Bool(true),
        ),
        (
            "let source=new ArrayBuffer(2);let holder={};holder[Symbol.species]=function(){return source};source.constructor=holder;let caught=false;try{source.slice()}catch(error){caught=error instanceof TypeError}caught",
            Value::Bool(true),
        ),
        (
            "let source=new ArrayBuffer(2);let holder={};holder[Symbol.species]={};source.constructor=holder;let caught=false;try{source.slice()}catch(error){caught=error instanceof TypeError}caught",
            Value::Bool(true),
        ),
        (
            "let typed=new Uint8Array(2);let defined=Reflect.defineProperty(typed,'0',{value:257,writable:true,enumerable:true,configurable:true});let rejected=!Reflect.defineProperty(typed,'-0',{value:1})&&!Reflect.defineProperty(typed,'1.5',{value:1})&&!Reflect.defineProperty(typed,'2',{value:1})&&!Reflect.defineProperty(typed,'0',{writable:false});let ordinary=Reflect.defineProperty(typed,'01',{value:5});let descriptor=Object.getOwnPropertyDescriptor(typed,'0');defined&&rejected&&ordinary&&typed[0]===1&&typed['-0']===undefined&&typed['1.5']===undefined&&typed['01']===5&&descriptor.writable&&descriptor.enumerable&&descriptor.configurable&&Reflect.deleteProperty(typed,'0')===false",
            Value::Bool(true),
        ),
        (
            "let values=[3,4];let iterable={};iterable[Symbol.iterator]=function(){return values[Symbol.iterator]()};let typed=new Int16Array(iterable);let view=typed.subarray(-1);typed[1]=9;typed.length===2&&typed[0]===3&&view.length===1&&view[0]===9&&view.byteOffset===2&&view.buffer===typed.buffer",
            Value::Bool(true),
        ),
        (
            "let TypedArray=Object.getPrototypeOf(Int8Array);TypedArray.name==='TypedArray'&&TypedArray.prototype===Object.getPrototypeOf(Int8Array.prototype)&&TypedArray.prototype.set===Int8Array.prototype.set",
            Value::Bool(true),
        ),
        (
            "let TypedArray=Object.getPrototypeOf(Int8Array);class Derived extends TypedArray{constructor(...args){return Reflect.construct(Int8Array,args,new.target)}}let typed=new Derived([7,8]);Object.getPrototypeOf(Derived)===TypedArray&&typed instanceof Derived&&typed.length===2&&typed[0]===7&&typed[1]===8",
            Value::Bool(true),
        ),
        (
            "let locale='th-u-nu-thai';let options={minimumFractionDigits:3};let expected=(0).toLocaleString(locale,options);let descriptor=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Int8Array).prototype,'toLocaleString');new Uint8Array([0]).toLocaleString(locale,options)===expected&&new BigInt64Array([0n]).toLocaleString(locale,options)===expected&&descriptor.writable&&descriptor.configurable&&!descriptor.enumerable&&Object.getPrototypeOf(Int8Array).prototype.toLocaleString.length===0",
            Value::Bool(true),
        ),
        (
            "let ctors=[Float64Array,Float32Array,Int32Array,Int16Array,Int8Array,Uint32Array,Uint16Array,Uint8Array,Uint8ClampedArray];let mismatches='';ctors.forEach(function(T){let sample=new T([42,43]);let first=Object.getOwnPropertyDescriptor(sample,'0');let second=Object.getOwnPropertyDescriptor(sample,'1');if(first.value!==42||second.value!==43||!first.writable||!first.enumerable||!first.configurable){mismatches+=T.name}});mismatches",
            Value::String("".into()),
        ),
        (
            "let values=Array.from({length:2},function(){return 7});let typed=new Uint8Array(values);values.length===2&&values[0]===7&&values[1]===7&&typed[0]===7&&typed[1]===7",
            Value::Bool(true),
        ),
        (
            "let TypedArray=Object.getPrototypeOf(Int8Array);let typed=new Uint8Array();let key='1.0';TypedArray.prototype[key]='inherited';let inherited=typed[key]==='inherited';typed[key]='own';Object.defineProperty(typed,key,{get:function(){return 'accessor'}});let result=inherited&&typed[key]==='accessor';delete TypedArray.prototype[key];result",
            Value::Bool(true),
        ),
        (
            "let values=[42,43];let iterable={};iterable[Symbol.iterator]=function(){return values[Symbol.iterator]()};let ctors=[Float64Array,Float32Array,Int32Array,Int16Array,Int8Array,Uint32Array,Uint16Array,Uint8Array,Uint8ClampedArray];let valid=true;ctors.forEach(function(T){let inputs=[values,Array.from(values),{0:42,1:43,length:2},iterable,(new T(values)).buffer];inputs.forEach(function(input){let typed=new T(input);valid=valid&&typed.length===2&&typed[0]===42&&typed[1]===43})});valid",
            Value::Bool(true),
        ),
        (
            "let typed=new Uint8Array();let key='0.0000001';let data={value:42,writable:true,configurable:true};let getter=function(){return 7};let setter=function(){};let accessor={get:getter,set:setter,enumerable:true,configurable:false};let dataOK=Reflect.defineProperty(typed,key,data)&&typed[key]===42&&Object.getOwnPropertyDescriptor(typed,key).configurable;let writable=Reflect.set(typed,key,8)&&typed[key]===8;let removed=Reflect.deleteProperty(typed,key)&&typed[key]===undefined;let accessorOK=Reflect.defineProperty(typed,key,accessor);let descriptor=Object.getOwnPropertyDescriptor(typed,key);dataOK&&writable&&removed&&accessorOK&&descriptor.get===getter&&descriptor.set===setter&&descriptor.enumerable&&!descriptor.configurable",
            Value::Bool(true),
        ),
        (
            "let typed=new Int32Array(1);let receiver={};let converted=0;let value={valueOf:function(){converted++;return 9}};let valid=Reflect.set(typed,'0',value,receiver)&&receiver[0]===value&&typed[0]===0&&converted===0;let invalid=Reflect.set(typed,'2',value,receiver)&&receiver[2]===undefined&&converted===0;Int32Array.prototype['1.5']='inherited';let canonical=typed['1.5']===undefined&&!Reflect.has(typed,'1.5');delete Int32Array.prototype['1.5'];valid&&invalid&&canonical",
            Value::Bool(true),
        ),
        (
            "let typed=new Uint8Array([1]);let marker={};let conversions=0;let value={valueOf:function(){conversions++;throw marker}};let caught=function(key){try{typed[key]=value}catch(error){return error===marker}return false};let own=caught('-0')&&caught('1.5')&&caught('-1')&&caught('1')&&conversions===4&&typed[0]===1;let receiver=Object.create(typed);let before=conversions;receiver['1.5']=value;own&&conversions===before&&!Object.prototype.hasOwnProperty.call(receiver,'1.5')",
            Value::Bool(true),
        ),
        (
            "let typed=new Uint8Array([42,43]);Object.preventExtensions(typed);let string=Reflect.defineProperty(typed,'foo',{value:42})===false&&Reflect.getOwnPropertyDescriptor(typed,'foo')===undefined;let key=Symbol('1');let symbol=Reflect.defineProperty(typed,key,{value:42})===false&&Reflect.getOwnPropertyDescriptor(typed,key)===undefined;string&&symbol",
            Value::Bool(true),
        ),
        (
            "let receiver=new Int32Array(1);let object=Object.create(receiver);let conversions=0;let value={valueOf:function(){conversions++;return 1}};Reflect.set(object,'100',value,receiver)&&conversions===1&&receiver[0]===0",
            Value::Bool(true),
        ),
        (
            "let TypedArray=Object.getPrototypeOf(Int8Array);let calls=0;Object.defineProperties(TypedArray.prototype,{'0':{configurable:true,get:function(){calls++;return 7}}});let typed=new Uint8Array([3]);let result=typed[0]===3&&Reflect.deleteProperty(typed,'0')===false&&calls===0;delete TypedArray.prototype['0'];result",
            Value::Bool(true),
        ),
        (
            "let signed=new BigInt64Array([-1n,9223372036854775808n]);let unsigned=new BigUint64Array(1);unsigned[0]=-1n;let view=new DataView(unsigned.buffer);let typeError=false;try{signed[0]=1}catch(error){typeError=error instanceof TypeError}signed.length===2&&signed[0]===-1n&&signed[1]===-9223372036854775808n&&unsigned[0]===18446744073709551615n&&view.getBigInt64(0,true)===-1n&&view.getBigUint64(0,true)===18446744073709551615n&&BigInt64Array.BYTES_PER_ELEMENT===8&&BigUint64Array.BYTES_PER_ELEMENT===8&&typeError",
            Value::Bool(true),
        ),
        (
            "let typed=new BigInt64Array(4);let conversions=0;let value={valueOf:function(){conversions++;return '3'}};typed.set([false,true],0);typed[2]='2';typed[3]=value;typed[0]===0n&&typed[1]===1n&&typed[2]===2n&&typed[3]===3n&&conversions===1",
            Value::Bool(true),
        ),
        (
            "let target=new BigInt64Array(4);let source={length:3};let events=[];Object.defineProperty(source,'0',{get:function(){events.push(target.join());return 4n}});Object.defineProperty(source,'1',{get:function(){events.push(target.join());return 5n}});Object.defineProperty(source,'2',{get:function(){events.push(target.join());return 6n}});target.set(source,1);let typedSource=new BigInt64Array([7n,8n]);let lengthGets=0;Object.defineProperty(typedSource,'length',{get:function(){lengthGets++;return 99}});let typedTarget=new BigInt64Array(2);typedTarget.set(typedSource);target.join()==='0,4,5,6'&&events.join('|')==='0,0,0,0|0,4,0,0|0,4,5,0'&&typedTarget.join()==='7,8'&&lengthGets===0",
            Value::Bool(true),
        ),
        (
            "let source=new Int16Array([4,5,6]);let calls=0;source.constructor={};source.constructor[Symbol.species]=function(buffer,offset,length){calls++;return new Uint8Array(buffer,offset,length)};let result=source.subarray(1);calls===1&&result instanceof Uint8Array&&result.length===2",
            Value::Bool(true),
        ),
        (
            "let typed=new Uint8Array([10,20,30,40,50,60]);typed.constructor={};typed.constructor[Symbol.species]=function(){return new Uint8Array(typed.buffer,2)};typed.slice(1,4).join()==='20,20,20,60'",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(10,{maxByteLength:20});let typed=new Float64Array(buffer);let initial=typed.length===1&&typed.byteLength===8&&typed[0]===0;buffer.resize(16);initial&&typed.length===2&&typed[1]===0",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4,{maxByteLength:4});let typed=new Uint8Array(buffer);typed.set([1,2,3,4]);let seen=[];typed.forEach(function(value,index){seen.push(value);if(index===1)buffer.resize(2)});seen.join()==='1,2,,'",
            Value::Bool(true),
        ),
        (
            "let typed=new Uint8Array([42,43]);typed.at(NaN)===42&&typed.at(-3)===undefined&&typed.includes(42,NaN)&&typed.indexOf(42,NaN)===0&&typed.lastIndexOf(42,NaN)===0&&typed.lastIndexOf(43,undefined)===-1",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4,{maxByteLength:8});let tracking=new Uint8Array(buffer);let fixed=new Uint8Array(buffer,0,4);let view=new DataView(buffer);tracking[3]=7;buffer.resize(6);let grown=buffer.resizable&&buffer.maxByteLength===8&&buffer.byteLength===6&&tracking.length===6&&tracking[3]===7&&tracking[5]===0&&fixed.length===4&&view.byteLength===6;buffer.resize(2);let rejected=false;try{fixed.set([1])}catch(error){rejected=error instanceof TypeError}grown&&buffer.byteLength===2&&tracking.length===2&&view.byteLength===2&&fixed.length===0&&fixed.byteLength===0&&fixed.byteOffset===0&&fixed[0]===undefined&&rejected",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(0,{maxByteLength:1});let typed=new Int8Array(buffer);typed[0]={valueOf:function(){buffer.resize(1);return 100}};typed.length===1&&typed[0]===100",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(2,{maxByteLength:5});let typed=new Int8Array(buffer);typed[0]=11;typed[1]=22;let replacement={valueOf:function(){buffer.resize(5);return 123}};let result=typed.with(4,replacement);let marker={};let thrown=false;try{typed.with(100,{valueOf:function(){throw marker}})}catch(error){thrown=error===marker}result.length===2&&result[0]===11&&result[1]===22&&typed.length===5&&thrown&&typed.with(NaN,7)[0]===7",
            Value::Bool(true),
        ),
        (
            "let values=[0,{valueOf:function(){values.length=0;return 100}},2];let typed=new Uint8Array(values);typed.length===3&&typed[0]===0&&typed[1]===100&&typed[2]===2",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4,{maxByteLength:4});let fixed=new Uint8Array(buffer,0,4);buffer.resize(3);let rejected=false;try{new Uint8Array(fixed)}catch(error){rejected=error instanceof TypeError}rejected",
            Value::Bool(true),
        ),
        (
            "let buffer=new SharedArrayBuffer(4,{maxByteLength:8});let bytes=new Uint8Array(buffer);let view=new DataView(buffer);bytes[0]=7;buffer.grow(6);let slice=buffer.slice(0,1);let rejected=false;try{ArrayBuffer.prototype.resize.call(buffer,1)}catch(error){rejected=error instanceof TypeError}buffer.growable&&buffer.maxByteLength===8&&buffer.byteLength===6&&bytes.length===6&&bytes[0]===7&&bytes[5]===0&&view.byteLength===6&&slice instanceof SharedArrayBuffer&&new Uint8Array(slice)[0]===7&&rejected",
            Value::Bool(true),
        ),
        (
            "let buffer=new SharedArrayBuffer(16);let ints=new Int32Array(buffer);let big=new BigInt64Array(buffer);let stored=Atomics.store(ints,0,5)===5;let added=Atomics.add(ints,0,2)===5&&Atomics.load(ints,0)===7;let bits=Atomics.or(ints,0,8)===7&&Atomics.and(ints,0,13)===15&&Atomics.xor(ints,0,3)===13&&Atomics.sub(ints,0,3)===14;let exchanged=Atomics.exchange(ints,0,4)===11&&Atomics.compareExchange(ints,0,4,9)===4&&ints[0]===9;let bigint=Atomics.store(big,1,5n)===5n&&Atomics.add(big,1,2n)===5n&&Atomics.load(big,1)===7n;let waiting=Atomics.wait(ints,0,8,0)==='not-equal'&&Atomics.wait(ints,0,9,0)==='timed-out';let async=Atomics.waitAsync(ints,0,8,0);stored&&added&&bits&&exchanged&&bigint&&waiting&&async.async===false&&async.value==='not-equal'&&Atomics.notify(ints,0)===0&&Atomics.isLockFree(4)&&Atomics.pause()===undefined",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(16);let ints=new Int32Array(buffer);let big=new BigInt64Array(buffer);let stored=Atomics.store(ints,0,5)===5;let added=Atomics.add(ints,0,2)===5&&Atomics.load(ints,0)===7;let bits=Atomics.or(ints,0,8)===7&&Atomics.and(ints,0,13)===15&&Atomics.xor(ints,0,3)===13&&Atomics.sub(ints,0,3)===14;let exchanged=Atomics.exchange(ints,0,4)===11&&Atomics.compareExchange(ints,0,4,9)===4&&ints[0]===9;let bigint=Atomics.store(big,1,5n)===5n&&Atomics.add(big,1,2n)===5n&&Atomics.load(big,1)===7n;let notified=Atomics.notify(ints,0)===0&&Atomics.notify(ints,0,1)===0;let waitThrows=false;try{Atomics.wait(ints,0,7,0)}catch(error){waitThrows=error instanceof TypeError}let waitAsyncThrows=false;try{Atomics.waitAsync(ints,0,7,0)}catch(error){waitAsyncThrows=error instanceof TypeError}stored&&added&&bits&&exchanged&&bigint&&notified&&waitThrows&&waitAsyncThrows",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4);let ints=new Int32Array(buffer);let observedIndex=false;let observedCount=false;let poisonedIndex={valueOf:function(){observedIndex=true;return 0}};let poisonedCount={valueOf:function(){observedCount=true;return 1}};let result=Atomics.notify(ints,poisonedIndex,poisonedCount);result===0&&observedIndex&&observedCount",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4);let ints=new Int32Array(buffer);let poisoned={valueOf:function(){throw new TypeError('should not be observed')}};let throws=false;try{Atomics.wait(ints,poisoned,poisoned,poisoned)}catch(error){throws=error instanceof TypeError&&error.message!=='should not be observed'}let plainThrows=false;try{Atomics.wait(ints,0,0,0)}catch(error){plainThrows=error instanceof TypeError}throws&&plainThrows",
            Value::Bool(true),
        ),
        (
            "let source=new ArrayBuffer(4,{maxByteLength:8});new Uint8Array(source).set([1,2,3,4]);let moved=source.transfer(6);let preserving=moved.resizable&&moved.maxByteLength===8&&moved.byteLength===6;let fixed=moved.transferToFixedLength(3);let detached=source.byteLength===0&&moved.byteLength===0;preserving&&detached&&moved.resizable===false&&fixed.resizable===false&&fixed.maxByteLength===3&&fixed.byteLength===3&&new Uint8Array(fixed).join()==='1,2,3'",
            Value::Bool(true),
        ),
        (
            "let typed=new Int16Array([1,2,3,4]);let read=typed.at(-1)===4&&typed.includes(2)&&typed.indexOf(3)===2&&typed.join('-')==='1-2-3-4'&&typed.reduce(function(total,value){return total+value},0)===10&&typed.reduceRight(function(total,value){return total-value},0)===-10;let callback=typed.every(function(value){return value>0})&&typed.some(function(value){return value===3})&&typed.find(function(value){return value>2})===3&&typed.findIndex(function(value){return value===3})===2&&typed.findLast(function(value){return value<4})===3&&typed.findLastIndex(function(value){return value<4})===2;let mapped=typed.map(function(value){return value*2});let filtered=typed.filter(function(value){return value%2===0});typed.copyWithin(1,2);typed.fill(9,3);let altered=typed[0]===1&&typed[1]===3&&typed[2]===4&&typed[3]===9;typed.reverse();read&&callback&&mapped[0]===2&&mapped[3]===8&&filtered.length===2&&filtered[0]===2&&filtered[1]===4&&altered&&typed[0]===9&&typed[3]===1&&typed.values===typed[Symbol.iterator]",
            Value::Bool(true),
        ),
        (
            "let typed=new Int16Array([3,1,2,1]);let keys=typed.keys();let entries=typed.entries();let iterators=keys.next().value===0&&keys.next().value===1&&entries.next().value[0]===0&&entries.next().value[1]===1;let last=typed.lastIndexOf(1)===3&&typed.lastIndexOf(1,2)===1;let sliced=typed.slice(1,3);let reversed=typed.toReversed();let replaced=typed.with(-1,9);let sorted=typed.toSorted();typed.sort(function(a,b){return b-a});iterators&&last&&sliced.join()==='1,2'&&reversed.join()==='1,2,1,3'&&replaced.join()==='3,1,2,9'&&sorted.join()==='1,1,2,3'&&typed.join()==='3,2,1,1'",
            Value::Bool(true),
        ),
        (
            "let typed=new Uint8Array([1,2]);let calls=0;let holder={};holder[Symbol.species]=function(length){calls++;return new Int16Array(length)};typed.constructor=holder;let mapped=typed.map(function(value){return value+1});let filtered=typed.filter(function(value){return value>1});let sliced=typed.slice(0,1);calls===3&&mapped instanceof Int16Array&&mapped.join()==='2,3'&&filtered instanceof Int16Array&&filtered[0]===2&&sliced instanceof Int16Array&&sliced[0]===1",
            Value::Bool(true),
        ),
        (
            "let fromArrayLike=Int32Array.from({length:3,0:1,1:2,2:3});let fromIterable=Int32Array.from([4,5]);let mapped=Int32Array.from([1,2,3],function(value,index){return value*10+index});let withThis=Uint8Array.from([1],function(value){return value+this.offset},{offset:5});let of=Int16Array.of(7,8,9);fromArrayLike.join()==='1,2,3'&&fromIterable.join()==='4,5'&&mapped.join()==='10,21,32'&&withThis.join()==='6'&&of.join()==='7,8,9'&&Int32Array.from.length===1&&Int32Array.of.length===0",
            Value::Bool(true),
        ),
        (
            "let caught=false;try{Int32Array.from.call({},[1])}catch(error){caught=error instanceof TypeError}let caughtOf=false;try{Int32Array.of.call(null)}catch(error){caughtOf=error instanceof TypeError}caught&&caughtOf",
            Value::Bool(true),
        ),
        (
            "let TypedArray=Object.getPrototypeOf(Int8Array);let descriptor=Object.getOwnPropertyDescriptor(TypedArray.prototype,Symbol.toStringTag);let getter=descriptor.get;let typed=new Uint8Array(2);let buffer=new ArrayBuffer(1);let tagged=Object.prototype.toString.call(typed)==='[object Uint8Array]'&&Object.prototype.toString.call(new Float64Array())==='[object Float64Array]';let inherited=!Int8Array.prototype.hasOwnProperty(Symbol.toStringTag)&&typed[Symbol.toStringTag]==='Uint8Array';let undef=getter.call({})===undefined&&getter.call(null)===undefined&&getter.call(42)===undefined&&getter.call(buffer)===undefined;let shape=descriptor.set===undefined&&descriptor.enumerable===false&&descriptor.configurable===true&&getter.length===0&&getter.name==='get [Symbol.toStringTag]';tagged&&inherited&&undef&&shape",
            Value::Bool(true),
        ),
        (
            "let buffer=new ArrayBuffer(4);let view=new DataView(buffer);view.setFloat16(0,2.158203125,false);let bytes=[view.getUint8(0),view.getUint8(1)];view.setFloat16(0,42,true);let roundTrip=view.getFloat16(0,true)===42;let crossEndian=view.getFloat16(0,false)===2.158203125;let inf=(view.setFloat16(2,Infinity,true),view.getFloat16(2,true))===Infinity;let nan=(view.setFloat16(2,NaN,true),view.getFloat16(2,true)!==view.getFloat16(2,true));let typed=new Float16Array([1.5,-2.5,65504]);let tagged=Object.prototype.toString.call(typed)==='[object Float16Array]';bytes[0]===0x40&&bytes[1]===0x51&&roundTrip&&crossEndian&&inf&&nan&&typed.join()==='1.5,-2.5,65504'&&typed.BYTES_PER_ELEMENT===2&&Float16Array.BYTES_PER_ELEMENT===2&&tagged",
            Value::Bool(true),
        ),
    ] {
        let actual = evaluate(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(actual, expected, "{source}");
    }
}

#[test]
fn typed_array_prototype_tostringtag_getter_reports_the_kind_of_a_detached_view() {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    let source = "let typed=new Uint8Array(new ArrayBuffer(2));let buffer=typed.buffer;let before=typed[Symbol.toStringTag];$262.detachArrayBuffer(buffer);before==='Uint8Array'&&typed[Symbol.toStringTag]==='Uint8Array'&&typed.buffer===buffer&&typed.length===0";
    assert_eq!(
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap(),
        Value::Bool(true)
    );
}

fn evaluate_with_host(source: &str) -> Value {
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.execute(&compile(&parse(source).unwrap()).unwrap())
        .unwrap_or_else(|error| panic!("{source}: {error}"))
}

#[test]
fn atomics_store_returns_the_coerced_value_rather_than_the_wrapped_element() {
    for source in [
        // Numbers: ToIntegerOrInfinity(value), so neither truncation to the
        // element width nor a stored NaN/Infinity leaks into the result.
        "let i16=new Int16Array(new SharedArrayBuffer(8));let wide=Atomics.store(i16,0,123456789)===123456789&&i16[0]===-13035;let ints=Atomics.store(i16,1,3.9)===3&&Atomics.store(i16,1,-3.9)===-3&&Atomics.store(i16,1,'7')===7&&Atomics.store(i16,1,undefined)===0&&Atomics.store(i16,1,NaN)===0&&Object.is(Atomics.store(i16,1,-0),0);let inf=Atomics.store(i16,1,Infinity)===Infinity&&i16[1]===0&&Atomics.store(i16,1,-Infinity)===-Infinity;let u8=new Uint8Array(new ArrayBuffer(2));let plain=Atomics.store(u8,0,-5)===-5&&u8[0]===251&&Atomics.store(u8,0,300.7)===300&&u8[0]===44;wide&&ints&&inf&&plain",
        // BigInt: the ToBigInt result, not the 64-bit wrapped element.
        "let u64=new BigUint64Array(new SharedArrayBuffer(16));let wrapped=Atomics.store(u64,0,-5n)===-5n&&u64[0]===18446744073709551611n;let wide=Atomics.store(u64,1,2n**64n+3n)===2n**64n+3n&&u64[1]===3n;let i64=new BigInt64Array(new ArrayBuffer(8));wrapped&&wide&&Atomics.store(i64,0,2n**63n)===2n**63n&&i64[0]===-(2n**63n)",
    ] {
        assert_eq!(evaluate_with_host(source), Value::Bool(true), "{source}");
    }
}

#[test]
fn atomics_revalidate_the_view_after_index_and_value_coercion() {
    for source in [
        // Detaching the buffer while an index or operand is coerced must
        // throw a TypeError before any read or write, for every operation.
        "let ok=true;let names=['store','compareExchange','exchange','add','sub','and','or','xor'];for(let TA of [Int32Array,Int8Array,Uint16Array]){{let ta=new TA(1);let bad={valueOf(){$262.detachArrayBuffer(ta.buffer);return 0}};try{Atomics.load(ta,bad);ok=false}catch(e){ok=ok&&e instanceof TypeError}}for(let name of names){for(let position of name==='compareExchange'?[1,2,3]:[1,2]){let ta=new TA(1);let args=[ta,0,0,0];args[position]={valueOf(){$262.detachArrayBuffer(ta.buffer);return 0}};try{Atomics[name](...args);ok=false}catch(e){ok=ok&&e instanceof TypeError}}}}ok",
        // Shrinking a resizable buffer during coercion: a fixed-length view
        // is now out of bounds (TypeError), while a length-tracking view is
        // still valid unless the index fell off its end (RangeError).
        "let rab=new ArrayBuffer(4,{maxByteLength:8});let tracking=new Uint8Array(rab);let range=false;try{Atomics.store(tracking,3,{valueOf(){rab.resize(2);return 1}})}catch(e){range=e instanceof RangeError}let rab2=new ArrayBuffer(4,{maxByteLength:8});let fixed=new Uint8Array(rab2,0,4);let oob=false;try{Atomics.add(fixed,0,{valueOf(){rab2.resize(2);return 1}})}catch(e){oob=e instanceof TypeError}let rab3=new ArrayBuffer(4,{maxByteLength:8});let t3=new Uint8Array(rab3);let fine=Atomics.store(t3,1,{valueOf(){rab3.resize(2);return 9}})===9&&t3[1]===9;let rab4=new ArrayBuffer(4,{maxByteLength:8});let t4=new Uint8Array(rab4);let load=false;try{Atomics.load(t4,{valueOf(){rab4.resize(0);return 0}})}catch(e){load=e instanceof RangeError}range&&oob&&fine&&load",
    ] {
        assert_eq!(evaluate_with_host(source), Value::Bool(true), "{source}");
    }
}
