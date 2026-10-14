// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Uint8Array Base64/Hex proposal: every decode/encode branch, option
//! validation and receiver check, driven through JavaScript source.

mod cov_g7_common;
use cov_g7_common::{failures, sweep_fuel_each, sweep_heap_each, with_setup};

#[test]
fn from_base64_decodes_full_groups_padding_and_whitespace() {
    assert_eq!(
        failures(
            "eq('empty',Uint8Array.fromBase64('').length,0);\
             eq('group',Uint8Array.fromBase64('TWFu').join(),'77,97,110');\
             eq('pad1',Uint8Array.fromBase64('TWE=').join(),'77,97');\
             eq('pad2',Uint8Array.fromBase64('TQ==').join(),'77');\
             eq('ws',Uint8Array.fromBase64(' T W\\nF\\tu\\r\\f ').join(),'77,97,110');\
             eq('two groups',Uint8Array.fromBase64('TWFuTWE=').join(),'77,97,110,77,97');\
             eq('loose2',Uint8Array.fromBase64('TWE').join(),'77,97');\
             eq('loose1',Uint8Array.fromBase64('TQ').join(),'77');\
             eq('loose nonzero bits',Uint8Array.fromBase64('TR==').join(),'77');\
             eq('std plus slash',Uint8Array.fromBase64('+/8=').join(),'251,255');\
             eq('url dash underscore',Uint8Array.fromBase64('-_8=',{alphabet:'base64url'}).join(),'251,255');\
             eq('alphabet base64',Uint8Array.fromBase64('+/8=',{alphabet:'base64'}).join(),'251,255');\
             eq('alphabet undefined',Uint8Array.fromBase64('+/8=',{alphabet:undefined}).join(),'251,255');\
             eq('digits',Uint8Array.fromBase64('0123').join(),'211,93,183');"
        ),
        ""
    );
}

#[test]
fn from_base64_rejects_malformed_input() {
    assert_eq!(
        failures(
            "th('bad first',function(){Uint8Array.fromBase64('!AAA')},SyntaxError);\
             th('bad second',function(){Uint8Array.fromBase64('A!AA')},SyntaxError);\
             th('bad third',function(){Uint8Array.fromBase64('AA!A')},SyntaxError);\
             th('bad fourth',function(){Uint8Array.fromBase64('AAA!')},SyntaxError);\
             th('bad third before pad',function(){Uint8Array.fromBase64('AA!=')},SyntaxError);\
             th('equals then char',function(){Uint8Array.fromBase64('AA=A')},SyntaxError);\
             th('std rejects url',function(){Uint8Array.fromBase64('-_8=')},SyntaxError);\
             th('url rejects std',function(){Uint8Array.fromBase64('+/8=',{alphabet:'base64url'})},SyntaxError);\
             th('padding mid stream',function(){Uint8Array.fromBase64('TQ==TWFu')},SyntaxError);\
             th('lone char',function(){Uint8Array.fromBase64('T')},SyntaxError);\
             th('early padding',function(){Uint8Array.fromBase64('T=')},SyntaxError);\
             th('early padding 3',function(){Uint8Array.fromBase64('T==')},SyntaxError);\
             th('incomplete padding',function(){Uint8Array.fromBase64('TQ=')},SyntaxError);\
             th('partial bad char',function(){Uint8Array.fromBase64('TW!')},SyntaxError);\
             th('bad after group',function(){Uint8Array.fromBase64('TWFu!!!!')},SyntaxError);\
             th('trailing garbage',function(){Uint8Array.fromBase64('TQ==x')},SyntaxError);"
        ),
        ""
    );
}

