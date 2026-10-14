// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Parses one ECMAScript module and retains its declarative import/export
/// entries separately from executable statements. Linking remains host-driven
/// through [`crate::Vm::execute_module_graph`], but this distinct goal keeps
/// module strictness and top-level syntax separate from classic scripts.
pub fn parse_module(source: &str) -> Result<Module, ParseError> {
    parse_module_with_edition(source, SyntaxEdition::EsNext)
}

/// Parses a Module with exact byte spans for its executable body items and
/// identifier expressions. Static-only import/export headers have no body
/// safe point. The spans are parser provenance, not debugger authorization.
pub fn parse_module_with_debug_spans(
    source: &str,
) -> Result<(Module, Vec<ParsedStatementSpan>, Vec<ParsedIdentifierSpan>), ParseError> {
    let escaped = crate::source_encoding::escape(source);
    let (module, spans, identifiers) = parse_encoded_module(&escaped, SyntaxEdition::EsNext, true)?;
    let source_bytes = crate::source_encoding::escaped_character_source_bytes(source);
    let spans = spans
        .into_iter()
        .map(|(start, end)| ParsedStatementSpan {
            start_byte: source_bytes[start],
            end_byte: source_bytes[end],
        })
        .collect();
    let identifiers = identifiers
        .into_iter()
        .map(|(start, end, name)| ParsedIdentifierSpan {
            start_byte: source_bytes[start],
            end_byte: source_bytes[end],
            name,
        })
        .collect();
    Ok((module, spans, identifiers))
}

/// Parses a Module with the selected body syntax edition. The Module goal
/// admits static import/export headers even for an ES5 payload; this is not
/// a claim that an ES5 Script admits module declarations.
pub fn parse_module_with_edition(
    source: &str,
    edition: SyntaxEdition,
) -> Result<Module, ParseError> {
    parse_encoded_module(&crate::source_encoding::escape(source), edition, false)
        .map(|(module, _, _)| module)
}

type EncodedModuleWithDebugSpans = (Module, Vec<(usize, usize)>, Vec<(usize, usize, String)>);

fn parse_encoded_module(
    source: &str,
    edition: SyntaxEdition,
    capture_spans: bool,
) -> Result<EncodedModuleWithDebugSpans, ParseError> {
    let mut parser = Parser::new_module(source);
    parser.capture_debug_identifiers = capture_spans;
    parser.select_edition(edition)?;
    parser.module_await = true;
    parser.module = true;
    parser.strict = true;
    let mut body = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut requests = Vec::new();
    let mut spans = Vec::new();
    while !parser.at_eof() {
        let start = parser.token_start();
        let body_len = body.len();
        if parser.check_identifier("import")
            && !parser.check_punct_at(1, Punct::LParen)
            && !parser.check_punct_at(1, Punct::Dot)
        {
            let declaration = parser.parse_import_declaration()?;
            if let Some(request) = declaration
                .first()
                .filter(|request| !matches!(request.import_name, ImportName::Source))
            {
                requests.push(RequestedModule {
                    specifier: request.module_request.clone(),
                    module_type: request.module_type,
                    phase: if request.import_name == ImportName::DeferredNamespace {
                        ImportPhase::Defer
                    } else {
                        ImportPhase::Evaluation
                    },
                });
            }
            imports.extend(declaration);
        } else if parser.check_identifier("export") || parser.check_punct(Punct::At) {
            // `@dec export class C {}`: decorators may come before `export`.
            let start = parser.token_start();
            let decorators = parser.parse_decorators()?;
            if decorators.is_empty() {
                // Nothing to add: `export` follows directly.
            } else if !parser.check_identifier("export") {
                let class = parser.parse_decorated_class(decorators, start)?;
                if class.name.is_none() {
                    return Err(parser.syntax_error("class declarations require a name"));
                }
                body.push(Stmt::ClassDecl(class));
                if capture_spans {
                    spans.push((start, parser.positions[parser.pos]));
                }
                continue;
            }
            if let Some((request, module_type)) =
                parser.parse_export_declaration(decorators, &mut body, &mut exports)?
            {
                requests.push(RequestedModule {
                    specifier: request,
                    module_type,
                    phase: ImportPhase::Evaluation,
                });
            }
        } else {
            body.push(parser.parse_statement()?);
        }
        if capture_spans && body.len() > body_len {
            debug_assert_eq!(body.len(), body_len + 1);
            spans.push((start, parser.positions[parser.pos]));
        }
    }
    let program = Program { body: body.clone() };
    if contains_super_call_outside_class(&program)
        || contains_super_property_outside_class(&program)
    {
        return Err(parser.syntax_error("super is not valid in module code"));
    }
    crate::compiler::validate_private_early_errors(&program)
        .map_err(|error| parser.syntax_error(error.to_string()))?;
    let mut exported_names = std::collections::HashSet::new();
    for export in &exports {
        let name = match export {
            ExportEntry::Local { export_name, .. }
            | ExportEntry::Indirect { export_name, .. }
            | ExportEntry::Namespace { export_name, .. } => Some(export_name),
            ExportEntry::Star { .. } => None,
        };
        if let Some(name) = name {
            if !exported_names.insert(name.clone()) {
                return Err(parser.syntax_error("duplicate exported name"));
            }
        }
    }
    validate_module_declarations(&body, &imports, &parser)?;
    parser.validate_edition()?;
    editions::validate_body(&body, edition)?;
    if exports
        .iter()
        .any(|export| matches!(export, ExportEntry::Namespace { .. }))
    {
        edition.require(SyntaxEdition::Es2020, "namespace exports")?;
    }
    if imports.iter().any(|import| {
        matches!(
            import.import_name,
            ImportName::DeferredNamespace | ImportName::Source
        )
    }) {
        edition.require(SyntaxEdition::EsNext, "import phases")?;
    }
    Ok((
        Module {
            body,
            imports,
            exports,
            requests,
        },
        spans,
        parser.debug_identifier_spans,
    ))
}

