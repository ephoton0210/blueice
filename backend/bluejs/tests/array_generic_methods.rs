// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Array.prototype methods applied to array-likes: specification step order,
//! primitive receivers, clamped lengths and huge sparse receivers, through the
//! real parse/compile/execute path. Cases also run with a one-object nursery
//! so an unrooted value fails deterministically (`truthy_with_budget` marks
//! the exception).
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm, VmConfig};

fn run(
    source: &str,
    nursery_capacity: usize,
    instruction_budget: u64,
) -> Result<Value, RuntimeError> {
    let mut config = VmConfig::default();
    config.heap.nursery_capacity = nursery_capacity;
    config.instruction_budget = instruction_budget;
    Vm::new(config)
        .unwrap()
        .execute(&compile(&parse(source).unwrap()).unwrap())
}

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    run(
        source,
        VmConfig::default().heap.nursery_capacity,
        VmConfig::default().instruction_budget,
    )
}

/// One ordinary run under `budget`. Tens of thousands of Proxy trap calls are
/// too slow under GC stress, and those walks hold no value of their own.
fn truthy_with_budget(source: &str, budget: u64) {
    let nursery = VmConfig::default().heap.nursery_capacity;
    assert_eq!(
        run(source, nursery, budget),
        Ok(Value::Bool(true)),
        "{source}"
    );
}

/// The ordinary run, then the same source with a one-object nursery.
fn truthy(source: &str) {
    let budget = VmConfig::default().instruction_budget;
    truthy_with_budget(source, budget);
    assert_eq!(run(source, 1, budget), Ok(Value::Bool(true)), "{source}");
}

const THREW: &str = "function threw(f,E){try{f()}catch(e){return e instanceof E}return false}";

#[test]
fn length_is_read_before_the_callback_is_validated() {
    truthy(&format!(
        "{THREW}var log=[];var o={{get length(){{log.push('length');return 2}}}};\
         ['forEach','filter','map','every','some','reduce','reduceRight','find','findIndex',\
         'findLast','findLastIndex','flatMap'].every(function(name){{\
         log=[];return threw(function(){{Array.prototype[name].call(o,null)}},TypeError)\
         &&log.join()==='length'}})"
    ));
}

#[test]
fn a_throwing_length_wins_over_a_non_callable_callback() {
    truthy(&format!(
        "{THREW}function Boom(){{}}var o={{get length(){{throw new Boom()}}}};\
         ['forEach','filter','reduce','reduceRight'].every(function(name){{\
         return threw(function(){{Array.prototype[name].call(o,undefined)}},Boom)}})"
    ));
}

#[test]
fn length_conversion_runs_before_the_callback_check_and_indices_stay_unread() {
    truthy(&format!(
        "{THREW}var converted=0;var read=false;\
         var o={{get 0(){{read=true;return 1}},length:{{toString:function(){{converted++;return '2'}}}}}};\
         ['forEach','filter','reduce'].every(function(name){{\
         return threw(function(){{Array.prototype[name].call(o)}},TypeError)}})\
         &&converted===3&&!read"
    ));
}

#[test]
fn an_empty_receiver_returns_before_from_index_is_coerced() {
    truthy(
        "var calls=0;var from={valueOf:function(){calls++;throw 1}};\
         [].includes(0,from)===false&&[].indexOf(0,from)===-1&&[].lastIndexOf(0,from)===-1&&calls===0",
    );
}

#[test]
fn every_some_and_map_skip_the_holes_of_a_huge_sparse_array() {
    truthy(
        "var a=[0,1,true,null,{},'five'];a[999999]=-6.6;var calls=0;\
         var every=a.every(function(v,i,o){calls++;return arguments.length===3&&o[i]===v});\
         var some=a.some(function(v){calls++;return v===-6.6});\
         var mapped=a.map(function(v){return typeof v});\
         every&&some&&calls===14&&mapped.length===1000000&&mapped[999999]==='number'\
         &&!(500000 in mapped)&&mapped[5]==='string'",
    );
}