#[test]
fn from_base64_last_chunk_handling_modes() {
    assert_eq!(
        failures(
            "var strict={lastChunkHandling:'strict'};\
             var stop={lastChunkHandling:'stop-before-partial'};\
             var loose={lastChunkHandling:'loose'};\
             eq('strict padded 2',Uint8Array.fromBase64('TQ==',strict).join(),'77');\
             eq('strict padded 3',Uint8Array.fromBase64('TWE=',strict).join(),'77,97');\
             eq('strict full',Uint8Array.fromBase64('TWFu',strict).join(),'77,97,110');\
             th('strict nonzero 2',function(){Uint8Array.fromBase64('TR==',strict)},SyntaxError);\
             th('strict nonzero 3',function(){Uint8Array.fromBase64('TWF=',strict)},SyntaxError);\
             th('strict partial',function(){Uint8Array.fromBase64('TWE',strict)},SyntaxError);\
             eq('loose',Uint8Array.fromBase64('TWE',loose).join(),'77,97');\
             eq('stop drops partial 2',Uint8Array.fromBase64('TWFuTQ',stop).join(),'77,97,110');\
             eq('stop drops partial 1',Uint8Array.fromBase64('TWFuT',stop).join(),'77,97,110');\
             eq('stop drops padded partial',Uint8Array.fromBase64('TWFuTQ=',stop).join(),'77,97,110');\
             eq('stop keeps padded full',Uint8Array.fromBase64('TWFuTQ==',stop).join(),'77,97,110,77');\
             th('stop bad char',function(){Uint8Array.fromBase64('TWFuT!',stop)},SyntaxError);\
             th('stop early padding',function(){Uint8Array.fromBase64('TWFuT=',stop)},SyntaxError);\
             th('bad handling',function(){Uint8Array.fromBase64('',{lastChunkHandling:'nope'})},TypeError);\
             th('bad handling type',function(){Uint8Array.fromBase64('',{lastChunkHandling:1})},TypeError);\
             th('bad alphabet',function(){Uint8Array.fromBase64('',{alphabet:'nope'})},TypeError);\
             th('bad alphabet type',function(){Uint8Array.fromBase64('',{alphabet:null})},TypeError);\
             th('null options',function(){Uint8Array.fromBase64('',null)},TypeError);"
        ),
        ""
    );
}

#[test]
fn from_base64_and_from_hex_reject_non_strings_and_construction() {
    assert_eq!(
        failures(
            "th('b64 number',function(){Uint8Array.fromBase64(1)},TypeError);\
             th('b64 object',function(){Uint8Array.fromBase64({})},TypeError);\
             th('hex number',function(){Uint8Array.fromHex(1)},TypeError);\
             th('b64 new',function(){new Uint8Array.fromBase64('')},TypeError);\
             th('hex new',function(){new Uint8Array.fromHex('')},TypeError);\
             th('set b64 number',function(){new Uint8Array(1).setFromBase64(1)},TypeError);\
             th('set hex number',function(){new Uint8Array(1).setFromHex(1)},TypeError);\
             var t=new Uint8Array(1);\
             th('to hex new',function(){new t.toHex()},TypeError);\
             th('to b64 new',function(){new t.toBase64()},TypeError);\
             th('set b64 new',function(){new t.setFromBase64('')},TypeError);\
             th('set hex new',function(){new t.setFromHex('')},TypeError);"
        ),
        ""
    );
}

#[test]
fn set_from_base64_reports_progress_and_partial_writes() {
    assert_eq!(
        failures(
            "var t=new Uint8Array(4);var r=t.setFromBase64('TWFu');\
             eq('full read',r.read,4);eq('full written',r.written,3);eq('full bytes',t.join(),'77,97,110,0');\
             var d=Object.getOwnPropertyDescriptor(r,'read');\
             eq('record props',Object.keys(r).join(),'read,written');\
             eq('record writable',d.writable&&d.enumerable&&d.configurable,true);\
             r=new Uint8Array(0).setFromBase64('TWFu');eq('empty target read',r.read,0);eq('empty target written',r.written,0);\
             r=new Uint8Array(2).setFromBase64('TWFu');eq('too small read',r.read,0);eq('too small written',r.written,0);\
             t=new Uint8Array(3);r=t.setFromBase64('TWFuTWFu');\
             eq('exact fit read',r.read,4);eq('exact fit written',r.written,3);eq('exact fit bytes',t.join(),'77,97,110');\
             t=new Uint8Array(1);r=t.setFromBase64('TQ==TWFu');\
             eq('pad fills read',r.read,4);eq('pad fills written',r.written,1);\
             t=new Uint8Array(10);r=t.setFromBase64('TQ==');\
             eq('final pad read',r.read,4);eq('final pad written',r.written,1);\
             t=new Uint8Array(1);r=t.setFromBase64('TWE');\
             eq('partial too big read',r.read,0);eq('partial too big written',r.written,0);\
             t=new Uint8Array(2);r=t.setFromBase64('TWE');\
             eq('partial read',r.read,3);eq('partial written',r.written,2);eq('partial bytes',t.join(),'77,97');\
             t=new Uint8Array(3);r=t.setFromBase64('TQ');\
             eq('partial1 read',r.read,2);eq('partial1 written',r.written,1);\
             t=new Uint8Array(3);r=t.setFromBase64('TWFu  ');\
             eq('trailing whitespace read',r.read,4);\
             t=new Uint8Array(3);r=t.setFromBase64('TWFuTQ',{lastChunkHandling:'stop-before-partial'});\
             eq('stop read',r.read,4);eq('stop written',r.written,3);\
             t=new Uint8Array(8);\
             th('prefix then error',function(){t.setFromBase64('TWFu!!!!')},SyntaxError);\
             eq('prefix kept',t.join(),'77,97,110,0,0,0,0,0');\
             t=new Uint8Array(8);\
             th('pad mid stream',function(){t.setFromBase64('TWFuTQ==TWFu')},SyntaxError);\
             eq('pad mid stream prefix',t.join(),'77,97,110,0,0,0,0,0');"
        ),
        ""
    );
}

