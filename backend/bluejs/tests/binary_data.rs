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
            "let buffer=new ArrayBuffer(4,{maxByteLength:8});let tracking=new Uint8Array(buffer);let fixed=new Uint8Array(buffer,0,4);let view=new DataView(buffer);tracking[3]=7;buffer.resize(6);let grown=buffer.resizable&&buffer.maxByteLength===8&&buffer.byteLength===6&&tracking.length===6&&tracking[3]===7&&tracking[5]===0&&fixed.length===4&&view.byteLength===6;buffer.resize(2);let rejected=false;try{fixed.set([1])}catch(error){rejected=error instanceof TypeError}grown&&buffer.byteLength===2&&tracking.length===2&&view.byteLength===2&&fixed.length===0&&fixed.byteLength===0&&fixed.byteOffset===0&&fixed[0]===undefined&&rejected",
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
    ] {
        let actual = evaluate(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(actual, expected, "{source}");
    }
}
