// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ESM, declaration, and source-map emission from the shared checked graph.

use crate::checker::CheckedProject;
use crate::compiler::{fingerprint, is_declaration_module, CompilerOptions, Project};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{Declaration, Module, TextEdit, Type, TypeParameter};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    pub fingerprint: String,
    pub artifacts: BTreeMap<String, BuildArtifact>,
    /// Root-relative declaration modules preserved for declaration-consuming
    /// builds. They are never JavaScript artifacts.
    pub declaration_modules: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildArtifact {
    pub module_id: String,
    pub javascript: String,
    pub source_map: Option<SourceMap>,
    pub declaration: Option<String>,
    pub fingerprint: String,
}

/// A Source Map v3 payload.  It is kept as structured data until the CLI or a
/// host chooses an artifact location, preventing source-map URLs from leaking
/// filesystem paths into the shared compiler layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    pub file: String,
    pub sources: Vec<String>,
    pub sources_content: Vec<String>,
    pub mappings: String,
}

impl SourceMap {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"version\":3,\"file\":\"{}\",\"sources\":[{}],\"sourcesContent\":[{}],\"names\":[],\"mappings\":\"{}\"}}",
            json_escape(&self.file),
            self.sources
                .iter()
                .map(|source| format!("\"{}\"", json_escape(source)))
                .collect::<Vec<_>>()
                .join(","),
            self.sources_content
                .iter()
                .map(|source| format!("\"{}\"", json_escape(source)))
                .collect::<Vec<_>>()
                .join(","),
            json_escape(&self.mappings),
        )
    }
}

pub(crate) fn emit(
    checked: &CheckedProject,
    project: &Project,
    options: &CompilerOptions,
) -> BuildOutput {
    let build_fingerprint = fingerprint(project, options);
    let artifacts = checked
        .modules
        .iter()
        .filter(|(id, _)| !is_declaration_module(id))
        .map(|(id, checked_module)| {
            let emitted = emit_javascript(&checked_module.module);
            let source_map = options
                .source_map
                .then(|| source_map(id, &checked_module.module.source, &emitted));
            let declaration = options
                .declaration
                .then(|| emit_declaration(&checked_module.module));
            (
                id.clone(),
                BuildArtifact {
                    module_id: id.clone(),
                    javascript: emitted.javascript,
                    source_map,
                    declaration,
                    fingerprint: build_fingerprint.clone(),
                },
            )
        })
        .collect();
    let declaration_modules = if options.declaration {
        {
            checked
                .modules
                .iter()
                .filter(|(id, _)| is_declaration_module(id))
                .map(|(id, checked_module)| (id.clone(), checked_module.module.source.clone()))
                .collect()
        }
    } else {
        BTreeMap::new()
    };
    BuildOutput {
        fingerprint: build_fingerprint,
        artifacts,
        declaration_modules,
    }
}

/// Rejects a source-map request before allocating an unbounded provenance
/// vector. The bound is conservative: erased/replaced spans and physical line
/// boundaries are the only sites that can add a mapping segment.
pub(crate) fn validate_source_map_limits(
    checked: &CheckedProject,
    max_segments: usize,
) -> Vec<Diagnostic> {
    checked
        .modules
        .iter()
        .filter(|(id, _)| !is_declaration_module(id))
        .filter_map(|(id, checked_module)| {
            let module = &checked_module.module;
            let physical_lines = module
                .source
                .chars()
                .filter(|character| matches!(character, '\r' | '\n'))
                .count()
                .saturating_add(1);
            let rewritten_imports = module
                .declarations
                .iter()
                .filter(|declaration| {
                    matches!(declaration, Declaration::Import(import) if !import.type_only)
                })
                .count();
            let bound = physical_lines.saturating_add(
                module
                    .edits
                    .len()
                    .saturating_add(rewritten_imports)
                    .saturating_mul(2),
            );
            (bound > max_segments).then(|| {
                Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    SourceSpan::new(id, 0, 0),
                    format!(
                        "source map requires up to {bound} segments, exceeding the {max_segments} segment limit"
                    ),
                )
            })
        })
        .collect()
}

