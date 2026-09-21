// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ES2023 change-array-by-copy methods (`toReversed`, `toSorted`,
//! `toSpliced`, `with`) and `Array.prototype[Symbol.unscopables]`, driven
//! through the real parse/compile/execute path.
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

/// A one-object nursery collects on nearly every allocation, so any value a
/// method leaves unrooted fails deterministically.
fn evaluate_gc_stress(source: &str) -> Result<Value, RuntimeError> {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = 1;
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
}

fn truthy(source: &str) {
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
}

fn throws(source: &str, error: &str) {
    let wrapped = format!(
        "var caught='none';try{{{source}}}catch(e){{caught=e instanceof {error}?'{error}':'other:'+e}}\
         caught==='{error}'"
    );
    truthy(&wrapped);
}

#[test]
fn to_reversed_returns_a_new_reversed_array_and_reads_holes_as_undefined() {
    truthy(
        "var a=[1,2,3];var r=a.toReversed();\
         r!==a&&r.join()==='3,2,1'&&a.join()==='1,2,3'&&Array.isArray(r)",
    );
    truthy(
        "var r=[1,,3].toReversed();r.length===3&&r.hasOwnProperty(1)&&r[1]===undefined&&r[0]===3",
    );
    truthy(
        "var r=Array.prototype.toReversed.call({length:2,0:'a',1:'b'});\
         Array.isArray(r)&&r.join()==='b,a'",
    );
}

#[test]
fn change_by_copy_methods_ignore_species_and_use_the_intrinsic_array() {
    truthy(
        "class Sub extends Array{static get [Symbol.species](){throw new Error('species read')}}\
         var s=Sub.from([3,1,2]);\
         var results=[s.toReversed(),s.toSorted(),s.toSpliced(0,1),s.with(0,9)];\
         results.every(r=>Object.getPrototypeOf(r)===Array.prototype)",
    );
}

#[test]
fn to_reversed_rejects_a_length_beyond_the_array_maximum() {
    throws(
        "Array.prototype.toReversed.call({length:4294967296})",
        "RangeError",
    );
}

#[test]
fn to_sorted_sorts_a_copy_with_default_and_custom_orders() {
    truthy("var a=[10,9,1];var s=a.toSorted();s!==a&&s.join()==='1,10,9'&&a.join()==='10,9,1'");
    truthy("[10,9,1].toSorted((x,y)=>x-y).join()==='1,9,10'");
    // Equal keys keep their original relative order.
    truthy(
        "var items=[{k:1,n:'a'},{k:0,n:'b'},{k:1,n:'c'},{k:0,n:'d'}];\
         items.toSorted((x,y)=>x.k-y.k).map(i=>i.n).join('')==='bdac'",
    );
}

#[test]
fn to_sorted_reads_holes_as_undefined_and_places_undefined_last() {
    truthy(
        "var s=[3,,1,undefined].toSorted();\
         s.length===4&&s[0]===1&&s[1]===3&&s.hasOwnProperty(2)&&s[2]===undefined\
         &&s.hasOwnProperty(3)&&s[3]===undefined",
    );
}

#[test]
fn to_sorted_validates_the_comparator_before_touching_the_receiver() {
    throws("[].toSorted(null)", "TypeError");
    throws("[].toSorted({})", "TypeError");
    throws(
        "var touched=false;\
         var o={get length(){touched=true;return 0}};\
         try{Array.prototype.toSorted.call(o,1)}finally{if(touched)throw new Error('read')}",
        "TypeError",
    );
    truthy("[2,1].toSorted(undefined).join()==='1,2'");
}

#[test]
fn to_sorted_propagates_a_throwing_comparator_and_leaves_the_source_alone() {
    truthy(
        "var a=[3,2,1];var threw=false;\
         try{a.toSorted(()=>{throw new RangeError('x')})}catch(e){threw=e instanceof RangeError}\
         threw&&a.join()==='3,2,1'",
    );
}

#[test]
fn to_spliced_removes_and_inserts_into_a_copy() {
    truthy(
        "var a=[1,2,3,4];var s=a.toSpliced(1,2,'x','y','z');\
         s!==a&&s.join()==='1,x,y,z,4'&&a.join()==='1,2,3,4'",
    );
    truthy("[1,2,3].toSpliced().join()==='1,2,3'");
    truthy("[1,2,3].toSpliced(1).join()==='1'");
    truthy("[1,2,3].toSpliced(-1,1).join()==='1,2'");
    truthy("[1,2,3].toSpliced(1,undefined).join()==='1,2,3'");
    truthy("[1,2,3].toSpliced(1,-5,'a').join()==='1,a,2,3'");
    truthy("[1,2,3].toSpliced(10,1,'a').join()==='1,2,3,a'");
    truthy("[1,,3].toSpliced(0,0).hasOwnProperty(1)");
}

