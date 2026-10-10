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
//! Rewriting a reference by its text is sound only when no local binds the same
//! name, so, as for namespaces, a body in which something could is refused.

use std::collections::{BTreeMap, BTreeSet};

use super::namespaces::{class_member_names, scan, Replacement};
use super::{javascript_specifier, Module, TextEdit};
use crate::compiler::CompilerOptions;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::namespace_analysis::{has_runtime_values, refuse_shadowing_of};
use crate::parser::Declaration;
use crate::{Token, TokenKind};

const IMPORT_DEFAULT_HELPER: &str = "var __importDefault = (this && this.__importDefault) || function (mod) { return (mod && mod.__esModule) ? mod : { \"default\": mod }; };";
const IMPORT_STAR_HELPER: &str = "var __importStar = (this && this.__importStar) || function (mod) { if (mod && mod.__esModule) return mod; var result = {}; if (mod != null) for (var k in mod) if (k !== \"default\" && Object.prototype.hasOwnProperty.call(mod, k)) { Object.defineProperty(result, k, { enumerable: true, get: function () { return mod[k]; } }); } Object.defineProperty(result, \"default\", { enumerable: true, value: mod }); return result; };";

/// The identifier a required module is bound to: its file's name and a number,
/// `m_1`, `m_2`.
fn require_name(specifier: &str, used: &mut BTreeMap<String, usize>) -> String {
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
    format!("{stem}_{count}")
}

fn lines_in(module: &Module, start: usize, end: usize) -> usize {
    module.source[start..end].matches('\n').count()
}

fn unsupported(span: SourceSpan, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::UnsupportedSyntax, span, message)
}

pub(super) fn lower_commonjs(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
    bound_import_calls: bool,
) -> Result<BTreeMap<String, Replacement>, Diagnostic> {
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
    let mut hoisted: Vec<String> = Vec::new();
    let mut es_syntax = false;
    let mut needs_default = false;
    let mut needs_star = false;

    for declaration in &module.declarations {
        match declaration {
            Declaration::Import(import) if import.is_type_only() => {}
            Declaration::Import(import) if import.equals_require => {
                let spec = javascript_specifier(
                    &import.specifier,
                    options.jsx == Some(crate::compiler::JsxMode::Preserve),
                );
                let local = &import.bindings[0].local;
                push(
                    edits,
                    import.span.start,
                    import.span.end,
                    format!("const {local} = require(\"{spec}\");"),
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
                    let variable = require_name(&import.specifier, &mut used_names);
                    text.push_str(&format!("const {variable} = require(\"{spec}\"); "));
                    for binding in named {
                        references.insert(
                            binding.local.clone(),
                            Replacement {
                                text: format!("{variable}.{}", binding.imported),
                                wrap_calls: !bound_import_calls,
                            },
                        );
                    }
                }
                for binding in import.bindings.iter().filter(|binding| !binding.type_only) {
                    if binding.imported == "default" {
                        let variable = require_name(&import.specifier, &mut used_names);
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
                                wrap_calls: !bound_import_calls,
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
                push(
                    edits,
                    import.span.start,
                    import.span.end,
                    text.trim_end().to_string(),
                );
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
                if let Some(specifier) = &export.specifier {
                    let spec = javascript_specifier(
                        specifier,
                        options.jsx == Some(crate::compiler::JsxMode::Preserve),
                    );
                    let name = require_name(&spec, &mut used_names);
                    let text = super::reexports::commonjs(export, &spec, &name);
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
                    text.push_str(&format!(
                        "exports.{} = {}; ",
                        binding.exported, binding.local
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
                chain.push("default".to_string());
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
                if variable.kind == crate::parser::VariableKind::Const
                    && super::reexports::class_parameter_shadows(module, &variable.name)
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
                let name_end = tokens[from..]
                    .iter()
                    .take_while(|token| token.start < variable.span.end)
                    .find(|token| {
                        (token.kind == TokenKind::Identifier
                            || matches!(token.text.as_str(), "type" | "of"))
                            && token.text == variable.name
                    })
                    .map(|token| token.end);
                let Some(name_end) = name_end else {
                    return Err(unsupported(
                        variable.span.clone(),
                        "an exported variable's name could not be located",
                    ));
                };
                edits.push(TextEdit {
                    start: variable.span.start,
                    end: name_end,
                    replacement: format!("exports.{}", variable.name),
                });
            }
            Declaration::Function(function)
                if function.exported && !function.declared && !function.overload =>
            {
                es_syntax = true;
                strip_export(&tokens, token_at(function.span.start), edits);
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
                hoisted.push(if function.default_export {
                    format!("exports.default = {};", function.name)
                } else {
                    format!("exports.{0} = {0};", function.name)
                });
            }
            // A class whose decorators or auto-accessors are lowered carries its
            // own `export` and `exports.C = C`.
            Declaration::Class(class)
                if class.exported && super::decorators::lowers_class(class) =>
            {
                es_syntax = true;
                chain.push(class.name.clone());
            }
            Declaration::Class(class) if class.exported => {
                es_syntax = true;
                chain.push(class.export_name().to_string());
                strip_export(&tokens, token_at(class.span.start), edits);
                if class.anonymous {
                    edits.push(TextEdit {
                        start: class.name_span.start,
                        end: class.name_span.start,
                        replacement: format!("{} ", class.name),
                    });
                }
                edits.push(TextEdit {
                    start: class.span.end,
                    end: class.span.end,
                    replacement: format!(" exports.{} = {};", class.export_name(), class.name),
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
            },
        );
    }

    // The header: strictness, helpers, then the keys.
    let mut header = String::new();
    if es_syntax || has_assignment {
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
    if es_syntax && !has_assignment {
        header.push_str("Object.defineProperty(exports, \"__esModule\", { value: true }); ");
        if !chain.is_empty() {
            let keys: Vec<String> = chain
                .iter()
                .rev()
                .map(|name| format!("exports.{name}"))
                .collect();
            header.push_str(&format!("{} = void 0; ", keys.join(" = ")));
        }
        for assignment in &hoisted {
            header.push_str(assignment);
            header.push(' ');
        }
    }
    if !header.is_empty() {
        edits.push(TextEdit {
            start: 0,
            end: 0,
            replacement: header,
        });
    }

    if references.is_empty() {
        return Ok(references);
    }
    // References are rewritten by their text, so nothing may bind them locally.
    let names: BTreeSet<String> = references.keys().cloned().collect();
    let own: BTreeSet<&str> = exported_variables.iter().map(String::as_str).collect();
    let first_span = module
        .declarations
        .first()
        .map(|declaration| declaration.span().clone())
        .unwrap_or_else(|| SourceSpan::new(&module.id, 0, 0));
    refuse_shadowing_of(
        module,
        &tokens,
        &module.declarations,
        (0, module.source.len()),
        &[],
        &names,
        &own,
        first_span,
        &|name| {
            format!(
                "a local named `{name}` shadows an imported or exported binding of the module \
                 it is used in, which CommonJS output does not support yet"
            )
        },
    )?;
    let mut covered: Vec<(usize, usize)> = edits
        .iter()
        .filter(|edit| edit.end > edit.start)
        .map(|edit| (edit.start, edit.end))
        .collect();
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
    let Some(export) = tokens.get(from).filter(|token| token.is("export")) else {
        return;
    };
    let mut next = from + 1;
    if tokens.get(next).is_some_and(|token| token.is("default")) {
        next += 1;
    }
    if let Some(after) = tokens.get(next) {
        edits.push(TextEdit {
            start: export.start,
            end: after.start,
            replacement: String::new(),
        });
    }
}