struct EmittedJavaScript {
    javascript: String,
    provenance: Vec<ProvenanceSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProvenanceSegment {
    generated_line: usize,
    generated_column: usize,
    source_line: usize,
    source_column: usize,
}

fn emit_javascript(module: &Module) -> EmittedJavaScript {
    let mut edits = module.edits.clone();
    for declaration in &module.declarations {
        let Declaration::Import(import) = declaration else {
            continue;
        };
        if import.type_only {
            continue;
        }
        let raw = &module.source[import.specifier_span.start..import.specifier_span.end];
        let Some(quote) = raw.chars().next() else {
            continue;
        };
        let Some(last) = raw.chars().last() else {
            continue;
        };
        if quote != last || !matches!(quote, '\'' | '\"') {
            continue;
        }
        let emitted_specifier = javascript_specifier(&import.specifier);
        edits.push(TextEdit {
            start: import.specifier_span.start,
            end: import.specifier_span.end,
            replacement: format!("{quote}{emitted_specifier}{quote}"),
        });
    }
    apply_edits(&module.source, edits)
}

fn javascript_specifier(specifier: &str) -> String {
    specifier
        .strip_suffix(".tsx")
        .or_else(|| specifier.strip_suffix(".ts"))
        .map(|stem| format!("{stem}.js"))
        .unwrap_or_else(|| specifier.to_string())
}

fn apply_edits(source: &str, mut edits: Vec<TextEdit>) -> EmittedJavaScript {
    edits.sort_by_key(|edit| (edit.start, edit.end));
    let mut emitted = ProvenanceEmitter::new(source, source.len());
    let mut cursor = 0usize;
    for edit in edits {
        if edit.start < cursor || edit.end < edit.start || edit.end > source.len() {
            // A nested erase is fully covered by its outer erase.  Parser
            // ownership produces these intentionally for `declare function`.
            continue;
        }
        emitted.copy(&source[cursor..edit.start], cursor);
        if edit.replacement.is_empty() {
            // Preserve physical lines so line-level source maps, diagnostics,
            // and ordinary text diffs remain stable after type erasure.
            emitted.preserve_line_breaks(&source[edit.start..edit.end], edit.start);
        } else {
            emitted.replace(&edit.replacement, edit.start);
        }
        emitted.mark(edit.end);
        cursor = edit.end;
    }
    emitted.copy(&source[cursor..], cursor);
    emitted.finish()
}

fn emit_declaration(module: &Module) -> String {
    let mut output = String::new();
    for declaration in &module.declarations {
        match declaration {
            Declaration::Import(import) if import.type_only => {
                output.push_str(&module.source[import.span.start..import.span.end]);
                if !output.ends_with('\n') {
                    output.push('\n');
                }
            }
            Declaration::TypeAlias(alias) if alias.exported => {
                output.push_str("export type ");
                output.push_str(&alias.name);
                emit_type_parameters(&mut output, &alias.type_parameters);
                output.push_str(" = ");
                output.push_str(&type_to_ts(&alias.value));
                output.push_str(";\n");
            }
            Declaration::Interface(interface) if interface.exported => {
                output.push_str("export interface ");
                output.push_str(&interface.name);
                emit_type_parameters(&mut output, &interface.type_parameters);
                output.push_str(" {\n");
                for field in &interface.fields {
                    output.push_str("  ");
                    output.push_str(&field.name);
                    if field.optional {
                        output.push('?');
                    }
                    output.push_str(": ");
                    output.push_str(&type_to_ts(&field.value));
                    output.push_str(";\n");
                }
                output.push_str("}\n");
            }
            Declaration::Variable(variable) if variable.exported && !variable.declared => {
                output.push_str("export declare const ");
                output.push_str(&variable.name);
                output.push_str(": ");
                output.push_str(
                    &variable
                        .annotation
                        .as_ref()
                        .map(type_to_ts)
                        .unwrap_or_else(|| "unknown".to_string()),
                );
                output.push_str(";\n");
            }
            Declaration::Function(function) if function.exported && !function.declared => {
                output.push_str("export declare function ");
                output.push_str(&function.name);
                emit_type_parameters(&mut output, &function.type_parameters);
                output.push('(');
                for (index, parameter) in function.parameters.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&parameter.name);
                    if parameter.optional {
                        output.push('?');
                    }
                    output.push_str(": ");
                    output.push_str(
                        &parameter
                            .annotation
                            .as_ref()
                            .map(type_to_ts)
                            .unwrap_or_else(|| "unknown".to_string()),
                    );
                }
                output.push_str("): ");
                output.push_str(
                    &function
                        .return_type
                        .as_ref()
                        .map(type_to_ts)
                        .unwrap_or_else(|| "unknown".to_string()),
                );
                output.push_str(";\n");
            }
            Declaration::TypeExport(export) => {
                output.push_str("export type ");
                if export.bindings.len() == 1 && export.bindings[0] == "*" {
                    output.push('*');
                } else {
                    output.push_str("{ ");
                    output.push_str(&export.bindings.join(", "));
                    output.push_str(" }");
                }
                if let Some(specifier) = &export.specifier {
                    output.push_str(" from ");
                    output.push_str(&format!("\"{specifier}\""));
                }
                output.push_str(";\n");
            }
            _ => {}
        }
    }
    output
}