#[test]
fn hex_decoding_and_progress() {
    assert_eq!(
        failures(
            "eq('lower',Uint8Array.fromHex('00ff7a').join(),'0,255,122');\
             eq('mixed',Uint8Array.fromHex('DEADbeef').join(),'222,173,190,239');\
             eq('empty',Uint8Array.fromHex('').length,0);\
             th('odd',function(){Uint8Array.fromHex('abc')},SyntaxError);\
             th('bad high',function(){Uint8Array.fromHex('zz')},SyntaxError);\
             th('bad low',function(){Uint8Array.fromHex('0z')},SyntaxError);\
             var t=new Uint8Array(4);var r=t.setFromHex('01ff');\
             eq('read',r.read,4);eq('written',r.written,2);eq('bytes',t.join(),'1,255,0,0');\
             r=new Uint8Array(0).setFromHex('0102');eq('empty read',r.read,0);eq('empty written',r.written,0);\
             th('empty target odd',function(){new Uint8Array(0).setFromHex('abc')},SyntaxError);\
             t=new Uint8Array(1);r=t.setFromHex('0102');\
             eq('short read',r.read,2);eq('short written',r.written,1);eq('short bytes',t.join(),'1');\
             t=new Uint8Array(4);\
             th('partial then bad',function(){t.setFromHex('a0zz')},SyntaxError);\
             eq('partial hex kept',t.join(),'160,0,0,0');\
             t=new Uint8Array(4);\
             th('partial then bad low',function(){t.setFromHex('a00z')},SyntaxError);\
             eq('partial hex low kept',t.join(),'160,0,0,0');\
             eq('to hex',new Uint8Array([0,15,255,171]).toHex(),'000fffab');\
             eq('to hex empty',new Uint8Array(0).toHex(),'');"
        ),
        ""
    );
}

#[test]
fn to_base64_encodes_every_remainder_alphabet_and_padding_mode() {
    assert_eq!(
        failures(
            "eq('empty',new Uint8Array(0).toBase64(),'');\
             eq('one',new Uint8Array([77]).toBase64(),'TQ==');\
             eq('two',new Uint8Array([77,97]).toBase64(),'TWE=');\
             eq('three',new Uint8Array([77,97,110]).toBase64(),'TWFu');\
             eq('four',new Uint8Array([77,97,110,77]).toBase64(),'TWFuTQ==');\
             eq('five',new Uint8Array([77,97,110,77,97]).toBase64(),'TWFuTWE=');\
             eq('std',new Uint8Array([251,255]).toBase64(),'+/8=');\
             eq('url',new Uint8Array([251,255]).toBase64({alphabet:'base64url'}),'-_8=');\
             eq('url one',new Uint8Array([251]).toBase64({alphabet:'base64url'}),'-w==');\
             eq('url three',new Uint8Array([251,255,254]).toBase64({alphabet:'base64url'}),'-__-');\
             eq('alphabet base64',new Uint8Array([251,255]).toBase64({alphabet:'base64'}),'+/8=');\
             eq('alphabet undefined',new Uint8Array([251,255]).toBase64({alphabet:undefined}),'+/8=');\
             eq('omit two',new Uint8Array([77,97]).toBase64({omitPadding:true}),'TWE');\
             eq('omit one',new Uint8Array([77]).toBase64({omitPadding:true}),'TQ');\
             eq('omit false',new Uint8Array([77]).toBase64({omitPadding:false}),'TQ==');\
             eq('empty options',new Uint8Array([77]).toBase64({}),'TQ==');\
             th('bad alphabet',function(){new Uint8Array(1).toBase64({alphabet:'x'})},TypeError);\
             th('bad alphabet type',function(){new Uint8Array(1).toBase64({alphabet:1})},TypeError);"
        ),
        ""
    );
}

