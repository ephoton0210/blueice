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
    let mut corpus: Vec<String> = include_str!("fixtures/execution.txt")
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();
    corpus.extend(
        include_str!("fixtures/string_protocols.txt")
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .map(str::to_owned),
    );
    corpus.extend(
        include_str!("fixtures/bound_functions.txt")
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .map(str::to_owned),
    );
    for locale in [
        "en", "tr", "az", "lt", "el", "sv", "de", "da", "ja", "th", "zz",
    ] {
        for string in ["Iİiı", "I\\u0301", "ΟΣ", "άι", "Straße", "I\\ud800İ", ""] {
            for method in ["toLocaleLowerCase", "toLocaleUpperCase"] {
                corpus.push(format!("'{string}'.{method}('{locale}')"));
            }
        }
        for options in [
            "{}",
            "{numeric:true}",
            "{sensitivity:'base'}",
            "{sensitivity:'accent'}",
            "{sensitivity:'case'}",
            "{caseFirst:'upper'}",
            "{caseFirst:'lower'}",
            "{ignorePunctuation:true}",
        ] {
            for (left, right) in [
                ("ä", "z"),
                ("é", "e"),
                ("2", "10"),
                ("A", "a"),
                ("a-b", "ab"),
            ] {
                corpus.push(format!(
                    "'{left}'.localeCompare('{right}','{locale}',{options})"
                ));
            }
        }
    }
    for tag in [
        "en-US",
        "iw",
        "sh",
        "mo",
        "en-u-kn-true",
        "de-u-co-phonebk",
        "en-t-en-us",
        "en-u-ca-gregory-ca-buddhist",
        "en-a-foo-a-bar",
        "en-1901-1901",
        "en_US",
        "abcd",
        "en-abc",
        "en-x-private",
        "x-private",
        "en-u",
        "zh-cmn",
        "i-klingon",
    ] {
        corpus.push(format!("Intl.getCanonicalLocales('{tag}').join(',')"));
    }
    for source in [
        "new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').toString()",
        "new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').baseName",
        "new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').calendar",
        "new Intl.Locale('en',{language:'fr',script:'Latn',region:'CA',calendar:'gregory',collation:'phonebk',hourCycle:'h23',caseFirst:'upper',numeric:true,numberingSystem:'latn'}).toString()",
        "new Intl.Locale('en',{numeric:false,firstDayOfWeek:1}).numeric",
        "new Intl.Locale('en',{firstDayOfWeek:1}).firstDayOfWeek",
        "new Intl.Locale('zh').maximize().toString()",
        "new Intl.Locale('zh-Hans-CN').minimize().toString()",
        "Intl.getCanonicalLocales(new Intl.Locale('iw-IL')).join(',')",
        "Intl.Locale('en')",
    ] {
        corpus.push(source.into());
    }
    for source in [
        "Math.E + Math.LN10 + Math.LN2 + Math.LOG10E + Math.LOG2E + Math.PI + Math.SQRT1_2 + Math.SQRT2",
        "Math.abs(-3) + Math.acos(1) + Math.acosh(1) + Math.asin(0) + Math.asinh(0) + Math.atan(0) + Math.atanh(0)",
        "Math.ceil(0.1) + Math.cbrt(27) + Math.cos(0) + Math.cosh(0) + Math.exp(0) + Math.expm1(0)",
        "Math.floor(0.9) + Math.fround(1.1) + Math.log(1) + Math.log1p(0) + Math.log2(8) + Math.log10(100)",
        "Math.sin(0) + Math.sinh(0) + Math.sqrt(9) + Math.tan(0) + Math.tanh(0) + Math.trunc(-1.9)",
        "Math.atan2(1,0) + Math.pow(2,8) + Math.hypot(3,4)",
        "Math.imul(0xffffffff,5) + Math.clz32(1)",
        "1 / Math.max(-0,0)",
        "1 / Math.round(-0.1)",
        "1 / Math.sign(-0)",
    ] {
        corpus.push(source.into());
    }
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
    for input in [
        r"\ud800\udc00",
        r"\udbff\udfff",
        r"\ud800\ud800\udc00",
        r"\ud800\udc00\udc00",
        r"\udc00\ud800",
        r"\udc00\ud800\udc00\ud800",
    ] {
        corpus.push(format!("RegExp.escape('{input}')"));
    }
    for length in [
        "Infinity",
        "-Infinity",
        "NaN",
        "-0",
        "-3.9",
        "3.9",
        "'3'",
        "Symbol()",
        "{valueOf(){throw 1;}}",
    ] {
        corpus.push(format!("function f(){{}} Object.defineProperty(f,'length',{{value:{length}}}); f.bind(null,1).length"));
    }
    for input in [
        "",
        "undefined",
        "null",
        "1",
        "true",
        "Symbol()",
        "new String('x')",
        "{toString(){throw 1;}}",
        "{[Symbol.toPrimitive](){throw 1;}}",
    ] {
        corpus.push(format!("RegExp.escape({input})"));
    }
    // Cross-product exercises the independent regexp matcher, UTF-16 offsets,
    // replacement expansion and split capture insertion through String APIs.
    for pattern in [
        "",
        "a",
        "(a)(b)?",
        "(?<x>a)",
        "a|b",
        "^a",
        "b$",
        ".",
        "[ab]+",
        "[^a]",
        "a*?",
        "(?=a)",
        "(?<=a)b",
        "(a)\\1",
        "\\d+",
        "\\p{ASCII}",
        "[a&&b]",
        "\\u{1F600}",
    ] {
        for flags in ["", "g", "y", "u", "gu", "gy", "v", "dgi"] {
            for string in ["", "ab", "aba", "aa", "a1b22", "A😀B", "\\ud800a"] {
                for operation in [
                    "s.search(r)",
                    "String(s.match(r))",
                    "s.replace(r,'[$&][$1][$<x>]')",
                    "String(s.split(r))",
                ] {
                    corpus.push(format!(
                        "let s='{string}'; let r=new RegExp({pattern:?},'{flags}'); {operation}"
                    ));
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
        for position in [
            "undefined",
            "NaN",
            "-Infinity",
            "Infinity",
            "-4",
            "-1",
            "-0",
            "0",
            "1",
            "2",
            "4",
            "0.9",
            "-0.9",
        ] {
            for method in [
                "at",
                "charAt",
                "charCodeAt",
                "codePointAt",
                "slice",
                "substring",
                "substr",
            ] {
                corpus.push(format!("'{string}'.{method}({position})"));
            }
            for method in [
                "indexOf",
                "lastIndexOf",
                "startsWith",
                "endsWith",
                "includes",
            ] {
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
    assert_matches_node(&corpus);
}

/// Strict Node/ICU differential cells chosen from NumberFormat provider data
/// that is already marked data-backed by the provider coverage inventory.
///
/// Keep each entry a primitive result so `node_oracle.js` can compare the
/// complete UTF-16 output, including spaces, digits and range-part sources.
#[test]
#[ignore = "requires Node.js on PATH; run explicitly with --ignored"]
fn number_format_matrix_matches_node() {
    let mut corpus = [
        // Decimal symbols, grouping, and non-Latin numbering systems.
        "new Intl.NumberFormat('en',{maximumFractionDigits:3}).format(1234567.895)",
        "new Intl.NumberFormat('de',{useGrouping:false,minimumFractionDigits:2,maximumFractionDigits:2}).format(1007.5)",
        "new Intl.NumberFormat('ar-u-nu-arab',{maximumFractionDigits:1}).format(1234.5)",
        "new Intl.NumberFormat('th-u-nu-thai',{useGrouping:false,minimumFractionDigits:2,maximumFractionDigits:2}).format(1007.5)",
        "new Intl.NumberFormat('hi-IN').format(1234567.89)",
        // Full-CLDR currency display, alpha spacing, and positive/negative
        // pattern data. These include a regional symbol distinction, narrow
        // fallback, ISO-code alpha adjacency and Arabic digits/bidi controls.
        "new Intl.NumberFormat('en-US',{style:'currency',currency:'USD',currencySign:'accounting'}).format(-987)",
        "new Intl.NumberFormat('de-DE',{style:'currency',currency:'EUR'}).format(1234.5)",
        "new Intl.NumberFormat('ja-JP',{style:'currency',currency:'JPY'}).format(-1234.5)",
        "new Intl.NumberFormat('fr-CA',{style:'currency',currency:'USD',currencyDisplay:'symbol'}).format(1234.5)",
        "new Intl.NumberFormat('fr-CA',{style:'currency',currency:'USD',currencyDisplay:'narrowSymbol'}).format(1234.5)",
        "new Intl.NumberFormat('fr-CA',{style:'currency',currency:'USD',currencyDisplay:'code'}).format(1234.5)",
        "new Intl.NumberFormat('fr-CA',{style:'currency',currency:'USD',currencyDisplay:'name'}).format(2)",
        "new Intl.NumberFormat('ar-u-nu-arab',{style:'currency',currency:'USD',currencyDisplay:'code'}).formatToParts(1234).map(function(part){return part.type+':'+part.value}).join('|')",
        "new Intl.NumberFormat('fr',{style:'percent',maximumFractionDigits:1}).format(-12.345)",
        // Simple and compound unit labels from raw unit data.
        "new Intl.NumberFormat('en',{style:'unit',unit:'meter',unitDisplay:'long'}).format(2)",
        "new Intl.NumberFormat('de',{style:'unit',unit:'kilometer-per-hour',unitDisplay:'long'}).format(123)",
        "new Intl.NumberFormat('fr',{style:'unit',unit:'megabyte',unitDisplay:'short'}).format(2)",
        "new Intl.NumberFormat('ko',{style:'unit',unit:'kilometer-per-hour',unitDisplay:'long'}).format(-987)",
        "new Intl.NumberFormat('ja',{style:'unit',unit:'liter',unitDisplay:'narrow'}).format(3)",
        // Compact/scientific output and typed part boundaries.
        "new Intl.NumberFormat('en',{notation:'compact'}).formatToParts(9876).map(function(part){return part.type+':'+part.value}).join('|')",
        "new Intl.NumberFormat('sw',{notation:'compact',compactDisplay:'long'}).formatToParts(1200).map(function(part){return part.type+':'+part.value}).join('|')",
        "new Intl.NumberFormat('he',{notation:'compact'}).formatToParts(1200).map(function(part){return part.type+':'+part.value}).join('|')",
        "new Intl.NumberFormat('de',{notation:'engineering'}).formatToParts(.000345).map(function(part){return part.type+':'+part.value}).join('|')",
        "new Intl.NumberFormat('en',{notation:'scientific'}).format(543211.1)",
        // CLDR scientific separators, mathematical minus signs, and bidi
        // controls are provider data rather than a synthesized `E-` shape.
        "new Intl.NumberFormat('ar-u-nu-arab',{notation:'scientific',maximumFractionDigits:1}).format(-.00123)",
        "new Intl.NumberFormat('fa',{notation:'scientific',maximumFractionDigits:1}).format(-.00123)",
        "new Intl.NumberFormat('et',{notation:'scientific',maximumFractionDigits:1}).format(-.00123)",
        "new Intl.NumberFormat('ps-u-nu-arabext',{notation:'scientific',maximumFractionDigits:1}).format(-.00123)",
        // Exact StringIntlMV/BigInt values must bypass IEEE-754 rounding.
        "new Intl.NumberFormat('en',{useGrouping:false,maximumFractionDigits:20}).format('1.234567890123456789e0')",
        "new Intl.NumberFormat('en').format(' 987654321987654321 ')",
        "new Intl.NumberFormat('en').formatRange('987654321987654321','987654321987654322')",
        "new Intl.NumberFormat('en').formatRange(9007199254740993n,9007199254740994n)",
        // Range strings and source ownership are independently observable.
        "new Intl.NumberFormat('en-US',{style:'currency',currency:'USD',maximumFractionDigits:0}).formatRange(3,5)",
        "new Intl.NumberFormat('en-US',{style:'currency',currency:'USD',maximumFractionDigits:0}).formatRangeToParts(3,5).map(function(part){return part.type+':'+part.value+':'+part.source}).join('|')",
        "new Intl.NumberFormat('pt-PT',{style:'currency',currency:'EUR',maximumFractionDigits:0}).formatRange(3,5)",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    // Generic compounds that formerly crossed the English compatibility
    // boundary. These language/script/region representatives exercise the
    // pinned full-CLDR denominator display-name and generic `per` paths;
    // Rust's provider matrix separately covers every sanctioned pair.
    corpus.extend(
        [
            "af", "am", "bo", "bs-Latn", "bs-Cyrl", "ceb", "chr", "cy", "dz", "ee", "ga",
            "gv", "haw", "ig", "kl", "ln", "mn", "mt", "om", "se", "si", "so", "to", "ug",
            "wae", "wo", "xh", "yi", "yo", "zu", "sr-Latn", "zh-TW",
        ]
        .into_iter()
        .map(|locale| {
            format!(
                "new Intl.NumberFormat('{locale}',{{style:'unit',unit:'gigabyte-per-acre',unitDisplay:'long'}}).format(2)"
            )
        }),
    );
    assert_matches_node(&corpus);
}

/// Currency and unit range output is governed by UTS 35's semantic-affix
/// collapse algorithm, not by a separate CLDR `currencyInterval` table. Run
/// selected provider locales through an independent Node 24 oracle so changes
/// to range sharing or plural-range reconstruction cannot fix one locale
/// family while regressing another. `formatRangeToParts()` source assignment
/// stays in the host-neutral tests: ECMA-402 deliberately makes collapsing
/// implementation-defined, so Node's source spans are not a portable oracle.
#[test]
#[ignore = "requires Node.js on PATH; run explicitly with --ignored"]
fn number_format_range_matrix_matches_node() {
    // Node 24's ICU data predates the pinned CLDR 48.2.1 provider for some
    // currency symbols. These representatives use stable raw records while
    // covering every range-connector and prefix/suffix-affix class.
    const RANGE_LOCALES: &[&str] = &["en", "ja"];
    const UNIT_CELLS: &[(&str, &str)] = &[
        ("acre", "long"),
        ("acre", "short"),
        ("acre", "narrow"),
        ("celsius", "long"),
        ("celsius", "short"),
        ("celsius", "narrow"),
        ("gigabyte", "long"),
        ("gigabyte", "short"),
        ("gigabyte", "narrow"),
        ("meter", "long"),
        ("meter", "short"),
        ("meter", "narrow"),
        ("percent", "long"),
        ("percent", "short"),
        ("percent", "narrow"),
        ("year", "long"),
        ("year", "short"),
        ("year", "narrow"),
        ("gigabyte-per-acre", "long"),
        ("gigabyte-per-acre", "short"),
        ("gigabyte-per-acre", "narrow"),
        ("celsius-per-liter", "long"),
        ("celsius-per-liter", "short"),
        ("celsius-per-liter", "narrow"),
    ];
    const RANGE_ENDPOINTS: &[(i8, i8)] = &[(1, 2), (2, 5), (-5, -2), (-3, 5)];

    let mut corpus = Vec::new();
    for &locale in RANGE_LOCALES {
        let mut cells = Vec::new();
        for currency_display in ["symbol", "code"] {
            for &(start, end) in RANGE_ENDPOINTS {
                cells.push(format!(
                    "new Intl.NumberFormat('{locale}',{{style:'currency',currency:'USD',currencyDisplay:'{currency_display}',maximumFractionDigits:0}}).formatRange({start},{end})"
                ));
            }
        }
        for &(unit, unit_display) in UNIT_CELLS {
            for &(start, end) in RANGE_ENDPOINTS {
                cells.push(format!(
                    "new Intl.NumberFormat('{locale}',{{style:'unit',unit:'{unit}',unitDisplay:'{unit_display}'}}).formatRange({start},{end})"
                ));
            }
        }
        // One primitive per locale keeps each fixture independent while
        // amortizing realm creation across the complete interval matrix.
        corpus.push(format!("[{}].join('\\u001f')", cells.join(",")));
    }
    let expected = node_oracle(&corpus);
    for ((&locale, source), expected) in RANGE_LOCALES.iter().zip(&corpus).zip(expected) {
        let actual = evaluate_source(source);
        let actual = decode_utf16_hex(
            actual
                .strip_prefix("string:")
                .expect("range matrix fixture returns a string"),
        );
        let expected = decode_utf16_hex(
            expected
                .strip_prefix("string:")
                .expect("Node range matrix fixture returns a string"),
        );
        let actual_cells = actual.split('\u{1f}').collect::<Vec<_>>();
        let expected_cells = expected.split('\u{1f}').collect::<Vec<_>>();
        assert_eq!(
            actual_cells.len(),
            expected_cells.len(),
            "{locale} range matrix cell count"
        );
        for (cell, (actual, expected)) in actual_cells.into_iter().zip(expected_cells).enumerate() {
            assert_eq!(actual, expected, "{locale} range matrix cell {cell}");
        }
    }
}

fn assert_matches_node(corpus: &[String]) {
    let expected = node_oracle(corpus);
    println!(
        "Comparing {} isolated scripts against Node.js",
        corpus.len()
    );
    for (source, expected) in corpus.iter().zip(expected) {
        assert_eq!(evaluate_source(source), expected, "{source}");
    }
}

fn node_oracle(corpus: &[String]) -> Vec<String> {
    let mut node = Command::new("node")
        .args(["-e", include_str!("fixtures/node_oracle.js")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("install Node.js to run the opt-in differential test");
    let mut stdin = node.stdin.take().unwrap();
    let input: String = corpus
        .iter()
        .map(|source| {
            source
                .bytes()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                + "\n"
        })
        .collect();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = node.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        output.status.success(),
        "Node oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = String::from_utf8(output.stdout).unwrap();
    let expected: Vec<_> = expected.lines().collect();
    assert_eq!(
        corpus.len(),
        expected.len(),
        "oracle must return exactly one result per script"
    );
    expected.into_iter().map(str::to_owned).collect()
}

fn evaluate_source(source: &str) -> String {
    match parse(source) {
        Err(_) => "error:SyntaxError".into(),
        Ok(ast) => match compile(&ast) {
            Err(
                blueice_bluejs::CompileError::DuplicateBinding(_)
                | blueice_bluejs::CompileError::InvalidSyntax(_),
            ) => "error:SyntaxError".into(),
            Err(error) => panic!("fixture {source} cannot execute: {error}"),
            // Intrinsics are mutable and VM-owned. Fresh bindings alone do
            // not isolate prototype writes between oracle fixtures.
            Ok(code) => canonical(Vm::default().execute(&code)),
        },
    }
}

fn decode_utf16_hex(encoded: &str) -> String {
    let (chunks, _) = encoded.as_bytes().as_chunks::<4>();
    let code_units = chunks
        .iter()
        .map(|chunk| {
            u16::from_str_radix(std::str::from_utf8(chunk).expect("hex is ASCII"), 16)
                .expect("oracle string contains UTF-16 hex")
        })
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&code_units)
}

fn canonical(result: Result<Value, RuntimeError>) -> String {
    match result {
        Ok(Value::Undefined) => "undefined".into(),
        Ok(Value::Null) => "null".into(),
        Ok(Value::Bool(b)) => format!("bool:{b}"),
        Ok(Value::Number(n)) if n.is_nan() => "number:NaN".into(),
        Ok(Value::Number(n)) => format!("number:{:016x}", n.to_bits()),
        Ok(Value::String(s)) => format!(
            "string:{}",
            s.as_code_units()
                .iter()
                .map(|unit| format!("{unit:04x}"))
                .collect::<String>()
        ),
        Err(RuntimeError::ReferenceError(_)) => "error:ReferenceError".into(),
        Err(RuntimeError::TypeError(_)) => "error:TypeError".into(),
        Err(RuntimeError::RangeError(_)) => "error:RangeError".into(),
        Err(RuntimeError::SyntaxError(_)) => "error:SyntaxError".into(),
        other => panic!("unexpected fixture result: {other:?}"),
    }
}