fn emit_type_parameters(output: &mut String, parameters: &[TypeParameter]) {
    if !parameters.is_empty() {
        output.push('<');
        for (index, parameter) in parameters.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            output.push_str(&parameter.name);
            if let Some(constraint) = &parameter.constraint {
                output.push_str(" extends ");
                output.push_str(&type_to_ts(constraint));
            }
            if let Some(default) = &parameter.default {
                output.push_str(" = ");
                output.push_str(&type_to_ts(default));
            }
        }
        output.push('>');
    }
}

fn type_to_ts(value: &Type) -> String {
    match value {
        Type::Any => "any".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Never => "never".to_string(),
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::Undefined => "undefined".to_string(),
        Type::Boolean => "boolean".to_string(),
        Type::Number => "number".to_string(),
        Type::String => "string".to_string(),
        Type::Literal(value) => value.clone(),
        Type::Named { name, arguments } if arguments.is_empty() => name.clone(),
        Type::Named { name, arguments } => format!(
            "{name}<{}>",
            arguments
                .iter()
                .map(type_to_ts)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Array(value) => format!("{}[]", type_to_ts(value)),
        Type::Tuple(values) => format!(
            "[{}]",
            values.iter().map(type_to_ts).collect::<Vec<_>>().join(", ")
        ),
        Type::Record(fields) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|field| format!(
                    "{}{}: {}",
                    field.name,
                    if field.optional { "?" } else { "" },
                    type_to_ts(&field.value)
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        Type::Union(values) => values
            .iter()
            .map(type_to_ts)
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Intersection(values) => values
            .iter()
            .map(type_to_ts)
            .collect::<Vec<_>>()
            .join(" & "),
    }
}

struct ProvenanceEmitter<'a> {
    source: &'a str,
    javascript: String,
    provenance: Vec<ProvenanceSegment>,
    generated_line: usize,
    generated_column: usize,
}

impl<'a> ProvenanceEmitter<'a> {
    fn new(source: &'a str, capacity: usize) -> Self {
        Self {
            source,
            javascript: String::with_capacity(capacity),
            provenance: Vec::new(),
            generated_line: 0,
            generated_column: 0,
        }
    }

    fn copy(&mut self, value: &str, source_offset: usize) {
        self.mark(source_offset);
        let mut characters = value.char_indices().peekable();
        while let Some((offset, character)) = characters.next() {
            let next = characters.peek().map(|(_, character)| *character);
            self.push(character, next);
            if is_line_break(character, next) {
                self.mark(source_offset + offset + character.len_utf8());
            }
        }
    }

    fn preserve_line_breaks(&mut self, value: &str, source_offset: usize) {
        let mut characters = value.char_indices().peekable();
        while let Some((offset, character)) = characters.next() {
            let next = characters.peek().map(|(_, character)| *character);
            if matches!(character, '\r' | '\n') {
                self.push(character, next);
                if is_line_break(character, next) {
                    self.mark(source_offset + offset + character.len_utf8());
                }
            }
        }
    }

    fn replace(&mut self, value: &str, source_offset: usize) {
        self.mark(source_offset);
        let mut characters = value.chars().peekable();
        while let Some(character) = characters.next() {
            self.push(character, characters.peek().copied());
        }
    }

    fn mark(&mut self, source_offset: usize) {
        let (source_line, source_column) = source_line_column(self.source, source_offset);
        let segment = ProvenanceSegment {
            generated_line: self.generated_line,
            generated_column: self.generated_column,
            source_line,
            source_column,
        };
        if self.provenance.last().is_some_and(|previous| {
            previous.generated_line == segment.generated_line
                && previous.generated_column == segment.generated_column
        }) {
            self.provenance.pop();
        }
        self.provenance.push(segment);
    }

    fn push(&mut self, character: char, next: Option<char>) {
        self.javascript.push(character);
        if is_line_break(character, next) {
            self.generated_line += 1;
            self.generated_column = 0;
        } else {
            self.generated_column += character.len_utf16();
        }
    }

    fn finish(self) -> EmittedJavaScript {
        EmittedJavaScript {
            javascript: self.javascript,
            provenance: self.provenance,
        }
    }
}

fn is_line_break(character: char, next: Option<char>) -> bool {
    character == '\n' || (character == '\r' && next != Some('\n'))
}

fn source_line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    let mut line = 0usize;
    let mut column = 0usize;
    let mut characters = prefix.chars().peekable();
    while let Some(character) = characters.next() {
        if is_line_break(character, characters.peek().copied()) {
            line += 1;
            column = 0;
        } else {
            column += character.len_utf16();
        }
    }
    (line, column)
}

