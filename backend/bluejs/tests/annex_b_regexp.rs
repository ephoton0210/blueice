// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Annex B RegExp behavior: `RegExp.prototype.compile` (B.2.4.1).

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

fn check(source: &str) {
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
}

#[test]
fn compile_replaces_source_and_flags_returns_the_receiver_and_resets_last_index() {
    check(
        "var re=/abc/gi;re.lastIndex=7;var r=re.compile('def','m');\
         r===re&&re.source==='def'&&re.flags==='m'&&re.lastIndex===0&&re.test('def')&&!re.test('DEF')",
    );
    check("var re=/abc/gi;re.compile();re.source==='(?:)'&&re.flags===''");
    check("var re=/abc/g;re.compile('a',undefined);re.flags===''");
}

#[test]
fn compile_copies_source_and_flags_from_a_regexp_without_observing_its_properties() {
    check(
        "var n=0;var src=/def/mig;\
         Object.defineProperties(src,{flags:{get:function(){n++}},source:{get:function(){n++}},global:{get:function(){n++}}});\
         var re=/abc/;re.lastIndex=3;re.compile(src);\
         n===0&&re.toString()===new RegExp('def','gim').toString()&&re.lastIndex===0",
    );
    check("var re=/abc/gim;re.compile(re);re.source==='abc'&&re.flags==='gim'");
    check(
        "var re=/./;re.lastIndex=23;var bad=[null,0,'',false,{},[]].every(function(f){\
         try{re.compile(re,f);return false}catch(e){return e instanceof TypeError}});\
         bad&&re.lastIndex===23",
    );
}

#[test]
fn compile_keeps_the_old_matcher_when_the_new_pattern_or_flags_are_invalid() {
    check(
        "var re=/abc/ig;var ok=[['?'],['.{2,1}'],['{','u'],['\\\\2','u'],['','igi'],['','gI'],['','w']].every(function(a){\
         try{re.compile(a[0],a[1]);return false}catch(e){return e instanceof SyntaxError}});\
         ok&&re.toString()==='/abc/gi'&&re.test('ABC')",
    );
    check(
        "var re=/./;re.lastIndex=99;var bad={toString:function(){throw new RangeError()}};\
         var a=false,b=false,c=false;\
         try{re.compile(bad)}catch(e){a=e instanceof RangeError}\
         try{re.compile('',bad)}catch(e){b=e instanceof RangeError}\
         try{re.compile(Symbol())}catch(e){c=e instanceof TypeError}\
         a&&b&&c&&re.lastIndex===99",
    );
}

#[test]
fn compile_installs_the_new_matcher_before_it_resets_a_read_only_last_index() {
    check(
        "var re=/initial/;Object.defineProperty(re,'lastIndex',{value:45,writable:false});\
         var threw=false;try{re.compile(/updated/gi)}catch(e){threw=e instanceof TypeError}\
         threw&&re.toString()==='/updated/gi'&&re.lastIndex===45",
    );
}

#[test]
fn compile_rejects_receivers_without_a_regexp_matcher_and_non_legacy_instances() {
    check(
        "var compile=RegExp.prototype.compile;\
         [undefined,null,23,true,'/x/',Symbol(),{},[],RegExp.prototype].every(function(v){\
         try{compile.call(v);return false}catch(e){return e instanceof TypeError}})",
    );
    check(
        "class Sub extends RegExp{}\
         var s=new Sub('');var threw=false;try{s.compile()}catch(e){threw=e instanceof TypeError}threw",
    );
    // A literal evaluated inside a constructor is an ordinary RegExp, whatever
    // the surrounding `new.target` is.
    check(
        "function F(){this.r=/a/}var f=new F();\
         Object.getPrototypeOf(f.r)===RegExp.prototype&&f.r.compile('b')===f.r&&f.r.source==='b'",
    );
    check(
        "function G(){return RegExp('a')}var r=new G();\
         Object.getPrototypeOf(r)===RegExp.prototype&&r.compile('b')===r",
    );
}

#[test]
fn compile_is_a_writable_configurable_method_of_length_two() {
    check(
        "var d=Object.getOwnPropertyDescriptor(RegExp.prototype,'compile');\
         d.writable&&!d.enumerable&&d.configurable&&RegExp.prototype.compile.length===2&&RegExp.prototype.compile.name==='compile'",
    );
}

#[test]
fn regexp_constructor_reads_the_source_after_observing_symbol_match() {
    // IsRegExp runs before [[OriginalSource]] is read, so a getter that
    // recompiles the pattern is visible in the copy.
    check(
        "var re=/a/;Object.defineProperty(re,Symbol.match,{get:function(){re.compile('b')}});\
         var parts=re[Symbol.split]('abba');parts.length===3&&parts[0]==='a'&&parts[1]===''&&parts[2]==='a'",
    );
    // ToUint32(limit) runs after the splitter was constructed.
    check(
        "var re=/a/;var limit={valueOf:function(){re.compile('b');return -1}};\
         var parts=re[Symbol.split]('abba',limit);parts.length===3&&parts[0]===''&&parts[1]==='bb'&&parts[2]===''",
    );
    // Symbol.match is read exactly once by each RegExp(re) / new RegExp(re).
    check(
        "var n=0;var re=/a/;Object.defineProperty(re,Symbol.match,{get:function(){n++;return true}});\
         var same=RegExp(re)===re;var copy=new RegExp(re);same&&copy!==re&&n===2",
    );
}

#[test]
fn flags_are_reported_in_canonical_order_after_compile() {
    check("var re=/(?:)/;re.compile('(?:)','imsuyg');re.flags==='gimsuy'");
}
