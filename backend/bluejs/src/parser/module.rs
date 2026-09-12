// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Parses one ECMAScript module and retains its declarative import/export
/// entries separately from executable statements. Linking remains host-driven
/// through [`crate::Vm::execute_module_graph`], but this distinct goal keeps
/// module strictness and top-level syntax separate from classic scripts.
pub fn parse_module(source: &str) -> Result<Module, ParseError> {
    let mut parser = Parser::new_module(source);
    parser.module_await = true;
    parser.module = true;
    parser.strict = true;
    let mut body = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut requests = Vec::new();
    while !parser.at_eof() {
        if parser.check_identifier("import")
            && !parser.check_punct_at(1, Punct::LParen)
            && !parser.check_punct_at(1, Punct::Dot)
        {
            let declaration = parser.parse_import_declaration()?;
            if let Some(request) = declaration
                .first()
                .filter(|request| !matches!(request.import_name, ImportName::Source))
            {
                requests.push(request.module_request.clone());
            }
            imports.extend(declaration);
        } else if parser.check_identifier("export") {
            if let Some(request) = parser.parse_export_declaration(&mut body, &mut exports)? {
                requests.push(request);
            }
        } else {
            body.push(parser.parse_statement()?);
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
    Ok(Module {
        body,
        imports,
        exports,
        requests,
    })
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