fn source_map(module_id: &str, source: &str, emitted: &EmittedJavaScript) -> SourceMap {
    SourceMap {
        file: output_file_name(module_id),
        sources: vec![module_id.to_string()],
        sources_content: vec![source.to_string()],
        mappings: encode_mappings(&emitted.provenance),
    }
}

fn encode_mappings(segments: &[ProvenanceSegment]) -> String {
    let last_line = segments
        .last()
        .map(|segment| segment.generated_line)
        .unwrap_or(0);
    let mut mappings = String::new();
    let mut index = 0usize;
    let mut previous_source_line = 0i64;
    let mut previous_source_column = 0i64;
    for line in 0..=last_line {
        if line > 0 {
            mappings.push(';');
        }
        let mut previous_generated_column = 0i64;
        let mut first = true;
        while let Some(segment) = segments
            .get(index)
            .filter(|segment| segment.generated_line == line)
        {
            if !first {
                mappings.push(',');
            }
            first = false;
            mappings.push_str(&base64_vlq(
                segment.generated_column as i64 - previous_generated_column,
            ));
            mappings.push('A');
            mappings.push_str(&base64_vlq(
                segment.source_line as i64 - previous_source_line,
            ));
            mappings.push_str(&base64_vlq(
                segment.source_column as i64 - previous_source_column,
            ));
            previous_generated_column = segment.generated_column as i64;
            previous_source_line = segment.source_line as i64;
            previous_source_column = segment.source_column as i64;
            index += 1;
        }
    }
    mappings
}

fn output_file_name(module_id: &str) -> String {
    module_id
        .rsplit('/')
        .next()
        .unwrap_or(module_id)
        .strip_suffix(".tsx")
        .or_else(|| {
            module_id
                .rsplit('/')
                .next()
                .unwrap_or(module_id)
                .strip_suffix(".ts")
        })
        .map(|stem| format!("{stem}.js"))
        .unwrap_or_else(|| format!("{module_id}.js"))
}

