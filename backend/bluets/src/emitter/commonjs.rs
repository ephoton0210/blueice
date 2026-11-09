// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CommonJS emit (`module: commonjs`).
//!
//! A module's ES syntax becomes `require` and `exports`, the way TypeScript
//! writes it, by edits to the source text so no line moves:
//!
//! ```js
//! "use strict"; Object.defineProperty(exports, "__esModule", { value: true });
//! exports.b = exports.a = void 0; exports.f = f;
//! const m_1 = require("./m");
//! exports.a = 1;
//! function f() { return exports.a + (0, m_1.g)(); }
//! ```
//!
//! An import becomes a `const` of the required module and each use of an
//! imported name reads a member of it (a call as `(0, m_1.f)(..)`, so `this` is
//! not the module); an exported variable is a property of `exports` and each
//! read of it reads that property; an exported function is assigned to `exports`
//! at the top, where the function is already defined; an exported class, enum or
//! namespace is assigned when it is built; `export =` is `module.exports =`.
//! `exports.x = void 0` first creates the keys in declaration order.
//!
//! Reference edits use lexical module binding identities, so parameters and
//! local declarations with the same spelling keep their own values.

use std::collections::{BTreeMap, BTreeSet};

use super::namespaces::{class_member_names, scan, Replacement};
use super::{javascript_specifier, Module, TextEdit};
use crate::compiler::CompilerOptions;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::namespace_analysis::has_runtime_values;
use crate::parser::Declaration;
use crate::{Token, TokenKind};

pub(in crate::emitter) mod class_values;
mod export_properties;

pub(crate) const IMPORT_DEFAULT_HELPER: &str = "var __importDefault = (this && this.__importDefault) || function (mod) { return (mod && mod.__esModule) ? mod : { \"default\": mod }; };";
pub(crate) const IMPORT_STAR_HELPER: &str = include_str!("commonjs/import_star.v1.js");

/// The identifier a required module is bound to: its file's name and a number,
/// `m_1`, `m_2`.
fn require_name(
    specifier: &str,
    used: &mut BTreeMap<String, usize>,
    generated_names: &mut BTreeSet<String>,
) -> String {
    let stem = specifier
        .rsplit('/')
        .next()
        .unwrap_or(specifier)
        .split('.')
        .next()
        .unwrap_or("module");
    let mut stem: String = stem
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if stem.is_empty() || stem.starts_with(|character: char| character.is_ascii_digit()) {
        stem.insert(0, '_');
    }
    let count = used.entry(stem.clone()).or_default();
    *count += 1;
    let name = format!("{stem}_{count}");
    generated_names.insert(name.clone());
    name
}

fn lines_in(module: &Module, start: usize, end: usize) -> usize {
    module.source[start..end].matches('\n').count()
}

fn unsupported(span: SourceSpan, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::UnsupportedSyntax, span, message)
}

