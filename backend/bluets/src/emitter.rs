// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ESM, declaration, and source-map emission from the shared checked graph.

use crate::checker::CheckedProject;
use crate::compiler::{fingerprint, is_declaration_module, CompilerOptions, Project};
use crate::parser::{Declaration, Module, TextEdit, Type};
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
            let javascript = emit_javascript(&checked_module.module);
            let source_map = options
                .source_map
                .then(|| source_map(id, &checked_module.module.source, &javascript));
            let declaration = options
                .declaration
                .then(|| emit_declaration(&checked_module.module));
            (
                id.clone(),
                BuildArtifact {
                    module_id: id.clone(),
                    javascript,
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

fn emit_javascript(module: &Module) -> String {
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

fn apply_edits(source: &str, mut edits: Vec<TextEdit>) -> String {
    edits.sort_by_key(|edit| (edit.start, edit.end));
    let mut emitted = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for edit in edits {
        if edit.start < cursor || edit.end < edit.start || edit.end > source.len() {
            // A nested erase is fully covered by its outer erase.  Parser
            // ownership produces these intentionally for `declare function`.
            continue;
        }
        emitted.push_str(&source[cursor..edit.start]);
        if edit.replacement.is_empty() {
            // Preserve physical lines so line-level source maps, diagnostics,
            // and ordinary text diffs remain stable after type erasure.
            for character in source[edit.start..edit.end].chars() {
                if matches!(character, '\n' | '\r') {
                    emitted.push(character);
                }
            }
        } else {
            emitted.push_str(&edit.replacement);
        }
        cursor = edit.end;
    }
    emitted.push_str(&source[cursor..]);
    emitted
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

fn emit_type_parameters(output: &mut String, parameters: &[String]) {
    if !parameters.is_empty() {
        output.push('<');
        output.push_str(&parameters.join(", "));
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

fn source_map(module_id: &str, source: &str, javascript: &str) -> SourceMap {
    let source_lines = source.lines().count().max(1);
    let output_lines = javascript.lines().count().max(1);
    let mut mappings = String::new();
    let mut previous_source_line = 0i64;
    for generated_line in 0..output_lines {
        if generated_line > 0 {
            mappings.push(';');
        }
        let source_line = generated_line.min(source_lines - 1) as i64;
        mappings.push_str("AA");
        mappings.push_str(&base64_vlq(source_line - previous_source_line));
        mappings.push('A');
        previous_source_line = source_line;
    }
    SourceMap {
        file: output_file_name(module_id),
        sources: vec![module_id.to_string()],
        sources_content: vec![source.to_string()],
        mappings,
    }
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
}