fn base64_vlq(value: i64) -> String {
    const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    let mut value = if value < 0 {
        ((-value) as u64) << 1 | 1
    } else {
        (value as u64) << 1
    };
    loop {
        let mut digit = (value & 0b1_1111) as usize;
        value >>= 5;
        if value != 0 {
            digit |= 0b10_0000;
        }
        encoded.push(BASE64[digit] as char);
        if value == 0 {
            break;
        }
    }
    encoded
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use crate::{compile, CompilerOptions, MapLoader, ModuleSource};

    use super::source_line_column;

    #[test]
    fn erases_types_and_rewrites_typescript_module_specifiers() {
        let loader = MapLoader::from([
            ModuleSource::new(
                "memory:///main.ts",
                "import { add } from './math.ts'; export const total: number = add(1, 2);",
            ),
            ModuleSource::new(
                "memory:///math.ts",
                "export function add(left: number, right: number): number { return left; }",
            ),
        ]);
        let output = compile("memory:///main.ts", &loader, CompilerOptions::default())
            .output
            .unwrap();
        let javascript = &output.artifacts["memory:///main.ts"].javascript;
        assert!(javascript.contains("'./math.js'"));
        assert!(javascript.contains("const total="));
        assert!(!javascript.contains(": number"));
    }

    #[test]
    fn emits_source_map_and_public_declaration() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///api.ts",
            "export interface User { id: string }\nexport function greeting(user: User): string { return 'hi'; }",
        )]);
        let output = compile(
            "memory:///api.ts",
            &loader,
            CompilerOptions {
                source_map: true,
                declaration: true,
                ..CompilerOptions::default()
            },
        )
        .output
        .unwrap();
        let artifact = &output.artifacts["memory:///api.ts"];
        assert!(artifact
            .source_map
            .as_ref()
            .unwrap()
            .to_json()
            .contains("\"version\":3"));
        assert!(artifact
            .declaration
            .as_ref()
            .unwrap()
            .contains("export interface User"));
    }

    #[test]
    fn source_map_tracks_columns_across_erased_annotations() {
        let source = "export const label: string = 'value';";
        let loader = MapLoader::from([ModuleSource::new("memory:///columns.ts", source)]);
        let output = compile(
            "memory:///columns.ts",
            &loader,
            CompilerOptions {
                source_map: true,
                ..CompilerOptions::default()
            },
        )
        .output
        .unwrap();
        let artifact = &output.artifacts["memory:///columns.ts"];
        let generated_equals = artifact.javascript.find('=').unwrap();
        let source_equals = source.find('=').unwrap();
        let expected_generated_column = artifact.javascript[..generated_equals]
            .encode_utf16()
            .count();
        let expected_source_column = source[..source_equals].encode_utf16().count();
        let mappings = decode_mappings(&artifact.source_map.as_ref().unwrap().mappings);
        assert!(mappings.iter().any(|mapping| {
            *mapping == (0, expected_generated_column, 0, expected_source_column)
        }));
    }

    #[test]
    fn provenance_uses_utf16_columns_and_normalizes_crlf_to_one_line_break() {
        let source = "const 值 = 1;\r\nconst next = 2;";
        let first_value = source.find('1').unwrap();
        let second_line = source.rfind("next").unwrap();
        assert_eq!(
            source_line_column(source, first_value),
            (0, "const 值 = ".encode_utf16().count())
        );
        assert_eq!(source_line_column(source, second_line), (1, 6));
    }

    #[test]
    fn emits_an_erasable_generic_function_without_a_javascript_type_parameter() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///generic.ts",
            "export function identity<T>(value: T): T { return value; }",
        )]);
        let output = compile("memory:///generic.ts", &loader, CompilerOptions::default())
            .output
            .unwrap();
        let javascript = &output.artifacts["memory:///generic.ts"].javascript;
        assert!(javascript.contains("function identity(value)"));
        assert!(!javascript.contains("<T>"));
    }

    #[test]
    fn erases_explicit_generic_arguments_from_direct_calls() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///generic-call.ts",
            "function identity<T extends string>(value: T): T { return value; }\n\
             console.log(identity<string>('Ada'));",
        )]);
        let output = compile(
            "memory:///generic-call.ts",
            &loader,
            CompilerOptions::default(),
        )
        .output
        .unwrap();
        let javascript = &output.artifacts["memory:///generic-call.ts"].javascript;
        assert!(javascript.contains("identity('Ada')"), "{javascript}");
        assert!(!javascript.contains("identity<string>"), "{javascript}");
    }

    #[test]
    fn retains_generic_constraints_and_defaults_in_declaration_output_only() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///generic.ts",
            "export interface Box<T extends string = string> { value: T }\n\
             export function echo<T extends string = string>(value?: T): T { return value; }",
        )]);
        let output = compile(
            "memory:///generic.ts",
            &loader,
            CompilerOptions {
                declaration: true,
                ..CompilerOptions::default()
            },
        )
        .output
        .unwrap();
        let artifact = &output.artifacts["memory:///generic.ts"];
        assert!(
            artifact.javascript.contains("function echo(value)"),
            "{}",
            artifact.javascript
        );
        assert!(!artifact.javascript.contains("extends string"));
        assert_eq!(
            artifact.declaration.as_deref(),
            Some(
                "export interface Box<T extends string = string> {\n  value: T;\n}\n\
                 export declare function echo<T extends string = string>(value?: T): T;\n"
            )
        );
    }

    #[test]
    fn erases_optional_parameter_markers_but_preserves_default_initializers() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///optional.ts",
            "export function count(value: number = 2, multiplier?: number): number { return value; }",
        )]);
        let output = compile("memory:///optional.ts", &loader, CompilerOptions::default())
            .output
            .unwrap();
        let javascript = &output.artifacts["memory:///optional.ts"].javascript;
        assert!(javascript.contains("function count(value= 2, multiplier)"));
        assert!(!javascript.contains("?:"));
        assert!(!javascript.contains("multiplier?"));
    }

    #[test]
    fn rejects_source_maps_that_exceed_the_segment_budget() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///limited.ts",
            "const label: string = 'BlueIce';\n",
        )]);
        let result = compile(
            "memory:///limited.ts",
            &loader,
            CompilerOptions {
                source_map: true,
                limits: crate::CompilerLimits {
                    max_source_map_segments: 1,
                    ..crate::CompilerLimits::default()
                },
                ..CompilerOptions::default()
            },
        );
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == crate::DiagnosticCode::ResourceLimit
                && diagnostic.message.contains("segment limit")
        }));
        assert!(result.output.is_none());
    }

    #[test]
    fn preserves_a_type_only_reexport_in_declarations_but_not_javascript() {
        let loader = MapLoader::from([
            ModuleSource::new(
                "memory:///api.ts",
                "export type { User } from './model.ts';",
            ),
            ModuleSource::new("memory:///model.ts", "export interface User { id: string }"),
        ]);
        let output = compile(
            "memory:///api.ts",
            &loader,
            CompilerOptions {
                declaration: true,
                ..CompilerOptions::default()
            },
        )
        .output
        .unwrap();
        let artifact = &output.artifacts["memory:///api.ts"];
        assert!(artifact.javascript.trim().is_empty());
        assert_eq!(
            artifact.declaration.as_deref(),
            Some("export type { User } from \"./model.ts\";\n")
        );
    }

    fn decode_mappings(value: &str) -> Vec<(usize, usize, usize, usize)> {
        let mut mappings = Vec::new();
        let mut source_index = 0i64;
        let mut source_line = 0i64;
        let mut source_column = 0i64;
        for (generated_line, line) in value.split(';').enumerate() {
            let mut generated_column = 0i64;
            for segment in line.split(',').filter(|segment| !segment.is_empty()) {
                let values = decode_vlq_fields(segment);
                assert_eq!(values.len(), 4);
                generated_column += values[0];
                source_index += values[1];
                source_line += values[2];
                source_column += values[3];
                assert_eq!(source_index, 0);
                mappings.push((
                    generated_line,
                    generated_column as usize,
                    source_line as usize,
                    source_column as usize,
                ));
            }
        }
        mappings
    }

    fn decode_vlq_fields(value: &str) -> Vec<i64> {
        let bytes = value.as_bytes();
        let mut fields = Vec::new();
        let mut index = 0usize;
        while index < bytes.len() {
            let mut value = 0u64;
            let mut shift = 0u32;
            loop {
                let digit = base64_value(bytes[index]);
                index += 1;
                value |= u64::from(digit & 0b1_1111) << shift;
                shift += 5;
                if digit & 0b10_0000 == 0 {
                    break;
                }
            }
            let negative = value & 1 == 1;
            let value = (value >> 1) as i64;
            fields.push(if negative { -value } else { value });
        }
        fields
    }

    fn base64_value(value: u8) -> u8 {
        match value {
            b'A'..=b'Z' => value - b'A',
            b'a'..=b'z' => value - b'a' + 26,
            b'0'..=b'9' => value - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 VLQ digit"),
        }
    }
}
