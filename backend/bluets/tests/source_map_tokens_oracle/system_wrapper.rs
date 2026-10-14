// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned System wrapper and executable-body map sites.

use super::*;
use blueice_bluets::ModuleKind;

pub(super) const LOADER: &str = r#"
const fs = require('node:fs');
const vm = require('node:vm');
globalThis.System = { register(dependencies, declare) {
    if (dependencies.length !== 0) throw new Error('unexpected dependency');
    const exports = {};
    const body = declare((name, value) => {
        if (typeof name === 'object') Object.assign(exports, name);
        else exports[name] = value;
        return value;
    }, {id: 'main'});
    if (body.setters.length !== 0) throw new Error('unexpected setter');
    body.execute();
} };
const input = process.argv[1];
vm.runInThisContext(fs.readFileSync(input, 'utf8'), {filename: input});
"#;

type SystemRole<'a> = (&'a str, &'a [(&'a str, i64, i64)]);

fn assert_system_token_roles(
    javascript: &str,
    mappings: &std::collections::BTreeSet<Mapping>,
    compiler: &str,
    roles: &[SystemRole<'_>],
) {
    let tokens = mapped_tokens(javascript, mappings)
        .into_iter()
        .filter(|token| token.source_index >= 0)
        .collect::<Vec<_>>();
    let expected_count = roles.iter().map(|(_, group)| group.len()).sum::<usize>();
    assert_eq!(
        tokens.len(),
        expected_count,
        "{compiler}: mapped token count"
    );
    let mut index = 0;
    for (role, group) in roles {
        for &(spelling, line, column) in *group {
            let token = &tokens[index];
            assert_eq!(
                (
                    token.token.as_str(),
                    token.source_index,
                    token.source_line,
                    token.source_column,
                    token.name_index
                ),
                (spelling, 0, line, column, None),
                "{compiler}: {role} mapped token {index}"
            );
            index += 1;
        }
    }
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
pub(super) fn system_wrapper_frames_match_native() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "Version 5.9.3"
    );

    let source = include_str!("../fixtures/k11_source_maps/system_wrapper.ts");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "blueice-k11-system-wrapper-{}-{nonce}",
        std::process::id()
    ));
    let native = directory.join("native");
    let blueice = directory.join("blueice");
    fs::create_dir_all(&native).unwrap();
    fs::create_dir_all(&blueice).unwrap();
    let input = directory.join("main.ts");
    fs::write(&input, source).unwrap();
    let native_build = Command::new(&tsc)
        .args([
            "--target",
            "ES2022",
            "--module",
            "System",
            "--strict",
            "--pretty",
            "false",
            "--sourceMap",
            "--inlineSources",
            "--outDir",
        ])
        .arg(&native)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        native_build.status.success(),
        "{}",
        String::from_utf8_lossy(&native_build.stderr)
    );

    let module = "memory:///main.ts";
    let compilation = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            module_kind: ModuleKind::System,
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let artifact = &output.artifacts[module];
    let native_javascript = fs::read_to_string(native.join("main.js")).unwrap();
    let native_map: Value =
        serde_json::from_str(&fs::read_to_string(native.join("main.js.map")).unwrap()).unwrap();
    let blue_map: Value =
        serde_json::from_str(&artifact.source_map.as_ref().unwrap().to_json()).unwrap();
    assert_eq!(blue_map["names"], native_map["names"]);
    assert_eq!(blue_map["sourcesContent"], native_map["sourcesContent"]);
    let plain = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            module_kind: ModuleKind::System,
            ..CompilerOptions::default()
        },
    );
    assert!(!plain.has_errors(), "{:#?}", plain.diagnostics);
    assert_eq!(
        artifact.javascript,
        plain.output.unwrap().artifacts[module].javascript,
        "System map instrumentation must not change JavaScript"
    );
    fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();

    let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
    let run = |path: &Path| {
        Command::new(&node)
            .arg("-e")
            .arg(LOADER)
            .arg(path)
            .output()
            .unwrap()
    };
    let native_run = run(&native.join("main.js"));
    let blue_run = run(&blueice.join("main.js"));
    assert!(
        native_run.status.success(),
        "{}",
        String::from_utf8_lossy(&native_run.stderr)
    );
    assert!(
        blue_run.status.success(),
        "{}",
        String::from_utf8_lossy(&blue_run.stderr)
    );
    assert_eq!(blue_run.status, native_run.status);
    assert_eq!(blue_run.stdout, native_run.stdout);
    assert_eq!(blue_run.stdout, b"42\n");
    assert_eq!(blue_run.stderr, native_run.stderr);

    let native_mappings = decode_map(&native_map);
    let blue_mappings = decode_map(&blue_map);
    let native_roles: &[SystemRole<'_>] = &[
        (
            "hoisted function header",
            &[
                ("function", 4, 0),
                ("bump", 4, 16),
                ("(", 4, 20),
                (")", 4, 20),
                ("{", 4, 20),
            ],
        ),
        (
            "exported compound assignment and function close",
            &[
                ("exports_1", 4, 31),
                ("(", 4, 31),
                ("\"answer\"", 4, 31),
                (",", 4, 31),
                ("answer", 4, 31),
                ("+=", 4, 37),
                ("22", 4, 41),
                (")", 4, 43),
                (";", 4, 43),
                ("}", 4, 45),
            ],
        ),
        (
            "exported initializer",
            &[
                ("exports_1", 3, 0),
                ("(", 3, 0),
                ("\"answer\"", 3, 0),
                (",", 3, 0),
                ("answer", 3, 11),
                ("=", 3, 17),
                ("20", 3, 28),
                (")", 3, 30),
                (";", 3, 30),
            ],
        ),
        (
            "user function call",
            &[("bump", 5, 0), ("(", 5, 4), (")", 5, 4), (";", 5, 6)],
        ),
        (
            "console read",
            &[
                ("console", 6, 0),
                (".", 6, 7),
                ("log", 6, 8),
                ("(", 6, 11),
                ("answer", 6, 12),
                (")", 6, 18),
                (";", 6, 19),
            ],
        ),
    ];
    let blue_roles: &[SystemRole<'_>] = &[
        (
            "hoisted function header",
            &[
                ("function", 4, 0),
                ("bump", 4, 16),
                ("(", 4, 20),
                (")", 4, 20),
                ("{", 4, 20),
            ],
        ),
        (
            "exported compound assignment and function close",
            &[
                ("exports", 4, 31),
                (".", 4, 31),
                ("answer", 4, 31),
                ("+=", 4, 37),
                ("22", 4, 41),
                (";", 4, 43),
                ("}", 4, 45),
            ],
        ),
        (
            "exported initializer",
            &[
                ("exports", 3, 0),
                (".", 3, 0),
                ("answer", 3, 11),
                ("=", 3, 17),
                ("20", 3, 28),
                (";", 3, 30),
            ],
        ),
        (
            "user function call",
            &[("bump", 5, 0), ("(", 5, 4), (")", 5, 4), (";", 5, 6)],
        ),
        (
            "console read",
            &[
                ("console", 6, 0),
                (".", 6, 7),
                ("log", 6, 8),
                ("(", 6, 11),
                ("exports", 6, 12),
                (".", 6, 12),
                ("answer", 6, 12),
                (")", 6, 18),
                (";", 6, 19),
            ],
        ),
    ];
    assert_system_token_roles(&native_javascript, &native_mappings, "Native", native_roles);
    assert_system_token_roles(&artifact.javascript, &blue_mappings, "BlueTSC", blue_roles);
    let sites = |javascript: &str| {
        [
            javascript.find("System.register").unwrap(),
            javascript.find("function bump").unwrap(),
            javascript.rfind("console.log(").unwrap(),
        ]
    };
    let native_sites = sites(&native_javascript)
        .map(|offset| mapped_source_at(&native_javascript, &native_mappings, offset));
    let blue_sites = sites(&artifact.javascript)
        .map(|offset| mapped_source_at(&artifact.javascript, &blue_mappings, offset));
    let body_open = |javascript: &str| {
        let function = javascript.find("function bump").unwrap();
        function + javascript[function..].find('{').unwrap()
    };
    let native_body_open = mapped_source_at(
        &native_javascript,
        &native_mappings,
        body_open(&native_javascript),
    );
    let blue_body_open = mapped_source_at(
        &artifact.javascript,
        &blue_mappings,
        body_open(&artifact.javascript),
    );
    for (generated, original) in [
        ("exports.answer += 22", "answer += 22"),
        ("console.log(exports.answer)", "console.log(answer)"),
    ] {
        let property = artifact.javascript.find(generated).unwrap();
        let property = property + generated.find("exports.answer").unwrap();
        let original = source.find(original).unwrap() + original.find("answer").unwrap();
        let (line, column) = source_position(source, original);
        let expected = Some((0, line, column, None));
        for offset in [0, "exports".len(), "exports.".len()] {
            assert_eq!(
                mapped_source_at(&artifact.javascript, &blue_mappings, property + offset),
                expected,
                "System property rewrite must retain the original binding site"
            );
        }
    }
    let initializer = artifact.javascript.find("exports.answer= 20").unwrap();
    let export_start = source.find("export let answer").unwrap();
    let answer_start = export_start + "export let ".len();
    for (offset, original) in [
        (0, export_start),
        ("exports".len(), export_start),
        ("exports.".len(), answer_start),
    ] {
        let (line, column) = source_position(source, original);
        assert_eq!(
            mapped_source_at(&artifact.javascript, &blue_mappings, initializer + offset),
            Some((0, line, column, None)),
            "System export initializer must use Native's keyword/name sites"
        );
    }
    for (javascript, mappings, execute_start, compiler) in [
        (
            &native_javascript,
            &native_mappings,
            native_javascript.rfind("exports_1(\"answer\"").unwrap(),
            "Native",
        ),
        (
            &artifact.javascript,
            &blue_mappings,
            artifact.javascript.find("exports.answer= 20").unwrap(),
            "BlueTSC",
        ),
    ] {
        let function_start = javascript.find("function bump").unwrap();
        let function_end = function_start + javascript[function_start..].find('}').unwrap() + 1;
        let console_start = javascript.rfind("console.log(").unwrap();
        let execute_end = console_start + javascript[console_start..].find(';').unwrap() + 1;
        for token in lex("<system-wrapper>", javascript).unwrap() {
            if token.kind == TokenKind::Eof {
                continue;
            }
            let user_token = (function_start..function_end).contains(&token.start)
                || (execute_start..execute_end).contains(&token.start);
            assert_eq!(
                mapped_source_at(javascript, mappings, token.start).is_some(),
                user_token,
                "{compiler}: token {:?} has the wrong mapped/unmapped role",
                token.text
            );
        }
    }
    fs::remove_dir_all(&directory).unwrap();
    let source_site = |offset| {
        let (line, column) = source_position(source, offset);
        Some((0, line, column, None))
    };
    assert_eq!(
        native_sites,
        [
            None,
            source_site(source.find("export function bump").unwrap()),
            source_site(source.find("console.log(").unwrap())
        ],
        "pinned TypeScript System sites"
    );
    assert_eq!(blue_sites, native_sites, "System wrapper and body sites");
    assert_eq!(
        native_body_open,
        source_site(source.find("bump()").unwrap() + "bump".len()),
        "Native function body inherits the parameter-list start"
    );
    assert_eq!(
        blue_body_open, native_body_open,
        "System function-body boundary"
    );
}
