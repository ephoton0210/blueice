// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned helper-frame oracle for legacy parameter decorators and metadata.

use super::*;

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn legacy_parameter_and_metadata_frames_match_native() {
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

    let source = include_str!("../fixtures/k11_source_maps/legacy_parameter_metadata.ts");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "blueice-k11-legacy-metadata-{}-{nonce}",
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
            "ES2022",
            "--strict",
            "--pretty",
            "false",
            "--experimentalDecorators",
            "--emitDecoratorMetadata",
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
            experimental_decorators: true,
            emit_decorator_metadata: true,
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
            experimental_decorators: true,
            emit_decorator_metadata: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!plain.has_errors(), "{:#?}", plain.diagnostics);
    assert_eq!(
        artifact.javascript,
        plain.output.unwrap().artifacts[module].javascript,
        "legacy metadata map markers must not change JavaScript"
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
    assert_eq!(blue_run.stdout, b"parameter 0\nmethod\n42\n");
    assert_eq!(blue_run.stderr, native_run.stderr);

    let sites = |javascript: &str, decorate: &str, param: &str, metadata: &str| {
        let body = javascript.find("class Example").unwrap();
        let call = body + javascript[body..].find(&format!("{decorate}([")).unwrap();
        let param_call = call + javascript[call..].find(&format!("{param}(0")).unwrap();
        let metadata_call = call + javascript[call..].find(&format!("{metadata}(")).unwrap();
        [
            javascript.find(&format!("var {decorate}")).unwrap(),
            javascript.find(&format!("var {param}")).unwrap(),
            javascript.find(&format!("var {metadata}")).unwrap(),
            call,
            param_call,
            metadata_call,
            param_call + javascript[param_call..].find("parameterDecor").unwrap(),
            call + javascript[call..].find("], Example.prototype").unwrap(),
        ]
    };
    let native_mappings = decode_map(&native_map);
    let blue_mappings = decode_map(&blue_map);
    let native_sites = sites(&native_javascript, "__decorate", "__param", "__metadata")
        .map(|offset| mapped_source_at(&native_javascript, &native_mappings, offset));
    let blue_sites = sites(
        &artifact.javascript,
        "__bluetsDecorate",
        "__bluetsParam",
        "__bluetsMetadata",
    )
    .map(|offset| mapped_source_at(&artifact.javascript, &blue_mappings, offset));
    fs::remove_dir_all(&directory).unwrap();
    let source_site = |offset| {
        let (line, column) = source_position(source, offset);
        Some((0, line, column, None))
    };
    assert_eq!(
        native_sites,
        [
            None,
            None,
            None,
            source_site(source.find("method(@").unwrap()),
            source_site(source.find("@parameterDecor").unwrap() + 1),
            None,
            source_site(source.find("@parameterDecor").unwrap() + 1),
            None,
        ],
        "pinned TypeScript parameter/metadata sites"
    );
    assert_eq!(blue_sites, native_sites, "legacy parameter/metadata frames");
}