#[test]
fn to_spliced_rejects_a_result_longer_than_the_array_maximum() {
    throws(
        "Array.prototype.toSpliced.call({length:4294967295},0,0,1)",
        "RangeError",
    );
    throws(
        "Array.prototype.toSpliced.call({length:9007199254740991},0,0,1)",
        "TypeError",
    );
}

#[test]
fn with_replaces_one_index_in_a_copy() {
    truthy("var a=[1,2,3];var w=a.with(1,'x');w!==a&&w.join()==='1,x,3'&&a.join()==='1,2,3'");
    truthy("[1,2,3].with(-1,'x').join()==='1,2,x'");
    truthy("[1,,3].with(0,9).hasOwnProperty(1)");
    truthy("[1,2,3].with(1.9,'x').join()==='1,x,3'");
}

#[test]
fn with_throws_a_range_error_for_an_out_of_range_index() {
    throws("[1,2,3].with(3,0)", "RangeError");
    throws("[1,2,3].with(-4,0)", "RangeError");
    throws("[].with(0,0)", "RangeError");
    throws("[1].with(Infinity,0)", "RangeError");
    throws("[1].with(-Infinity,0)", "RangeError");
}

#[test]
fn new_methods_expose_standard_function_metadata() {
    truthy(
        "[['toReversed',0],['toSorted',1],['toSpliced',2],['with',2]].every(([n,l])=>{\
           var d=Object.getOwnPropertyDescriptor(Array.prototype,n);\
           return d.writable&&!d.enumerable&&d.configurable\
             &&d.value.name===n&&d.value.length===l\
             &&!('prototype' in d.value)});",
    );
    throws("new Array.prototype.toSorted()", "TypeError");
}

#[test]
fn new_methods_work_when_they_are_the_first_property_read() {
    // Intrinsics bootstrap lazily; a fresh VM must expose them immediately.
    truthy("typeof [].toSorted==='function'&&typeof [].with==='function'");
}

#[test]
fn change_by_copy_methods_survive_collection_on_every_allocation() {
    for source in [
        "var a=Array.from({length:200},(_,i)=>({i}));\
         var r=a.toReversed();r.length===200&&r[0].i===199&&r[199].i===0",
        "var a=Array.from({length:200},(_,i)=>({i:199-i}));\
         var s=a.toSorted((x,y)=>x.i-y.i);s[0].i===0&&s[199].i===199",
        "var a=Array.from({length:200},(_,i)=>({i}));\
         var s=a.toSpliced(10,5,{i:'n'});s.length===196&&s[10].i==='n'&&s[11].i===15",
        "var a=Array.from({length:200},(_,i)=>({i}));\
         var w=a.with(100,{i:'w'});w[100].i==='w'&&w[0].i===0&&w[199].i===199",
    ] {
        assert_eq!(
            evaluate_gc_stress(source).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }
}

#[test]
fn array_prototype_unscopables_is_a_null_prototype_record_of_the_new_names() {
    truthy(
        "var u=Array.prototype[Symbol.unscopables];\
         Object.getPrototypeOf(u)===null\
         &&Object.keys(u).join()==='at,copyWithin,entries,fill,find,findIndex,findLast,\
findLastIndex,flat,flatMap,includes,keys,toReversed,toSorted,toSpliced,values'\
         &&Object.keys(u).every(k=>u[k]===true)",
    );
    truthy(
        "var d=Object.getOwnPropertyDescriptor(Array.prototype,Symbol.unscopables);\
         d.writable===false&&d.enumerable===false&&d.configurable===true",
    );
    truthy(
        "var u=Array.prototype[Symbol.unscopables];\
         Object.keys(u).every(k=>{var p=Object.getOwnPropertyDescriptor(u,k);\
           return p.writable&&p.enumerable&&p.configurable})",
    );
}

#[test]
fn with_statement_skips_unscopable_array_names() {
    truthy("var keys='outer';var out;with([]){out=keys}out==='outer'");
    truthy("var toSorted='outer';var out;with([1]){out=toSorted}out==='outer'");
    // A name that is not unscopable still resolves on the array.
    truthy("var out;with([1,2]){out=push}out===Array.prototype.push");
}
