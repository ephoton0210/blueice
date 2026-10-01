// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ESM, declaration, and source-map emission from the shared checked graph.

use crate::checker::CheckedProject;
use crate::compiler::{fingerprint, is_declaration_module, CompilerOptions, Project};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{Declaration, Module, TextEdit, Type, TypeParameter};
use crate::strict_boundaries;
use std::collections::BTreeMap;

pub use private_lowering::CLASS_HELPER_V1_VERSION;

mod class_lowering;
mod classes;
mod enums;
mod private_lowering;

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
    /// Present only when every admitted strict crossing has an emitted helper
    /// call. Publishers must verify these byte locations before release.
    pub strict_runtime: Option<EmittedStrictModule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedStrictModule {
    pub helper_version: String,
    pub helper_import: EmittedRuntimeSite,
    pub boundaries: Vec<EmittedRuntimeBoundary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedRuntimeBoundary {
    pub contract_id: String,
    pub function: String,
    pub source_span: SourceSpan,
    pub max_string_bytes: usize,
    pub ingress: Vec<EmittedRuntimeSite>,
    pub egress: EmittedRuntimeSite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedRuntimeSite {
    pub source_span: SourceSpan,
    pub generated_start: usize,
    pub expected_text: String,
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
) -> Result<BuildOutput, Diagnostic> {
    let build_fingerprint = fingerprint(project, options);
    let exported_enums = crate::checker::exported_enums(project);
    let artifacts = checked
        .modules
        .iter()
        .filter(|(id, _)| !is_declaration_module(id))
        .map(|(id, checked_module)| {
            let (emitted, strict_runtime) =
                emit_javascript(&checked_module.module, project, &exported_enums, options)?;
            if options.source_map
                && emitted.provenance.len() > options.limits.max_source_map_segments
            {
                return Err(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    SourceSpan::new(id, 0, 0),
                    "emitted source map exceeded the segment limit",
                ));
            }
            let source_map = options
                .source_map
                .then(|| source_map(id, &checked_module.module.source, &emitted));
            let declaration = options
                .declaration
                .then(|| emit_declaration(&checked_module.module))
                .transpose()?;
            Ok((
                id.clone(),
                BuildArtifact {
                    module_id: id.clone(),
                    javascript: emitted.javascript,
                    source_map,
                    declaration,
                    fingerprint: build_fingerprint.clone(),
                    strict_runtime,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, Diagnostic>>()?;
    let declaration_modules = if options.declaration {
        {
            checked
                .modules
                .iter()
                .filter(|(id, _)| {
                    is_declaration_module(id) && !project.is_ambient_declaration_module(id)
                })
                .map(|(id, checked_module)| (id.clone(), checked_module.module.source.clone()))
                .collect()
        }
    } else {
        BTreeMap::new()
    };
    Ok(BuildOutput {
        fingerprint: build_fingerprint,
        artifacts,
        declaration_modules,
    })
}

/// Rejects a source-map request before allocating an unbounded provenance
/// vector. The bound is conservative: erased/replaced spans and physical line
/// boundaries are the only sites that can add a mapping segment.
pub(crate) fn validate_source_map_limits(
    checked: &CheckedProject,
    options: &CompilerOptions,
) -> Vec<Diagnostic> {
    let max_segments = options.limits.max_source_map_segments;
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
            let strict_functions = module
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    Declaration::Function(function) if options.strict_runtime_boundaries.iter().any(
                        |boundary| boundary.span == function.span && boundary.function == function.name,
                    ) => Some(function.parameters.len()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let strict_edits = if strict_functions.is_empty() {
                0
            } else {
                1usize.saturating_add(strict_functions.len().saturating_mul(3))
            };
            let inserted_line_breaks = if strict_functions.is_empty() {
                0
            } else {
                1usize.saturating_add(strict_functions.iter().sum::<usize>())
            };
            let bound = physical_lines.saturating_add(
                module
                    .edits
                    .len()
                    .saturating_add(rewritten_imports)
                    .saturating_add(strict_edits)
                    .saturating_mul(2),
            ).saturating_add(inserted_line_breaks);
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

fn emit_javascript(
    module: &Module,
    project: &Project,
    exported_enums: &BTreeMap<String, BTreeMap<String, crate::checker::ExportedEnum>>,
    options: &CompilerOptions,
) -> Result<(EmittedJavaScript, Option<EmittedStrictModule>), Diagnostic> {
    if let Some(Declaration::Namespace(namespace)) = module
        .declarations
        .iter()
        .find(|declaration| matches!(declaration, Declaration::Namespace(_)))
    {
        return Err(Diagnostic::error(
            DiagnosticCode::UnsupportedSyntax,
            namespace.span.clone(),
            "a namespace is not emitted yet",
        ));
    }
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
    edits.extend(classes::overload_signature_erasures(module));
    class_lowering::lower_class_members(module, options, &mut edits)?;
    enums::lower_enums(module, project, exported_enums, options, &mut edits)?;
    let plan = strict_boundaries::plan_emission(module, options)?;
    let mut strict_runtime = None;
    if let Some((strict_edits, record)) = plan {
        edits.extend(strict_edits);
        strict_runtime = Some(record);
    }
    let emitted = apply_edits(&module.source, edits);
    if let Some(record) = &mut strict_runtime {
        strict_boundaries::locate_emitted_calls(module, &emitted.javascript, record)?;
    }
    Ok((emitted, strict_runtime))
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

fn emit_declaration(module: &Module) -> Result<String, Diagnostic> {
    let mut output = String::new();
    let enum_evaluations = crate::enum_eval::evaluate_enums(module);
    let mut enum_position = 0usize;
    for declaration in &module.declarations {
        let enum_index = enum_position;
        if matches!(declaration, Declaration::Enum(_)) {
            enum_position += 1;
        }
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
                if !interface.heritage.is_empty() {
                    output.push_str(" extends ");
                    output.push_str(
                        &interface
                            .heritage
                            .iter()
                            .map(type_to_ts)
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                }
                output.push_str(" {\n");
                for field in &interface.fields {
                    output.push_str("  ");
                    if field.readonly {
                        output.push_str("readonly ");
                    }
                    output.push_str(&field.name);
                    if field.optional {
                        output.push('?');
                    }
                    if let Type::Function { parameters, result } = &field.value {
                        output.push_str(&method_signature_to_ts(parameters, result));
                    } else {
                        output.push_str(": ");
                        output.push_str(&type_to_ts(&field.value));
                    }
                    output.push_str(";\n");
                }
                output.push_str("}\n");
            }
            Declaration::Variable(variable)
                if !variable.declared
                    && (variable.exported
                        || is_default_export_name(module, &variable.name)
                        || is_value_export_name(module, &variable.name)) =>
            {
                if variable.exported {
                    output.push_str("export declare ");
                } else {
                    output.push_str("declare ");
                }
                output.push_str(variable.kind.as_str());
                output.push(' ');
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
            Declaration::Function(function)
                if (function.exported
                    || is_default_export_name(module, &function.name)
                    || is_value_export_name(module, &function.name))
                    && (function.overload
                        || !module.declarations.iter().any(|declaration| {
                            matches!(
                                declaration,
                                Declaration::Function(other)
                                    if other.name == function.name && other.overload
                            )
                        })) =>
            {
                if function.default_export {
                    output.push_str("export default function ");
                } else if function.exported {
                    output.push_str("export declare function ");
                } else {
                    output.push_str("declare function ");
                }
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
            Declaration::Enum(declaration)
                if declaration.exported || is_value_export_name(module, &declaration.name) =>
            {
                let evaluation = enum_evaluations
                    .get(enum_index)
                    .expect("every enum was evaluated");
                output.push_str(&enums::emit_enum_declaration(declaration, evaluation));
            }
            Declaration::Class(class)
                if class.exported
                    || is_default_export_name(module, &class.name)
                    || is_value_export_name(module, &class.name) =>
            {
                let prefix = if class.exported {
                    "export declare "
                } else {
                    "declare "
                };
                classes::emit_class_declaration(class, prefix, &mut output)?;
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
            Declaration::DefaultExport(export) => {
                output.push_str("export default ");
                output.push_str(&export.name);
                output.push_str(";\n");
            }
            Declaration::ValueExport(export) => {
                output.push_str("export { ");
                for (index, binding) in export.bindings.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&binding.local);
                    if binding.local != binding.exported {
                        output.push_str(" as ");
                        output.push_str(&binding.exported);
                    }
                }
                output.push_str(" };\n");
            }
            _ => {}
        }
    }
    Ok(output)
}

fn is_default_export_name(module: &Module, name: &str) -> bool {
    module.declarations.iter().any(|declaration| {
        matches!(declaration, Declaration::DefaultExport(export) if export.name == name)
    })
}

fn is_value_export_name(module: &Module, name: &str) -> bool {
    module.declarations.iter().any(|declaration| {
        matches!(declaration, Declaration::ValueExport(export) if export.bindings.iter().any(|binding| binding.local == name))
    })
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
        Type::Array(value) => value.array_element_text(type_to_ts),
        Type::Tuple(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| {
                    if let Some(label) = &value.label {
                        format!(
                            "{}{label}{}: {}",
                            if value.rest { "..." } else { "" },
                            if value.optional { "?" } else { "" },
                            type_to_ts(&value.annotation)
                        )
                    } else {
                        format!(
                            "{}{}{}",
                            if value.rest { "..." } else { "" },
                            type_to_ts(&value.annotation),
                            if value.optional { "?" } else { "" }
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Record(fields) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|field| {
                    let name = format!(
                        "{}{}{}",
                        if field.readonly { "readonly " } else { "" },
                        field.name,
                        if field.optional { "?" } else { "" }
                    );
                    if let Type::Function { parameters, result } = &field.value {
                        format!("{name}{}", method_signature_to_ts(parameters, result))
                    } else {
                        format!("{name}: {}", type_to_ts(&field.value))
                    }
                })
                .collect::<Vec<_>>()
                .join("; ")
        ),
        Type::Function { parameters, result } => {
            format!(
                "{} => {}",
                method_parameters_to_ts(parameters),
                type_to_ts(result)
            )
        }
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

fn method_parameters_to_ts(parameters: &[crate::parser::Parameter]) -> String {
    let members = parameters
        .iter()
        .map(|parameter| {
            format!(
                "{}{}: {}",
                parameter.name,
                if parameter.optional { "?" } else { "" },
                parameter
                    .annotation
                    .as_ref()
                    .map(type_to_ts)
                    .unwrap_or_else(|| "unknown".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("({members})")
}

fn method_signature_to_ts(parameters: &[crate::parser::Parameter], result: &Type) -> String {
    format!(
        "{}: {}",
        method_parameters_to_ts(parameters),
        type_to_ts(result)
    )
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
            let next = characters.peek().copied();
            self.push(character, next);
            if is_line_break(character, next) {
                self.mark(source_offset);
            }
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
#[path = "emitter/tests.rs"]
mod tests;