#[test]
fn every_some_and_map_stop_and_report_like_the_dense_algorithm() {
    truthy(
        "var a=[];a.length=1000000;a[10]=1;a[999999]=2;var seen=[];\
         var every=a.every(function(v,i){seen.push(i);return false});\
         var some=a.some(function(v,i){seen.push(i);return false});\
         every===false&&some===false&&seen.join()==='10,10,999999'",
    );
}

#[test]
fn for_each_filter_and_reduce_visit_only_present_indices_of_a_huge_sparse_array() {
    truthy(
        "var a=[];a.length=1000000;a[3]='a';a[500000]='b';a[999999]='c';var seen=[];\
         a.forEach(function(v,i,o){seen.push(i+v)});\
         var filtered=a.filter(function(v){return v!=='b'});\
         var joined=a.reduce(function(acc,v,i){return acc+v+i},'>');\
         var right=a.reduceRight(function(acc,v,i){return acc+v+i},'<');\
         var bare=a.reduce(function(acc,v){return acc+v});\
         seen.join()==='3a,500000b,999999c'&&filtered.join()==='a,c'\
         &&joined==='>a3b500000c999999'&&right==='<c999999b500000a3'&&bare==='abc'",
    );
}

#[test]
fn searches_visit_only_present_indices_of_a_huge_sparse_array() {
    truthy(
        "var a=new Array();a[100]=1;a[99999]='';a[10]={};a[5555]=5.5;a[123456]='str';a[5]=1e309;\
         a.lastIndexOf(1)===100&&a.lastIndexOf('')===99999&&a.lastIndexOf('str')===123456\
         &&a.lastIndexOf(5.5)===5555&&a.lastIndexOf(1e309)===5&&a.lastIndexOf(true)===-1\
         &&a.lastIndexOf(1,99)===-1&&a.lastIndexOf(1,-123357)===100&&a.lastIndexOf(1,-123358)===-1\
         &&a.indexOf('str')===123456&&a.indexOf(1,101)===-1&&a.includes(5.5)&&a.includes(undefined)\
         &&!a.includes(7)&&a.lastIndexOf(undefined)===-1",
    );
}

#[test]
fn an_element_added_during_iteration_is_visited_and_a_deleted_one_is_not() {
    truthy(
        "var a=[];a.length=1000000;a[0]=0;a[900000]=9;var seen=[];\
         a.forEach(function(v,i){seen.push(i);if(i===0){a[500000]=5}});\
         var b=[];b.length=1000000;b[0]=0;b[900000]=9;var kept=[];\
         b.forEach(function(v,i){kept.push(i);if(i===0){delete b[900000]}});\
         var c=[];c.length=1000000;c[0]=0;c[900000]=9;\
         var mapped=c.map(function(v,i){if(i===0){c[700000]=7}return v});\
         seen.join()==='0,500000,900000'&&kept.join()==='0'&&mapped[700000]===7",
    );
}

#[test]
fn a_getter_mutating_the_receiver_is_observed_by_the_scan() {
    truthy(
        "var a=[];a.length=1000000;var seen=[];\
         Object.defineProperty(a,'5',{get:function(){a[600000]='late';return 'five'},configurable:true});\
         a[900000]='end';\
         a.forEach(function(v,i){seen.push(i)});\
         seen.join()==='5,600000,900000'",
    );
}

#[test]
fn inherited_indices_and_prototype_swaps_are_visited() {
    truthy(
        "var proto=[];proto[700000]='p';var a=[];a.length=1000000;a[10]='own';Object.setPrototypeOf(a,proto);\
         var seen=[];a.forEach(function(v,i){seen.push(i+v)});\
         var swapped=[];swapped.length=1000000;swapped[0]=0;var visits=[];\
         swapped.forEach(function(v,i){visits.push(i);\
         if(i===0){var extra={};extra[800000]='x';Object.setPrototypeOf(swapped,extra)}});\
         seen.join()==='10own,700000p'&&visits.join()==='0,800000'",
    );
}

