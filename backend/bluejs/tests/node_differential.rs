// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicit opt-in: cargo test -p blueice-bluejs --test node_differential -- --ignored
//! Node is an independent oracle, not a runtime or default-test dependency.
use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires Node.js on PATH; run explicitly with --ignored"]
fn primitive_completions_and_error_classes_match_node() {
    let mut corpus: Vec<String> = include_str!("fixtures/execution.txt").lines().filter(|line| !line.trim().is_empty() && !line.starts_with('#')).map(str::to_owned).collect();
    corpus.extend(include_str!("fixtures/string_protocols.txt").lines().filter(|line| !line.trim().is_empty() && !line.starts_with('#')).map(str::to_owned));
    corpus.extend(include_str!("fixtures/bound_functions.txt").lines().filter(|line| !line.trim().is_empty() && !line.starts_with('#')).map(str::to_owned));
    // Every UTF-16 code unit in initial and non-initial position. Batching
    // amortizes realm bootstrap while retaining exact independent results.
    for start in (0..=0xffff).step_by(256) {
        let mut parts = Vec::new();
        for unit in start..start + 256 {
            parts.push(format!("RegExp.escape('\\u{unit:04x}')"));
            parts.push(format!("RegExp.escape('_\\u{unit:04x}')"));
        }
        corpus.push(format!("[{}].join('|')", parts.join(",")));
    }
    for input in [r"\ud800\udc00", r"\udbff\udfff", r"\ud800\ud800\udc00", r"\ud800\udc00\udc00", r"\udc00\ud800", r"\udc00\ud800\udc00\ud800"] {
        corpus.push(format!("RegExp.escape('{input}')"));
    }
    for length in ["Infinity", "-Infinity", "NaN", "-0", "-3.9", "3.9", "'3'", "Symbol()", "{valueOf(){throw 1;}}"] {
        corpus.push(format!("function f(){{}} Object.defineProperty(f,'length',{{value:{length}}}); f.bind(null,1).length"));
    }
    for input in ["", "undefined", "null", "1", "true", "Symbol()", "new String('x')", "{toString(){throw 1;}}", "{[Symbol.toPrimitive](){throw 1;}}"] {
        corpus.push(format!("RegExp.escape({input})"));
    }
    // Cross-product exercises the independent regexp matcher, UTF-16 offsets,
    // replacement expansion and split capture insertion through String APIs.
    for pattern in ["", "a", "(a)(b)?", "(?<x>a)", "a|b", "^a", "b$", ".", "[ab]+", "[^a]", "a*?", "(?=a)", "(?<=a)b", "(a)\\1", "\\d+", "\\p{ASCII}", "[a&&b]", "\\u{1F600}"] {
        for flags in ["", "g", "y", "u", "gu", "gy", "v", "dgi"] {
            for string in ["", "ab", "aba", "aa", "a1b22", "A😀B", "\\ud800a"] {
                for operation in ["s.search(r)", "String(s.match(r))", "s.replace(r,'[$&][$1][$<x>]')", "String(s.split(r))"] {
                    corpus.push(format!("let s='{string}'; let r=new RegExp({pattern:?},'{flags}'); {operation}"));
                }
                if flags.contains('g') {
                    corpus.push(format!("let r=new RegExp({pattern:?},'{flags}'); let out=''; for(let m of '{string}'.matchAll(r)){{out+=m.index+':'+m[0]+';';}} out"));
                }
            }
        }
    }
    // Transport each source as hex UTF-8 so real line terminators cannot
    // accidentally turn a multiline script into several separate fixtures.
    for newline in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        corpus.extend([
            format!("let x=1{newline}x+2"),
            format!("let x=1// ignored{newline}x+2"),
            format!("let x=1/*{newline}*/x+2"),
            format!("let x=1; let y=2; x{newline}++y; y"),
            format!("throw{newline}1"),
            format!("'a\\{newline}b'"),
            format!("'a{newline}b'"),
            format!("`a{newline}b`"),
            format!("`a\\{newline}b`"),
            format!("`${{1 // }} ` ' ignored{newline}+2}}`"),
            format!("`${{'a\\{newline}b'}}`"),
        ]);
    }
    for space in ['\u{feff}', '\u{00a0}', '\u{0085}', '\u{180e}', '\u{200b}'] {
        corpus.push(format!("1{space}+2"));
    }
    // Exercise every lone surrogate through both the lexer and a native
    // constructor. Result framing must not silently replace any of them.
    for unit in 0xd800..=0xdfff {
        corpus.push(format!("'\\u{unit:04x}'"));
        corpus.push(format!("String.fromCharCode({unit})"));
    }
    for string in ["", "abc", r"\ud800", r"A\ud83d\ude00B", r"e\u0301"] {
        for position in ["undefined", "NaN", "-Infinity", "Infinity", "-4", "-1", "-0", "0", "1", "2", "4", "0.9", "-0.9"] {
            for method in ["at", "charAt", "charCodeAt", "codePointAt", "slice", "substring", "substr"] {
                corpus.push(format!("'{string}'.{method}({position})"));
            }
            for method in ["indexOf", "lastIndexOf", "startsWith", "endsWith", "includes"] {
                for search in ["", "a", r"\ude00"] {
                    corpus.push(format!("'{string}'.{method}('{search}',{position})"));
                }
            }
        }
    }
    // Deterministic broad float coverage, including shortest-decimal
    // formatting. Every expected value still comes from Node, not Rust.
    let mut state = 0x83da_172c_d093_1b57u64;
    for _ in 0..2048 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let n = f64::from_bits(state);
        if n.is_finite() {
            corpus.push(format!("'' + ({n:e})"));
        }
    }
    // Every finite binary exponent, both sides of the binade boundary,
    // both signs; include the smallest/largest subnormals explicitly.
    for exponent in 0u64..2047 {
        for fraction in [1, (1u64 << 52) - 1] {
            for sign in [0, 1u64 << 63] {
                let n = f64::from_bits(sign | (exponent << 52) | fraction);
                corpus.push(format!("'' + ({n:e})"));
            }
        }
    }
    // Dense neighbors of a known decimal midpoint, not only broad samples.
    let midpoint = (1_114_289_515_931_746.0f64 + 0.25).to_bits();
    for bits in midpoint - 64..=midpoint + 64 {
        for sign in [0, 1u64 << 63] {
            let n = f64::from_bits(bits | sign);
            corpus.push(format!("'' + ({n:e})"));
        }
    }
    let mut node = Command::new("node")
        .args(["-e", include_str!("fixtures/node_oracle.js")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("install Node.js to run the opt-in differential test");
    let mut stdin = node.stdin.take().unwrap();
    let input: String = corpus.iter().map(|source| source.bytes().map(|b| format!("{b:02x}")).collect::<String>() + "\n").collect();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = node.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(output.status.success(), "Node oracle failed: {}", String::from_utf8_lossy(&output.stderr));
    let expected = String::from_utf8(output.stdout).unwrap();
    let expected: Vec<_> = expected.lines().collect();
    assert_eq!(corpus.len(), expected.len(), "oracle must return exactly one result per script");
    println!("Comparing {} isolated scripts against Node.js", corpus.len());
    for (source, expected) in corpus.iter().zip(expected) {
        let result = match parse(source) {
            Err(_) => "error:SyntaxError".into(),
            Ok(ast) => match compile(&ast) {
                Err(blueice_bluejs::CompileError::DuplicateBinding(_) | blueice_bluejs::CompileError::InvalidSyntax(_)) => "error:SyntaxError".into(),
                Err(error) => panic!("fixture {source} cannot execute: {error}"),
                // Intrinsics are mutable and VM-owned. Fresh bindings alone
                // do not isolate prototype writes between oracle fixtures.
                Ok(code) => canonical(Vm::default().execute(&code)),
            },
        };
        assert_eq!(result, expected, "{source}");
    }
}

fn canonical(result: Result<Value, RuntimeError>) -> String {
    match result {
        Ok(Value::Undefined) => "undefined".into(),
        Ok(Value::Null) => "null".into(),
        Ok(Value::Bool(b)) => format!("bool:{b}"),
        Ok(Value::Number(n)) if n.is_nan() => "number:NaN".into(),
        Ok(Value::Number(n)) => format!("number:{:016x}", n.to_bits()),
        Ok(Value::String(s)) => format!("string:{}", s.as_code_units().iter().map(|unit| format!("{unit:04x}")).collect::<String>()),
        Err(RuntimeError::ReferenceError(_)) => "error:ReferenceError".into(),
        Err(RuntimeError::TypeError(_)) => "error:TypeError".into(),
        Err(RuntimeError::RangeError(_)) => "error:RangeError".into(),
        Err(RuntimeError::SyntaxError(_)) => "error:SyntaxError".into(),
        other => panic!("unexpected fixture result: {other:?}"),
    }
}