pub(super) fn lower_commonjs(
    module: &Module,
    project: &crate::Project,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
    wrapper: crate::ModuleKind,
    generated_names: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, Replacement>, Diagnostic> {
    let system_wrapper = wrapper == crate::ModuleKind::System;
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return Ok(BTreeMap::new());
    };
    let interop = options.es_module_interop;
    let token_at = |offset: usize| tokens.partition_point(|token| token.start < offset);
    let push = |edits: &mut Vec<TextEdit>, start: usize, end: usize, text: String| {
        let lines = lines_in(module, start, end);
        edits.push(TextEdit {
            start,
            end,
            replacement: format!("{text}{}", "\n".repeat(lines)),
        });
    };

    let has_assignment = module.declarations.iter().any(|declaration| {
        matches!(declaration, Declaration::ValueExport(export) if export.export_assignment)
    });
    let mut used_names: BTreeMap<String, usize> = BTreeMap::new();
    let mut references: BTreeMap<String, Replacement> = BTreeMap::new();
    let mut exported_variables: BTreeSet<String> = BTreeSet::new();
    let mut chain: Vec<String> = Vec::new();
    let mut export_members = BTreeMap::new();
    let mut hoisted: Vec<(String, SourceSpan)> = Vec::new();
    let mut es_syntax = false;
    let mut needs_default = false;
    let mut needs_star = false;

    if matches!(wrapper, crate::ModuleKind::Amd | crate::ModuleKind::System) {
        super::output_options::erase_elided_import_comments(module, edits)?;
    }

    for declaration in &module.declarations {
        match declaration {
            Declaration::Import(import) if import.is_type_only() => {}
            Declaration::Import(import) if import.equals_require => {
                let spec = javascript_specifier(
                    &import.specifier,
                    options.jsx == Some(crate::compiler::JsxMode::Preserve),
                );
                let local = &import.bindings[0].local;
                if import.exported {
                    es_syntax = true;
                    references.insert(
                        local.clone(),
                        Replacement {
                            text: format!("exports.{local}"),
                            wrap_calls: !system_wrapper,
                            anchor_each_token: false,
                        },
                    );
                }
                push(
                    edits,
                    import.span.start,
                    import.span.end,
                    if import.exported {
                        format!("exports.{local} = require(\"{spec}\");")
                    } else {
                        format!("const {local} = require(\"{spec}\");")
                    },
                );
            }
            Declaration::Import(import) => {
                es_syntax = true;
                let spec = javascript_specifier(
                    &import.specifier,
                    options.jsx == Some(crate::compiler::JsxMode::Preserve),
                );
                let mut text = String::new();
                if import.bindings.is_empty() {
                    text.push_str(&format!("require(\"{spec}\");"));
                }
                let named: Vec<_> = import
                    .bindings
                    .iter()
                    .filter(|binding| {
                        !binding.type_only
                            && binding.imported != "default"
                            && binding.imported != "*"
                    })
                    .collect();
                if !named.is_empty() {
                    let variable =
                        require_name(&import.specifier, &mut used_names, generated_names);
                    text.push_str(&format!("const {variable} = require(\"{spec}\"); "));
                    for binding in named {
                        references.insert(
                            binding.local.clone(),
                            Replacement {
                                text: format!("{variable}.{}", binding.imported),
                                wrap_calls: !system_wrapper,
                                anchor_each_token: false,
                            },
                        );
                    }
                }
                for binding in import.bindings.iter().filter(|binding| !binding.type_only) {
                    if binding.imported == "default" {
                        let variable =
                            require_name(&import.specifier, &mut used_names, generated_names);
                        let required = if interop {
                            needs_default = true;
                            format!("__importDefault(require(\"{spec}\"))")
                        } else {
                            format!("require(\"{spec}\")")
                        };
                        text.push_str(&format!("const {variable} = {required}; "));
                        references.insert(
                            binding.local.clone(),
                            Replacement {
                                text: format!("{variable}.default"),
                                wrap_calls: !system_wrapper,
                                anchor_each_token: false,
                            },
                        );
                    } else if binding.imported == "*" {
                        let required = if interop {
                            needs_star = true;
                            format!("__importStar(require(\"{spec}\"))")
                        } else {
                            format!("require(\"{spec}\")")
                        };
                        text.push_str(&format!("const {} = {required}; ", binding.local));
                    }
                }
                let text = if matches!(wrapper, crate::ModuleKind::Amd | crate::ModuleKind::System)
                {
                    format!(
                        "{}{}{}",
                        super::relocations::unmapped(),
                        text.trim_end(),
                        super::relocations::unmapped()
                    )
                } else {
                    text.trim_end().to_string()
                };
                push(edits, import.span.start, import.span.end, text);
            }
            Declaration::ValueExport(export) if export.export_assignment => {
                let local = &export.bindings[0].local;
                push(
                    edits,
                    export.span.start,
                    export.span.end,
                    format!("module.exports = {local};"),
                );
            }
            Declaration::ValueExport(export) if export.is_type_only() => {}
            Declaration::ValueExport(export) => {
                es_syntax = true;
                for binding in export.bindings.iter().filter(|binding| !binding.type_only) {
                    export_members.insert(
                        binding.exported.clone(),
                        export_properties::key(module, binding).member,
                    );
                }
                if let Some(specifier) = &export.specifier {
                    let spec = javascript_specifier(
                        specifier,
                        options.jsx == Some(crate::compiler::JsxMode::Preserve),
                    );
                    let name = require_name(&spec, &mut used_names, generated_names);
                    let text = if export.star
                        && export.namespace.is_none()
                        && !system_wrapper
                        && super::helper_selection::external(options)
                    {
                        let helper =
                            super::helper_selection::reference(&module.source, "__exportStar");
                        format!("{helper}(require({spec:?}),exports);")
                    } else {
                        super::reexports::commonjs(module, export, &spec, &name, system_wrapper)
                    };
                    let text = if system_wrapper && export.star && export.namespace.is_none() {
                        format!(
                            "{}{text}{}",
                            super::relocations::unmapped(),
                            super::relocations::unmapped()
                        )
                    } else {
                        text
                    };
                    if let Some(namespace) = &export.namespace {
                        chain.push(namespace.clone());
                    }
                    chain.extend(
                        export
                            .bindings
                            .iter()
                            .filter(|binding| !binding.type_only)
                            .map(|binding| binding.exported.clone()),
                    );
                    push(edits, export.span.start, export.span.end, text);
                    continue;
                }
                let mut text = String::new();
                for binding in export.bindings.iter().filter(|binding| !binding.type_only) {
                    chain.push(binding.exported.clone());
                    let live = module.declarations.iter().any(|declaration| matches!(declaration,
                        Declaration::Variable(variable) if variable.name == binding.local && variable.kind != crate::parser::VariableKind::Const));
                    if live {
                        let local = if module.declarations.iter().any(|declaration| matches!(declaration,
                            Declaration::Variable(variable) if variable.name == binding.local && variable.exported)) {
                            format!("exports.{}", binding.local)
                        } else { binding.local.clone() };
                        text.push_str(&format!("Object.defineProperty(exports, {:?}, {{ enumerable: true, get: function() {{ return {local}; }} }}); ", binding.exported));
                        continue;
                    }
                    let key = export_properties::key(module, binding);
                    text.push_str(&format!(
                        "{}{} = {};{} {}",
                        super::relocations::source_anchor(key.start),
                        key.member,
                        binding.local,
                        super::relocations::source_anchor(key.end),
                        super::relocations::unmapped(),
                    ));
                }
                push(
                    edits,
                    export.span.start,
                    export.span.end,
                    text.trim_end().to_string(),
                );
            }
            Declaration::DefaultExport(export) => {
                es_syntax = true;
                if export.expression {
                    let initializer = module
                        .declarations
                        .iter()
                        .find_map(|declaration| match declaration {
                            Declaration::Variable(variable) if variable.name == export.name => {
                                variable.initializer.first()
                            }
                            _ => None,
                        })
                        .ok_or_else(|| {
                            unsupported(
                                export.span.clone(),
                                "default export expression has no initializer",
                            )
                        })?;
                    push(
                        edits,
                        export.span.start,
                        initializer.start,
                        "exports.default = ".to_string(),
                    );
                    continue;
                }
                chain.push("default".to_string());
                push(
                    edits,
                    export.span.start,
                    export.span.end,
                    format!("exports.default = {};", export.name),
                );
            }
            Declaration::TypeExport(_) => es_syntax = true,
            Declaration::Variable(variable) if variable.exported && !variable.declared => {
                es_syntax = true;
                chain.push(variable.name.clone());
                let class_storage = if system_wrapper {
                    class_values::Storage::Original
                } else {
                    class_values::storage(module, variable, options, &tokens)
                };
                if variable.kind == crate::parser::VariableKind::Const
                    && (super::reexports::class_parameter_shadows(module, &variable.name)
                        || matches!(class_storage, class_values::Storage::Original)
                            && variable.initializer.iter().any(|token| {
                                module
                                    .class_expression(token.start)
                                    .is_some_and(|expression| expression.name.is_none())
                            }))
                {
                    strip_export(&tokens, token_at(variable.span.start), edits);
                    edits.push(TextEdit {
                        start: variable.span.end,
                        end: variable.span.end,
                        replacement: format!(" exports.{0} = {0};", variable.name),
                    });
                    continue;
                }
                exported_variables.insert(variable.name.clone());
                if variable.initializer.is_empty() {
                    edits.push(TextEdit {
                        start: variable.span.start,
                        end: variable.span.end,
                        replacement: String::new(),
                    });
                    continue;
                }
                let mut depth = 0usize;
                for token in &variable.initializer {
                    if module.edits.iter().any(|edit| {
                        edit.replacement.is_empty()
                            && edit.start <= token.start
                            && token.end <= edit.end
                    }) {
                        continue;
                    }
                    match token.text.as_str() {
                        "(" | "[" | "{" => depth += 1,
                        ")" | "]" | "}" => depth = depth.saturating_sub(1),
                        "," if depth == 0 => {
                            return Err(unsupported(
                                variable.span.clone(),
                                "an exported variable with several declarators is not supported in CommonJS output yet",
                            ));
                        }
                        _ => {}
                    }
                }
                let from = token_at(variable.span.start);
                let name_token = tokens[from..]
                    .iter()
                    .take_while(|token| token.start < variable.span.end)
                    .find(|token| {
                        (token.kind == TokenKind::Identifier
                            || matches!(token.text.as_str(), "type" | "of"))
                            && token.text == variable.name
                    });
                let Some(name_token) = name_token else {
                    return Err(unsupported(
                        variable.span.clone(),
                        "an exported variable's name could not be located",
                    ));
                };
                if let class_values::Storage::Retained(keyword) = class_storage {
                    class_values::retain(variable, keyword, name_token, edits);
                    continue;
                }
                let replacement = if system_wrapper {
                    let export_anchor = super::relocations::source_anchor(variable.span.start);
                    let name_anchor = super::relocations::source_anchor(name_token.start);
                    format!(
                        "{export_anchor}exports{export_anchor}.{name_anchor}{}",
                        variable.name
                    )
                } else {
                    format!(
                        "{}exports.{}",
                        super::relocations::source_anchor(name_token.start),
                        variable.name
                    )
                };
                edits.push(TextEdit {
                    start: variable.span.start,
                    end: name_token.end,
                    replacement,
                });
            }
            Declaration::Function(function)
                if function.exported && !function.declared && !function.overload =>
            {
                es_syntax = true;
                strip_export(&tokens, token_at(function.span.start), edits);
                if function.async_function
                    && options.target
                        >= if function.generator {
                            crate::EcmaTarget::Es2018
                        } else {
                            crate::EcmaTarget::Es2017
                        }
                {
                    if let Some(asynchronous) = tokens[token_at(function.span.start)..]
                        .iter()
                        .take_while(|token| {
                            token.start < function.body_open.unwrap_or(function.span.end)
                        })
                        .find(|token| token.is("async"))
                    {
                        if let Some(edit) = edits.last_mut().filter(|edit| {
                            edit.start == function.span.start && edit.end == asynchronous.end
                        }) {
                            edit.replacement = format!(
                                "{}async{}",
                                super::relocations::source_anchor(asynchronous.start),
                                super::relocations::source_anchor(asynchronous.end)
                            );
                        }
                    }
                }
                if function.anonymous {
                    let position = tokens[token_at(function.span.start)..]
                        .iter()
                        .find(|token| token.is(if function.generator { "*" } else { "function" }))
                        .unwrap()
                        .end;
                    edits.push(TextEdit {
                        start: position,
                        end: position,
                        replacement: format!(" {}", function.name),
                    });
                }
                let assignment = if function.default_export {
                    format!("exports.default = {};", function.name)
                } else {
                    format!("exports.{0} = {0};", function.name)
                };
                hoisted.push((assignment, function.span.clone()));
            }
            // A class whose decorators or auto-accessors are lowered carries its
            // own `export` and `exports.C = C`.
            Declaration::Class(class)
                if class.exported && super::decorators::lowers_class(class, options) =>
            {
                es_syntax = true;
                chain.push(class.export_name().to_string());
            }
            Declaration::Class(class) if class.exported => {
                es_syntax = true;
                chain.push(class.export_name().to_string());
                let header = token_at(class.span.start)..token_at(class.name_span.start);
                if let Some(export) = header.into_iter().find(|index| tokens[*index].is("export")) {
                    if let Some(modifier) = &class.abstract_modifier {
                        // The parsed abstract modifier is erased with this
                        // export prefix; only the following `class` is runtime.
                        let keyword = &tokens[token_at(modifier.end)];
                        debug_assert!(keyword.is("class"));
                        let anchor = super::relocations::source_anchor(tokens[export].start);
                        edits.push(TextEdit {
                            start: tokens[export].start,
                            end: keyword.end,
                            replacement: format!("{anchor}class{anchor}"),
                        });
                    } else if !options.experimental_decorators
                        && options.target == crate::EcmaTarget::EsNext
                        && options.defines_class_fields()
                        && !class.decorators.is_empty()
                    {
                        strip_export_with_anchor(&tokens, export, edits, None);
                    } else {
                        strip_export(&tokens, export, edits);
                    }
                }
                if class.anonymous {
                    edits.push(TextEdit {
                        start: class.name_span.start,
                        end: class.name_span.start,
                        replacement: format!("{} ", class.name),
                    });
                }
                let publication = if options.target == crate::EcmaTarget::Es5 && !class.anonymous {
                    &class.name_span
                } else {
                    &class.span
                };
                let publication_anchor = |position| {
                    if system_wrapper
                        || options.target == crate::EcmaTarget::Es5 && class.default_export
                    {
                        super::relocations::unmapped().to_string()
                    } else {
                        super::relocations::source_anchor(position)
                    }
                };
                let static_initializer = options.target >= crate::EcmaTarget::Es2015
                    && class.members.iter().any(|member| {
                        member.field.as_ref().is_some_and(|field| {
                            field.is_static && !field.declared && field.initializer.is_some()
                        })
                    });
                let completion = if static_initializer {
                    super::relocations::unmapped().to_string()
                } else {
                    super::relocations::source_anchor(class.span.end)
                };
                edits.push(TextEdit {
                    start: class.span.end,
                    end: class.span.end,
                    replacement: format!(
                        "{completion} {}exports.{} = {};{}",
                        publication_anchor(publication.start),
                        class.export_name(),
                        class.name,
                        publication_anchor(publication.end)
                    ),
                });
            }
            Declaration::Enum(item) if item.exported && !item.declared => {
                es_syntax = true;
                chain.push(item.name.clone());
            }
            Declaration::Namespace(item)
                if item.exported && !item.declared && has_runtime_values(&item.body) =>
            {
                es_syntax = true;
                chain.push(item.name.clone());
            }
            Declaration::Interface(item) if item.exported => es_syntax = true,
            Declaration::TypeAlias(item) if item.exported => es_syntax = true,
            _ => {}
        }
    }

    // A reference to an exported variable reads `exports`.
    for name in &exported_variables {
        references.insert(
            name.clone(),
            Replacement {
                text: format!("exports.{name}"),
                wrap_calls: false,
                anchor_each_token: system_wrapper,
            },
        );
    }

    // The header: strictness, helpers, then the keys.
    let mut header = String::new();
    if (es_syntax || has_assignment) && !system_wrapper {
        header.push_str("\"use strict\"; ");
    }
    if needs_default {
        header.push_str(IMPORT_DEFAULT_HELPER);
        header.push(' ');
    }
    if needs_star {
        header.push_str(IMPORT_STAR_HELPER);
        header.push(' ');
    }
    if es_syntax && !has_assignment && !system_wrapper {
        header.push_str("Object.defineProperty(exports, \"__esModule\", { value: true }); ");
        if !chain.is_empty() {
            let hoisted_names = module
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    Declaration::Function(function)
                        if function.exported && !function.declared && !function.overload =>
                    {
                        Some(if function.default_export {
                            "default"
                        } else {
                            function.name.as_str()
                        })
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            let mut seen = BTreeSet::new();
            let declarations = chain
                .iter()
                .filter(|name| seen.insert(name.as_str()) && !hoisted_names.contains(name.as_str()))
                .collect::<Vec<_>>();
            let keys = declarations
                .into_iter()
                .rev()
                .map(|name| {
                    export_members
                        .get(name.as_str())
                        .cloned()
                        .unwrap_or_else(|| format!("exports.{name}"))
                })
                .collect::<Vec<_>>();
            if !keys.is_empty() {
                header.push_str(&format!("{} = void 0; ", keys.join(" = ")));
            }
        }
        for (assignment, span) in &hoisted {
            if options.source_map {
                header.push_str(&super::relocations::source_anchor(span.start));
            }
            header.push_str(assignment);
            if options.source_map {
                header.push_str(&super::relocations::source_anchor(span.end));
                header.push(' ');
                header.push_str(super::relocations::unmapped());
            } else {
                header.push(' ');
            }
        }
    }
    if !header.is_empty() {
        edits.push(TextEdit {
            start: 0,
            end: 0,
            replacement: if options.source_map {
                format!("{}{header}", super::relocations::unmapped())
            } else {
                header
            },
        });
    }

    if references.is_empty() {
        return Ok(references);
    }
    // Only references whose lexical owner is the module may be rewritten.
    let names: BTreeSet<String> = references.keys().cloned().collect();
    let positions = crate::checker::module_value_reference_positions(
        project,
        module,
        &names,
        options.limits.max_type_expansions,
    );
    let mut covered: Vec<(usize, usize)> = edits
        .iter()
        .filter(|edit| edit.end > edit.start)
        .map(|edit| (edit.start, edit.end))
        .collect();
    covered.extend(
        tokens
            .iter()
            .filter(|token| names.contains(&token.text) && !positions.contains(&token.start))
            .map(|token| (token.start, token.end)),
    );
    // Enums and namespaces are rebuilt whole by their own lowering.
    for declaration in &module.declarations {
        match declaration {
            Declaration::Enum(item) => covered.push((item.span.start, item.span.end)),
            Declaration::Namespace(item) => covered.push((item.span.start, item.span.end)),
            _ => {}
        }
    }
    let members = class_member_names(module, &module.declarations);
    let mut pending: Vec<TextEdit> = Vec::new();
    scan(
        module,
        &tokens,
        0,
        &references,
        &covered,
        &members,
        &mut pending,
    );
    edits.extend(pending);
    Ok(references)
}

/// Removes `export` (and `default`) from the start of a declaration.
fn strip_export(tokens: &[Token], from: usize, edits: &mut Vec<TextEdit>) {
    let source = tokens.get(from).map(|token| token.start);
    strip_export_with_anchor(tokens, from, edits, source);
}

fn strip_export_with_anchor(
    tokens: &[Token],
    from: usize,
    edits: &mut Vec<TextEdit>,
    source: Option<usize>,
) {
    let Some(export) = tokens.get(from).filter(|token| token.is("export")) else {
        return;
    };
    let mut next = from + 1;
    if tokens.get(next).is_some_and(|token| token.is("default")) {
        next += 1;
    }
    if let Some(after) = tokens.get(next) {
        let anchor = source.map_or_else(
            || super::relocations::unmapped().to_string(),
            super::relocations::source_anchor,
        );
        edits.push(TextEdit {
            start: export.start,
            end: after.end,
            replacement: format!(
                "{}{}{}",
                anchor,
                after.text,
                if after.is("function") || after.is("class") {
                    anchor.clone()
                } else {
                    String::new()
                }
            ),
        });
    }
}
