// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, bounded lookup for debugger-facing generated JavaScript positions.

use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    SourceMapLookupError, SourceMapOriginalPosition,
};

fn position(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

fn lookup(
    map: &blueice_bluets::SourceMap,
    javascript: &str,
    offset: usize,
) -> Option<SourceMapOriginalPosition> {
    let (line, column) = position(javascript, offset);
    map.original_position_at(javascript, line, column, 100_000, 16_000_000)
        .unwrap()
}

#[test]
fn public_lookup_retains_relocated_decorator_and_hides_generated_helper_frames() {
    let source = include_str!("fixtures/k11_source_maps/decorated.ts");
    let module = "memory:///main.ts";
    let result = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts[module];
    let javascript = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    let relocated = javascript.find("_method_decorators = [decoSite]").unwrap()
        + "_method_decorators = [".len();
    let (line, column) = position(source, source.find("@decoSite").unwrap() + 1);
    assert_eq!(
        lookup(map, javascript, relocated),
        Some(SourceMapOriginalPosition {
            source_index: 0,
            line,
            column_utf16: column,
            name_index: None,
        })
    );
    for generated in ["var __bluetsEsDecorate", "let _instanceExtraInitializers"] {
        assert_eq!(
            lookup(map, javascript, javascript.find(generated).unwrap()),
            None,
            "{generated}: synthetic helper/scaffold must not acquire user provenance"
        );
    }
}

#[test]
fn public_lookup_validates_the_whole_stream_and_bounds_work() {
    let module = "memory:///main.ts";
    let source = "const label: string = '😀';\n";
    let result = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, source)]),
        CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts[module];
    let javascript = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    let string = javascript.find("'😀'").unwrap();
    let (generated_line, generated_column) = position(javascript, string);
    let (source_line, source_column) = position(source, source.find("'😀'").unwrap());
    assert_eq!(
        map.original_position_at(
            javascript,
            generated_line,
            generated_column,
            100_000,
            16_000_000
        )
        .unwrap(),
        Some(SourceMapOriginalPosition {
            source_index: 0,
            line: source_line,
            column_utf16: source_column,
            name_index: None,
        })
    );
    assert_eq!(
        map.original_position_at(javascript, generated_line, generated_column, 0, 16_000_000),
        Err(SourceMapLookupError::SegmentLimit)
    );
    assert_eq!(
        map.original_position_at(javascript, generated_line, generated_column, 100_000, 1),
        Err(SourceMapLookupError::InputLimit)
    );
    let mut many_empty_names = map.clone();
    many_empty_names.names = vec![String::new(); 1024];
    assert_eq!(
        many_empty_names.original_position_at(
            javascript,
            generated_line,
            generated_column,
            100_000,
            100
        ),
        Err(SourceMapLookupError::InputLimit),
        "empty table entries must still consume the input budget"
    );
    assert_eq!(
        map.original_position_at(
            javascript,
            javascript.lines().count() + 1,
            0,
            100_000,
            16_000_000
        ),
        Err(SourceMapLookupError::QueryOutOfBounds)
    );
    let mut malformed = map.clone();
    malformed.mappings.push_str(";!");
    assert_eq!(
        malformed.original_position_at(
            javascript,
            generated_line,
            generated_column,
            100_000,
            16_000_000
        ),
        Err(SourceMapLookupError::MalformedMap),
        "an early lookup cannot ignore a malformed later segment"
    );
    malformed = map.clone();
    malformed.sources_content.clear();
    assert_eq!(
        malformed.original_position_at(
            javascript,
            generated_line,
            generated_column,
            100_000,
            16_000_000
        ),
        Err(SourceMapLookupError::MalformedMap)
    );
    malformed = map.clone();
    malformed
        .mappings
        .push_str(&";".repeat(javascript.lines().count() + 2));
    assert_eq!(
        malformed.original_position_at(
            javascript,
            generated_line,
            generated_column,
            100_000,
            16_000_000
        ),
        Err(SourceMapLookupError::MalformedMap),
        "empty mapping lines beyond generated JavaScript must be rejected"
    );
}

#[test]
fn public_lookup_distinguishes_legacy_metadata_and_parameter_helper_sites() {
    let source = include_str!("fixtures/k11_source_maps/legacy_parameter_metadata.ts");
    let module = "memory:///main.ts";
    let result = compile(
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
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let output = result.output.unwrap();
    let artifact = &output.artifacts[module];
    let javascript = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    for definition in [
        "var __bluetsDecorate",
        "var __bluetsParam",
        "var __bluetsMetadata",
    ] {
        assert_eq!(
            lookup(map, javascript, javascript.find(definition).unwrap()),
            None,
            "{definition} is generated-only"
        );
    }
    let body = javascript.find("class Example").unwrap();
    let call = body + javascript[body..].find("__bluetsDecorate([").unwrap();
    let parameter_call = call + javascript[call..].find("__bluetsParam(0").unwrap();
    let (line, column_utf16) = position(source, source.find("@parameterDecor").unwrap() + 1);
    assert_eq!(
        lookup(map, javascript, parameter_call),
        Some(SourceMapOriginalPosition {
            source_index: 0,
            line,
            column_utf16,
            name_index: None,
        })
    );
    let metadata_call = call + javascript[call..].find("__bluetsMetadata(").unwrap();
    assert_eq!(lookup(map, javascript, metadata_call), None);
}

#[test]
fn public_lookup_keeps_non_decorator_helpers_and_system_scaffolding_unmapped() {
    let module = "memory:///main.ts";
    let spread_source = include_str!("fixtures/k11_source_maps/object_spread_helper.ts");
    let spread = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, spread_source)]),
        CompilerOptions {
            target: EcmaTarget::Es5,
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!spread.has_errors(), "{:#?}", spread.diagnostics);
    let spread_output = spread.output.unwrap();
    let artifact = &spread_output.artifacts[module];
    let javascript = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    let definition = javascript
        .find("function __blueice_target_object_properties")
        .unwrap();
    assert_eq!(lookup(map, javascript, definition), None);
    let body = javascript.find("var merged").unwrap();
    let call = body
        + javascript[body..]
            .find("__blueice_target_object_properties_")
            .unwrap();
    let (line, column_utf16) = position(
        spread_source,
        spread_source.find("merged").unwrap() + "merged".len(),
    );
    assert_eq!(
        lookup(map, javascript, call),
        Some(SourceMapOriginalPosition {
            source_index: 0,
            line,
            column_utf16,
            name_index: None,
        })
    );

    let system_source = include_str!("fixtures/k11_source_maps/system_wrapper.ts");
    let system = compile(
        module,
        &MapLoader::from([ModuleSource::new(module, system_source)]),
        CompilerOptions {
            module_kind: ModuleKind::System,
            source_map: true,
            inline_sources: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!system.has_errors(), "{:#?}", system.diagnostics);
    let system_output = system.output.unwrap();
    let artifact = &system_output.artifacts[module];
    let javascript = &artifact.javascript;
    let map = artifact.source_map.as_ref().unwrap();
    assert_eq!(
        lookup(map, javascript, javascript.find("System.register").unwrap()),
        None
    );
    let (line, column_utf16) = position(system_source, system_source.find("console").unwrap());
    assert_eq!(
        lookup(map, javascript, javascript.find("console.log").unwrap()),
        Some(SourceMapOriginalPosition {
            source_index: 0,
            line,
            column_utf16,
            name_index: None,
        })
    );
}