/// Module items use one lexical environment: imported bindings, top-level
/// functions, classes, and lexical declarations must be unique, and none may
/// collide with a top-level `var`. Modules are always strict, so `eval` and
/// `arguments` cannot be imported bindings either.
fn validate_module_declarations(
    body: &[Stmt],
    imports: &[ImportEntry],
    parser: &Parser,
) -> Result<(), ParseError> {
    let mut lexical = std::collections::HashSet::new();
    for import in imports {
        let Some(name) = &import.local_name else {
            continue;
        };
        if matches!(name.as_str(), "eval" | "arguments") {
            return Err(parser.syntax_error("module import binds eval or arguments"));
        }
        if !lexical.insert(name.clone()) {
            return Err(parser.syntax_error("duplicate module lexical declaration"));
        }
    }
    let mut vars = std::collections::HashSet::new();
    for statement in body {
        match statement {
            Stmt::VarDecl(kind, declarations) => {
                for declaration in declarations {
                    for name in pattern_bound_names(&declaration.pattern) {
                        if *kind == DeclKind::Var {
                            vars.insert(name);
                        } else if !lexical.insert(name) {
                            return Err(parser.syntax_error("duplicate module lexical declaration"));
                        }
                    }
                }
            }
            Stmt::FunctionDecl(function) => {
                let name = function.name.as_ref().expect("declaration has a name");
                if !lexical.insert(name.clone()) {
                    return Err(parser.syntax_error("duplicate module lexical declaration"));
                }
            }
            Stmt::ModuleDefaultFunction { binding, .. } if !lexical.insert(binding.clone()) => {
                return Err(parser.syntax_error("duplicate module lexical declaration"));
            }
            Stmt::ClassDecl(class) => {
                let name = class.name.as_ref().expect("declaration has a name");
                if !lexical.insert(name.clone()) {
                    return Err(parser.syntax_error("duplicate module lexical declaration"));
                }
            }
            _ => {}
        }
    }
    if lexical.iter().any(|name| vars.contains(name)) {
        return Err(
            parser.syntax_error("module lexical declaration conflicts with a var declaration")
        );
    }
    Ok(())
}

pub(super) fn pattern_bound_names(pattern: &Pattern) -> Vec<String> {
    match pattern {
        Pattern::Identifier(name) => vec![name.clone()],
        Pattern::Array(elements) => elements
            .iter()
            .flatten()
            .flat_map(|element| pattern_bound_names(&element.pattern))
            .collect(),
        Pattern::Object(properties) => properties
            .iter()
            .flat_map(|property| match property {
                ObjectPatternProp::KeyValue { value, .. } | ObjectPatternProp::Rest(value) => {
                    pattern_bound_names(value)
                }
            })
            .collect(),
    }
}