#[test]
fn a_proxy_prototype_still_observes_every_has_check() {
    truthy_with_budget(
        "var log=0;var a=[];a.length=70000;a[5]='x';\
         Object.setPrototypeOf(a,new Proxy({},{has:function(t,k){log++;return false}}));\
         var seen=[];Array.prototype.forEach.call(a,function(v,i){seen.push(i)});\
         seen.join()==='5'&&log===69999",
        50_000_000,
    );
}

#[test]
fn a_proxy_receiver_still_observes_every_has_check() {
    truthy_with_budget(
        "var log=[];var target=[];target.length=70000;target[7]='x';\
         var proxy=new Proxy(target,{has:function(t,k){log.push(k);return k in t}});\
         var seen=[];Array.prototype.forEach.call(proxy,function(v,i){seen.push(i)});\
         seen.join()==='7'&&log.length===70000",
        50_000_000,
    );
}

#[test]
fn array_likes_with_huge_lengths_visit_integer_keys_beyond_the_array_index_range() {
    truthy(
        "var o={length:2**53-1,5:'x'};o[9007199254740990]='y';o['01']='no';var seen=[];\
         Array.prototype.forEach.call(o,function(v,i){seen.push(i+v)});\
         var last=Array.prototype.lastIndexOf.call(o,'y');\
         var first=Array.prototype.indexOf.call(o,'y');\
         var found=Array.prototype.includes.call(o,undefined);\
         var some=Array.prototype.some.call(o,function(v){return v==='y'});\
         seen.join()==='5x,9007199254740990y'&&last===9007199254740990\
         &&first===9007199254740990&&found&&some",
    );
}

#[test]
fn includes_reads_holes_as_undefined_without_visiting_them() {
    truthy(
        "var a=[];a.length=1000000;a[999999]=1;\
         var early=a.includes(undefined)&&a.includes(1)&&!a.includes(NaN);\
         a[500000]=NaN;\
         early&&a.includes(NaN)&&!a.includes(undefined,999999)&&!a.includes(undefined,1000000)\
         &&a.includes(undefined,-1000000)&&a.includes(1,-1)&&!a.includes(1,1000000)",
    );
}

#[test]
fn reduce_of_a_receiver_with_no_elements_and_no_initial_value_throws() {
    truthy(&format!(
        "{THREW}var a=[];a.length=1000000;var f=function(){{}};\
         threw(function(){{a.reduce(f)}},TypeError)&&threw(function(){{a.reduceRight(f)}},TypeError)\
         &&a.reduce(f,7)===7&&a.reduceRight(f,8)===8"
    ));
}

#[test]
fn values_only_the_walk_holds_survive_a_callback_that_deletes_them() {
    truthy(
        "var a=[];a.length=100000;a[5]={tag:1};a[7]={tag:2};\
         var kept=a.filter(function(v,i,o){delete o[i];return true});\
         var b=[];b.length=100000;b[3]={n:1};b[4]={n:2};b[9]={n:3};\
         Object.defineProperty(b,'6',{get:function(){return {n:10}},configurable:true});\
         var total=b.reduce(function(acc,v){return {n:acc.n+v.n}},{n:0});\
         kept.length===2&&kept[0].tag===1&&kept[1].tag===2&&total.n===16",
    );
}

#[test]
fn a_walk_whose_receiver_keeps_changing_falls_back_to_visiting_every_index() {
    truthy(
        "var a=[];a.length=70000;a[0]=0;var visited=0;\
         a.forEach(function(v,i,o){visited++;if(i<1000){o[i+1]=i+1}if(i===1000){o[65000]='late'}});\
         visited===1002",
    );
}

#[test]
fn a_dense_scan_of_a_huge_length_reports_the_instruction_budget_not_a_hang() {
    assert_eq!(
        evaluate(
            "var log=0;var o=new Proxy({length:2**53-1},{has:function(t,k){log++;return false}});\
             Array.prototype.forEach.call(o,function(){})",
        ),
        Err(RuntimeError::InstructionLimit)
    );
}