#[test]
fn option_getters_that_throw_propagate_their_error() {
    assert_eq!(
        failures(
            "var boom={};\
             function thrower(name){var o={};Object.defineProperty(o,name,{get:function(){throw boom}});return o}\
             function caught(f){try{f();return false}catch(e){return e===boom}}\
             eq('from alphabet',caught(function(){Uint8Array.fromBase64('',thrower('alphabet'))}),true);\
             eq('from handling',caught(function(){Uint8Array.fromBase64('',thrower('lastChunkHandling'))}),true);\
             eq('set alphabet',caught(function(){new Uint8Array(1).setFromBase64('',thrower('alphabet'))}),true);\
             eq('to alphabet',caught(function(){new Uint8Array(1).toBase64(thrower('alphabet'))}),true);\
             eq('to omit',caught(function(){new Uint8Array(1).toBase64(thrower('omitPadding'))}),true);"
        ),
        ""
    );
}

#[test]
fn receiver_validation_covers_brand_detachment_and_immutability() {
    assert_eq!(
        failures(
            "var P=Uint8Array.prototype;\
             var others=[1,{},new Int8Array(1),new Uint8ClampedArray(1),new ArrayBuffer(1),undefined];\
             for(var i=0;i<others.length;i++){\
               var o=others[i];\
               th('toHex '+i,function(){P.toHex.call(o)},TypeError);\
               th('toBase64 '+i,function(){P.toBase64.call(o)},TypeError);\
               th('setFromHex '+i,function(){P.setFromHex.call(o,'')},TypeError);\
               th('setFromBase64 '+i,function(){P.setFromBase64.call(o,'')},TypeError);\
             }\
             var buf=new ArrayBuffer(4);var t=new Uint8Array(buf);$262.detachArrayBuffer(buf);\
             th('detached toHex',function(){t.toHex()},TypeError);\
             th('detached toBase64',function(){t.toBase64()},TypeError);\
             th('detached setFromHex',function(){t.setFromHex('')},TypeError);\
             th('detached setFromBase64',function(){t.setFromBase64('')},TypeError);\
             var detachOnRead=new Uint8Array(4);\
             var options={get alphabet(){$262.detachArrayBuffer(detachOnRead.buffer);return 'base64'}};\
             th('detached by option',function(){detachOnRead.setFromBase64('',options)},TypeError);\
             var imm=new Uint8Array(new ArrayBuffer(4).sliceToImmutable());\
             th('immutable setFromHex',function(){imm.setFromHex('00')},TypeError);\
             th('immutable setFromBase64',function(){imm.setFromBase64('AA==')},TypeError);\
             eq('immutable toHex',imm.toHex(),'00000000');\
             eq('immutable toBase64',imm.toBase64(),'AAAAAA==');\
             var sub=new Uint8Array([1,2,3,4,5]).subarray(1,4);\
             eq('subarray toHex',sub.toHex(),'020304');\
             eq('subarray toBase64',sub.toBase64(),'AgME');\
             var rab=new ArrayBuffer(4,{maxByteLength:8});var tracking=new Uint8Array(rab);\
             tracking.setFromHex('01020304');rab.resize(2);\
             eq('tracking after shrink',tracking.toHex(),'0102');"
        ),
        ""
    );
}

const SETUP: &str = "var t = new Uint8Array(8);";

const BODIES: &[&str] = &[
    "t.setFromBase64('TWFuTWE=');",
    "t.setFromHex('0aff');",
    "Uint8Array.fromBase64('TWFu', { alphabet: 'base64url', lastChunkHandling: 'strict' });",
    "t.toBase64({ alphabet: 'base64url', omitPadding: true });",
    "Uint8Array.fromHex('0aff').toHex();",
    "try { t.setFromBase64('TWFu!!!!') } catch (e) {}",
];

#[test]
fn every_heap_allocation_failure_is_reported_as_the_heap_limit() {
    sweep_heap_each(&with_setup(SETUP, BODIES));
}

#[test]
fn every_instruction_budget_exhaustion_is_reported_as_the_instruction_limit() {
    sweep_fuel_each(&with_setup(SETUP, BODIES));
}
