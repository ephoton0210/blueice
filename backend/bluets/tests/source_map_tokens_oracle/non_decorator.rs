// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generated object-spread helper source-map frames at ES5.

use super::*;
use blueice_bluets::EcmaTarget;

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
pub(super) fn object_spread_helper_frames_match_native() {
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

    let source = include_str!("../fixtures/k11_source_maps/object_spread_helper.ts");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "blueice-k11-spread-helper-{}-{nonce}",
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
            "ES5",
            "--module",
            "ES2022",
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
            target: EcmaTarget::Es5,
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
            target: EcmaTarget::Es5,
            ..CompilerOptions::default()
        },
    );
    assert!(!plain.has_errors(), "{:#?}", plain.diagnostics);
    assert_eq!(
        artifact.javascript,
        plain.output.unwrap().artifacts[module].javascript,
        "target-helper map markers must not change JavaScript"
    );

    let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
    fs::write(blueice.join("main.js"), &artifact.javascript).unwrap();
    let native_run = Command::new(&node)
        .arg(native.join("main.js"))
        .output()
        .unwrap();
    let blue_run = Command::new(&node)
        .arg(blueice.join("main.js"))
        .output()
        .unwrap();
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
    assert_eq!(blue_run.stdout, b"3\n");
    assert_eq!(blue_run.stderr, native_run.stderr);

    let native_mappings = decode_map(&native_map);
    let blue_mappings = decode_map(&blue_map);
    let native_tokens = mapped_tokens(&native_javascript, &native_mappings);
    let blue_tokens = mapped_tokens(&artifact.javascript, &blue_mappings);
    assert_eq!(
        alignment::first_ordered_role_delta(&native_tokens, &blue_tokens),
        None,
        "every object-spread source-anchored token must align by role and original position"
    );
    for (compiler, javascript, mappings, expected_prefix, flags) in [
        ("Native", &native_javascript, &native_mappings, 108, None),
        (
            "BlueTSC",
            &artifact.javascript,
            &blue_mappings,
            181,
            Some((", true", ", false")),
        ),
    ] {
        let body_start = javascript.find("var base").unwrap();
        let generated_flags = flags
            .map(|(first, second)| {
                [first, second]
                    .into_iter()
                    .flat_map(|needle| {
                        let comma = javascript.find(needle).expect("target helper flag");
                        [comma, comma + 2]
                    })
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        assert_eq!(generated_flags.len(), if flags.is_some() { 4 } else { 0 });
        let mut prefix_tokens = 0;
        let mut mapped_tokens = 0;
        let mut flag_tokens = 0;
        for token in lex("<object-spread>", javascript).unwrap() {
            if token.kind == TokenKind::Eof {
                continue;
            }
            let expected_mapped =
                token.start >= body_start && !generated_flags.contains(&token.start);
            assert_eq!(
                mapped_source_at(javascript, mappings, token.start).is_some(),
                expected_mapped,
                "{compiler}: object-spread token {:?} has the wrong mapped/unmapped role",
                token.text
            );
            if token.start < body_start {
                prefix_tokens += 1;
            } else if generated_flags.contains(&token.start) {
                assert!(matches!(token.text.as_str(), "," | "true" | "false"));
                flag_tokens += 1;
            } else {
                mapped_tokens += 1;
            }
        }
        assert_eq!(
            prefix_tokens, expected_prefix,
            "{compiler}: generated helper size"
        );
        assert_eq!(
            mapped_tokens, 42,
            "{compiler}: complete source-anchored body"
        );
        assert_eq!(
            flag_tokens,
            generated_flags.len(),
            "{compiler}: helper flags"
        );
    }
    let native_definition = native_javascript.find("var __assign").unwrap();
    let blue_definition = artifact
        .javascript
        .find("function __blueice_target_object_properties")
        .unwrap();
    let native_body = native_javascript.find("var merged").unwrap();
    let blue_body = artifact.javascript.find("var merged").unwrap();
    let native_call = native_body + native_javascript[native_body..].find("__assign(").unwrap();
    let blue_call = blue_body
        + artifact.javascript[blue_body..]
            .find("__blueice_target_object_properties_")
            .unwrap();
    let native_sites = [native_definition, native_call]
        .map(|offset| mapped_source_at(&native_javascript, &native_mappings, offset));
    let blue_sites = [blue_definition, blue_call]
        .map(|offset| mapped_source_at(&artifact.javascript, &blue_mappings, offset));
    fs::remove_dir_all(&directory).unwrap();
    let (line, column) = source_position(source, source.find("merged").unwrap() + "merged".len());
    assert_eq!(native_sites, [None, Some((0, line, column, None))]);
    assert_eq!(blue_sites, native_sites, "object-spread helper frames");
}
