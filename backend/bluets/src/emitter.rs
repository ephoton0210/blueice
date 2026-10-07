// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ESM, declaration, and source-map emission from the shared checked graph.

use crate::checker::CheckedProject;
use crate::compiler::{
    fingerprint, is_declaration_module, is_external_library_module, CompilerOptions, Project,
};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{Declaration, Module, TextEdit, Type, TypeParameter};
use crate::strict_boundaries;
use std::collections::BTreeMap;

pub use decorators::DECORATOR_HELPER_V1_VERSION;
pub use legacy_decorators::LEGACY_DECORATOR_HELPER_V1_VERSION;
pub use private_lowering::CLASS_HELPER_V1_VERSION;

mod class_lowering;
mod classes;
mod commonjs;
mod decorators;
mod enums;
mod inferred_declarations;
mod inferred_returns;
mod jsx;
mod legacy_decorators;
mod namespaces;
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
        .filter(|(id, _)| !is_declaration_module(id) && !is_external_library_module(id))
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
                .then(|| {
                    let context = inferred_declarations::Context::new(checked_module, project)?;
                    emit_declaration(
                        &inferred_returns::declaration_module(checked_module),
                        &checked_module.symbols,
                        Some(&context),
                    )
                })
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
                    is_declaration_module(id)
                        && !is_external_library_module(id)
                        && !project.is_ambient_declaration_module(id)
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
    let mut edits = module.edits.clone();
    for declaration in &module.declarations {
        let Declaration::Import(import) = declaration else {
            continue;
        };
        // A CommonJS import is rewritten whole, with its specifier.
        if import.type_only || options.module_kind == crate::compiler::ModuleKind::CommonJs {
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
        let emitted_specifier = javascript_specifier(
            &import.specifier,
            options.jsx == Some(crate::compiler::JsxMode::Preserve),
        );
        edits.push(TextEdit {
            start: import.specifier_span.start,
            end: import.specifier_span.end,
            replacement: format!("{quote}{emitted_specifier}{quote}"),
        });
    }
    edits.extend(classes::overload_signature_erasures(module));
    // CommonJS name rewriting goes first so that the lowerings below, which copy
    // or move source text (a static initializer, a decorator expression), take
    // the rewritten text with them.
    let references = if options.module_kind == crate::compiler::ModuleKind::CommonJs {
        commonjs::lower_commonjs(module, options, &mut edits)?
    } else {
        BTreeMap::new()
    };
    class_lowering::lower_class_members(module, options, &mut edits)?;
    enums::lower_enums(module, project, exported_enums, options, &mut edits)?;
    namespaces::lower_namespaces(module, options, &mut edits)?;
    if options.experimental_decorators {
        legacy_decorators::lower_legacy_decorators(module, options, &mut edits)?;
    } else {
        decorators::lower_decorators(module, options, &mut edits)?;
    }
    jsx::lower_jsx(module, options, &references, &mut edits)?;
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

/// Every declaration of a module in source order, descending into namespace
/// bodies, each with whether it is inside one.
fn runtime_declarations(declarations: &[Declaration]) -> Vec<(&Declaration, bool)> {
    fn visit<'a>(
        declarations: &'a [Declaration],
        nested: bool,
        into: &mut Vec<(&'a Declaration, bool)>,
    ) {
        for declaration in declarations {
            into.push((declaration, nested));
            if let Declaration::Namespace(namespace) = declaration {
                visit(&namespace.body, true, into);
            }
        }
    }
    let mut all = Vec::new();
    visit(declarations, false, &mut all);
    all
}

/// The specifier an emitted module is imported by: a TypeScript extension becomes
/// the JavaScript one (`.tsx` stays `.jsx` when JSX is preserved).
fn javascript_specifier(specifier: &str, preserve_jsx: bool) -> String {
    if let Some(stem) = specifier.strip_suffix(".tsx") {
        return format!("{stem}{}", if preserve_jsx { ".jsx" } else { ".js" });
    }
    specifier
        .strip_suffix(".ts")
        .map(|stem| format!("{stem}.js"))
        .unwrap_or_else(|| specifier.to_string())
}

/// One-line text of an expression: runs of whitespace become one space, and a
/// space an erased annotation left before a closing bracket or comma goes
/// (outside string and template literals).
fn compact_expression(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut output = String::with_capacity(collapsed.len());
    let mut quote: Option<char> = None;
    let mut chars = collapsed.chars().peekable();
    let mut escaped = false;
    while let Some(character) = chars.next() {
        match quote {
            Some(open) => {
                output.push(character);
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == open {
                    quote = None;
                }
            }
            None => {
                if matches!(character, '"' | '\'' | '`') {
                    quote = Some(character);
                    output.push(character);
                } else if character == ' '
                    && chars
                        .peek()
                        .is_some_and(|next| matches!(next, ')' | ']' | ','))
                {
                    // dropped
                } else {
                    output.push(character);
                }
            }
        }
    }
    output
}

/// Replaces each `\0M<start>,<end>\0` marker with the text of that source
/// range after the edits that lie inside it, on one line.
fn expand_relocations(replacement: &str, source: &str, edits: &[TextEdit]) -> String {
    let mut output = String::new();
    let mut rest = replacement;
    while let Some(open) = rest.find("\u{0}M") {
        output.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find('\u{0}') else {
            output.push_str(&rest[open..]);
            return output;
        };
        let range = &after[..close];
        rest = &after[close + 1..];
        let Some((start, end)) = range.split_once(',').and_then(|(start, end)| {
            Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
        }) else {
            continue;
        };
        let mut text = String::new();
        let mut cursor = start;
        for edit in edits.iter().filter(|edit| {
            edit.start >= start && edit.end <= end && !edit.replacement.contains('\u{0}')
        }) {
            if edit.start < cursor {
                continue;
            }
            text.push_str(&source[cursor..edit.start]);
            text.push_str(&expand_relocations(&edit.replacement, source, edits));
            cursor = edit.end;
        }
        text.push_str(&source[cursor..end]);
        output.push_str(&compact_expression(&text));
    }
    output.push_str(rest);
    output
}

fn apply_edits(source: &str, mut edits: Vec<TextEdit>) -> EmittedJavaScript {
    edits.sort_by_key(|edit| (edit.start, edit.end));
    // A replacement may carry relocation markers (see `decorators::relocated`):
    // the text of another source range, with the edits inside it applied.
    let relocating = edits.iter().any(|edit| edit.replacement.contains('\u{0}'));
    let snapshot = relocating.then(|| edits.clone());
    let mut emitted = ProvenanceEmitter::new(source, source.len());
    let mut cursor = 0usize;
    for edit in edits {
        if edit.start < cursor || edit.end < edit.start || edit.end > source.len() {
            // A nested erase is fully covered by its outer erase.  Parser
            // ownership produces these intentionally for `declare function`.
            continue;
        }
        emitted.copy(&source[cursor..edit.start], cursor);
        let replacement = match &snapshot {
            Some(all) if edit.replacement.contains('\u{0}') => {
                expand_relocations(&edit.replacement, source, all)
            }
            _ => edit.replacement.clone(),
        };
        if replacement.is_empty() {
            // Preserve physical lines so line-level source maps, diagnostics,
            // and ordinary text diffs remain stable after type erasure.
            emitted.preserve_line_breaks(&source[edit.start..edit.end], edit.start);
        } else {
            emitted.replace(&replacement, edit.start);
        }
        emitted.mark(edit.end);
        cursor = edit.end;
    }
    emitted.copy(&source[cursor..], cursor);
    emitted.finish()
}

fn emit_declaration(
    module: &Module,
    symbols: &[crate::checker::Symbol],
    inferred: Option<&inferred_declarations::Context<'_>>,
) -> Result<String, Diagnostic> {
    let mut output = String::new();
    let enum_evaluations = crate::enum_eval::evaluate_enums(module);
    let mut enum_position = 0usize;
    let mut private_alias = false;
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
            Declaration::Import(import) => {
                if let Some(text) = inferred.and_then(|context| context.import(import)) {
                    output.push_str(&text);
                }
            }
            Declaration::TypeAlias(alias)
                if alias.exported
                    || inferred
                        .and_then(|context| context.private_alias(&alias.name))
                        .is_some() =>
            {
                private_alias |= !alias.exported;
                output.push_str(if alias.exported {
                    "export type "
                } else {
                    "type "
                });
                output.push_str(&alias.name);
                emit_type_parameters(&mut output, &alias.type_parameters);
                output.push_str(" = ");
                output.push_str(
                    &inferred
                        .and_then(|context| context.private_alias(&alias.name))
                        .map(str::to_string)
                        .unwrap_or_else(|| type_to_ts(&alias.value)),
                );
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
                    output.push_str("    ");
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
                if variable.annotation.is_none() {
                    if let Some((text, initializer)) =
                        inferred.and_then(|context| context.variable(&variable.name))
                    {
                        output.push_str(if *initializer { " = " } else { ": " });
                        output.push_str(text);
                        output.push_str(";\n");
                        continue;
                    }
                }
                let inferred = symbols
                    .iter()
                    .find(|symbol| {
                        symbol.kind == crate::checker::SymbolKind::Variable
                            && symbol.name == variable.name
                            && symbol.span == variable.span
                    })
                    .and_then(|symbol| symbol.value_type.as_ref());
                let value_type = variable.annotation.as_ref().or(inferred);
                if variable.annotation.is_none()
                    && variable.kind == crate::parser::VariableKind::Const
                    && matches!(value_type, Some(Type::Literal(_)))
                {
                    output.push_str(" = ");
                } else {
                    output.push_str(": ");
                }
                output.push_str(
                    &value_type
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
                        &inferred
                            .and_then(|context| context.parameter_type(parameter.span.start))
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                parameter
                                    .annotation
                                    .as_ref()
                                    .map(type_to_ts)
                                    .unwrap_or_else(|| "unknown".to_string())
                            }),
                    );
                }
                output.push_str("): ");
                output.push_str(
                    &inferred
                        .and_then(|context| context.return_type(function.span.start))
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            function
                                .return_type
                                .as_ref()
                                .map(type_to_ts)
                                .unwrap_or_else(|| "unknown".to_string())
                        }),
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
            Declaration::Namespace(namespace)
                if namespace.exported || is_value_export_name(module, &namespace.name) =>
            {
                let prefix = if namespace.exported {
                    "export declare "
                } else {
                    "declare "
                };
                output.push_str(&namespaces::emit_namespace_declaration(
                    module, namespace, prefix,
                )?);
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
                classes::emit_class_declaration(class, prefix, &mut output, inferred)?;
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
            Declaration::ValueExport(export) if export.export_assignment => {
                output.push_str(&format!("export = {};\n", export.bindings[0].local));
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
    if private_alias
        || (output.is_empty()
            && module.declarations.iter().any(|declaration| {
                matches!(
                    declaration,
                    Declaration::Import(_)
                        | Declaration::TypeExport(_)
                        | Declaration::ValueExport(_)
                        | Declaration::DefaultExport(_)
                )
            }))
    {
        output.push_str("export {};\n");
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
        Type::KeyOf(value) => format!("keyof {}", type_to_ts(value)),
        Type::IndexedAccess { object, index } => {
            format!("{}[{}]", type_to_ts(object), type_to_ts(index))
        }
        Type::Predicate(predicate) => predicate.text(type_to_ts),
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
        Type::Named { name, arguments } if arguments.is_empty() => {
            crate::parser::source_type_name(name).to_string()
        }
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
        Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            ..
        } => {
            let mut generic = String::new();
            emit_type_parameters(&mut generic, type_parameters);
            format!(
                "{generic}{} => {}",
                method_parameters_to_ts(parameters),
                type_to_ts(result)
            )
        }
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
