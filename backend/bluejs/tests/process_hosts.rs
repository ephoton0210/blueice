// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn adapter(requests: &[Value], fault: Option<(&str, &std::path::Path)>) -> Vec<Value> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bluejs-test262"));
    if let Some((mode, path)) = fault {
        command
            .env("BLUEJS_REGEXP_WORKER", path)
            .env("BLUEJS_FAULT_MODE", mode);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for request in requests {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut lines = std::str::from_utf8(&result.stdout).unwrap().lines();
    assert_eq!(lines.next(), Some("{\"ready\":1}"));
    lines
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn adapter_preserves_phases_limits_and_fresh_realms() {
    let cases = [
        (json!({"source":"globalThis.x=42", "mode":"sloppy"}), "ok"),
        (
            json!({"source":"assert.sameValue(typeof x,'undefined')", "mode":"strict"}),
            "ok",
        ),
        (
            json!({"source":"assert(true)", "mode":"raw"}),
            "ReferenceError",
        ),
        (
            json!({"source":"$262.createRealm()", "mode":"raw"}),
            "unsupported",
        ),
        (json!({"source":"1", "mode":"module"}), "ok"),
        (
            json!({"source":"1", "mode":"sloppy", "asynchronous":true}),
            "timeout",
        ),
        (
            json!({"source":"let x;let x;", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"let =", "mode":"sloppy", "parse_only":true}),
            "unclassified_parse_error",
        ),
        // The adapter must distinguish early errors it can establish from a
        // valid production whose execution is not implemented yet.  These
        // assignment-target negatives exercise grammar that used to stop at
        // an arbitrary subset-parser error.
        (
            json!({"source":"([...rest, value] = input)", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"(x ??= y) = 1", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"x?.y = 1", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"x ** y = 1", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"({ break } = input)", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"\"use strict\"; ({ implements } = input)", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"x ??= y", "mode":"sloppy"}),
            "ReferenceError",
        ),
        (json!({"source":"x?.y", "mode":"sloppy"}), "unsupported"),
        (
            json!({"source":"x ** y", "mode":"sloppy"}),
            "ReferenceError",
        ),
        (
            json!({"source":"switch() {}", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"try{}catch([...rest, next]){}", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"try{}catch({...rest, next}){}", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"try{}catch(eval){}", "mode":"strict", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"try{}", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"catch", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"try{}finally(){}", "mode":"sloppy", "parse_only":true}),
            "SyntaxError",
        ),
        (
            json!({"source":"1", "mode":"sloppy", "parse_only":true}),
            "ok",
        ),
        (json!({"source":"try{}catch(e){}", "mode":"sloppy"}), "ok"),
        (
            json!({"source":"1", "mode":"sloppy", "bytecode_limit":0}),
            "resource_error",
        ),
        (
            json!({"source":"1", "mode":"sloppy", "includes":["assert.js","other.js"]}),
            "unsupported",
        ),
        (
            json!({"source":"verifyProperty(Math,'PI',{value:Math.PI,writable:false,enumerable:false,configurable:false});isConstructor(function(){})", "mode":"sloppy", "includes":["propertyHelper.js","isConstructor.js"]}),
            "ok",
        ),
        (
            json!({"source":"addOffset(2) === 5", "mode":"sloppy", "includes":["helpers.js"], "harness_sources":["var offset=3; function addOffset(value){return value+offset}"]}),
            "ok",
        ),
        (
            json!({"source":"1", "mode":"strict", "includes":["helpers.js"], "harness_sources":["let ="]}),
            "unsupported",
        ),
        (
            json!({"source":"1", "mode":"sloppy", "includes":["helpers.js"], "harness_sources":["try{}catch(e){}"]}),
            "ok",
        ),
        (
            json!({"source":"1", "mode":"sloppy", "includes":["helpers.js"], "harness_sources":["throw 1"]}),
            "ThrownValue",
        ),
        (
            json!({"source":"1", "mode":"sloppy", "heap_limit":1000}),
            "harness_error",
        ),
        (
            json!({"source":"1", "mode":"sloppy", "heap_limit":0}),
            "harness_error",
        ),
        (json!({"source":"null.x", "mode":"sloppy"}), "TypeError"),
        (
            json!({"source":"'a'.repeat(-1)", "mode":"sloppy"}),
            "RangeError",
        ),
        (
            json!({"source":"new RegExp('[')", "mode":"sloppy"}),
            "SyntaxError",
        ),
        (
            json!({"source":"assert(false)", "mode":"sloppy"}),
            "Test262Error",
        ),
        (json!({"source":"throw 1", "mode":"sloppy"}), "ThrownValue"),
        (
            json!({"source":"1", "mode":"sloppy", "instruction_budget":0}),
            "timeout",
        ),
        (json!({"source":"for(;;){}", "mode":"sloppy"}), "timeout"),
        (
            json!({"source":"'abc'.repeat(20)", "mode":"sloppy", "string_limit":100}),
            "resource_error",
        ),
        (
            json!({"source":"new RegExp('(a+)+$').test('a'.repeat(40)+'!')", "mode":"sloppy", "regex_timeout_ms":40}),
            "timeout",
        ),
        (json!(null), "harness_error"),
        (
            json!({"source":"1", "mode":"raw", "includes":["other.js"]}),
            "ok",
        ),
        (
            json!({"source":"with({}){var f=function(){return 3;};}assert.sameValue(f(),3)", "mode":"sloppy"}),
            "ok",
        ),
        (
            json!({"source":"function f(a=b,b=2){}f()", "mode":"sloppy"}),
            "ReferenceError",
        ),
        (
            json!({"source":"function f(a=1,get=()=>a){var a=2;return get();}assert.sameValue(f(),1)", "mode":"sloppy"}),
            "ok",
        ),
        (
            json!({"source":"class Base{constructor(value){this.value=value}method(){return this.value}}class C extends Base{constructor(){super(3)}method(){return super.method()}static field=1;static{this.block=2}}let value=new C;assert.sameValue(value.method(),3);assert.sameValue(C.field,1);assert.sameValue(C.block,2)", "mode":"sloppy"}),
            "ok",
        ),
        (
            json!({"source":"class Base{set value(value){this.saved=value}}class C extends Base{constructor(){super()}store(){super.value=2;return this.saved}}assert.sameValue(new C().store(),2)", "mode":"sloppy"}),
            "ok",
        ),
        (
            json!({"source":"class C extends null{constructor(){super()}}new C", "mode":"sloppy"}),
            "TypeError",
        ),
        (
            json!({"source":"let value=$262.IsHTMLDDA;assert.sameValue(!value,true);assert.sameValue(value==null,true);assert.sameValue(typeof value,'undefined');switch(value){case undefined:throw 1;case null:throw 2;case value:break}", "mode":"sloppy", "is_html_dda":true}),
            "ok",
        ),
        (
            json!({"source":"async function work(){}work()", "mode":"sloppy"}),
            "ok",
        ),
        (
            json!({"source":"async function* values(){yield await Promise.resolve(7)}let iterator=values();iterator.next().then(result=>{assert.sameValue(result.value,7);assert.sameValue(result.done,false);return iterator.next()}).then(result=>{assert.sameValue(result.done,true);$DONE()},$DONE)", "mode":"sloppy", "asynchronous":true}),
            "ok",
        ),
        (
            json!({"source":"async function* values(){yield 2;yield 3}async function total(){let sum=0;for await(let value of values())sum+=value;return sum}total().then(sum=>{assert.sameValue(sum,5);$DONE()},$DONE)", "mode":"sloppy", "asynchronous":true}),
            "ok",
        ),
        (
            json!({"source":"1", "mode":"sloppy", "includes":["helpers.js"], "harness_sources":["async function work(){}work()"]}),
            "ok",
        ),
    ];
    let requests: Vec<_> = cases.iter().map(|(request, _)| request.clone()).collect();
    let replies = adapter(&requests, None);
    assert_eq!(replies.len(), cases.len());
    for (reply, (request, expected)) in replies.iter().zip(&cases) {
        assert_eq!(reply["kind"], *expected, "{request}: {reply}");
    }
    assert_eq!(replies[6]["phase"], "parse");
}

#[test]
fn adapter_reports_invalid_requested_modules_at_resolution() {
    for invalid_source in ["0++;", "break;"] {
        let replies = adapter(
            &[json!({
                "source":"import './invalid.js'",
                "mode":"module",
                "module_path":"entry.js",
                "module_sources":{"invalid.js":invalid_source},
            })],
            None,
        );
        assert_eq!(
            replies[0]["phase"], "resolution",
            "{invalid_source}: {replies:?}"
        );
        assert_eq!(
            replies[0]["kind"], "SyntaxError",
            "{invalid_source}: {replies:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn regex_protocol_faults_and_missing_helper_fail_closed() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("bluejs-regex-fault-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let script = directory.join("worker.py");
    std::fs::write(&script, include_bytes!("fixtures/regex_fault.py")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    for mode in [
        "early_exit",
        "bad_ready",
        "after_ready_exit",
        "malformed",
        "oversized",
        "invalid_range",
        "compile_reply",
        "match_reply",
    ] {
        let replies = adapter(
            &[json!({"source":"new RegExp('a').test('a')", "mode":"sloppy"})],
            Some((mode, &script)),
        );
        assert_eq!(replies[0]["kind"], "worker_error", "{mode}: {replies:?}");
    }
    let missing = directory.join("missing");
    for source in ["/a/", "new RegExp('a')"] {
        let replies = adapter(
            &[json!({"source":source,"mode":"sloppy"})],
            Some(("unused", &missing)),
        );
        assert_eq!(replies[0]["kind"], "worker_error");
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn helper_rejects_oversized_frames_and_handles_out_of_range_starts() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bluejs-regexp-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    fn read_frame(output: &mut impl Read) -> Vec<u8> {
        let mut size = [0; 4];
        output.read_exact(&mut size).unwrap();
        let mut bytes = vec![0; u32::from_le_bytes(size) as usize];
        output.read_exact(&mut bytes).unwrap();
        bytes
    }
    assert_eq!(read_frame(&mut output), b"bluejs-regexp-worker/1");
    let request =
        serde_json::to_vec(&json!({"source":[97],"flags":"","input":[97],"start":2})).unwrap();
    input
        .write_all(&(request.len() as u32).to_le_bytes())
        .unwrap();
    input.write_all(&request).unwrap();
    input.flush().unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&read_frame(&mut output)).unwrap(),
        json!({"Found":null})
    );
    input
        .write_all(&(16 * 1024 * 1024u32 + 1).to_le_bytes())
        .unwrap();
    drop(input);
    assert!(!child.wait().unwrap().success());
}

#[test]
fn compilation_deadline_and_transport_limit_are_enforced() {
    let replies = adapter(
        &[
            json!({"source":"new RegExp('a'.repeat(100000))", "mode":"sloppy", "regex_timeout_ms":1}),
            json!({"source":"new RegExp('漢'.repeat(3000000))", "mode":"sloppy", "string_limit":8000000}),
            json!({"source":"assert(/a/.test('a'))", "mode":"sloppy"}),
        ],
        None,
    );
    assert_eq!(replies[0]["kind"], "timeout", "{replies:?}");
    assert_eq!(replies[1]["kind"], "worker_error", "{replies:?}");
    assert_eq!(replies[2]["kind"], "ok", "{replies:?}");
}
