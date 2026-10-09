// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub(super) mod classes;
mod computed_fields;
mod module_exports;
mod namespaces;

use classes::*;
use namespaces::*;

use super::expression::ExpressionLowerer;
use super::*;
use blueice_bluets::{
    evaluate_enums, evaluate_enums_in, has_runtime_values, rewrite_declaration, BodyInput,
    NamespaceExports,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn lower_script(
    module: &Module,
    define_class_fields: bool,
) -> Result<(Vec<bluejs::Stmt>, Vec<LoweringProvenance>), BridgeError> {
    let mut body = Vec::new();
    let mut provenance = Vec::new();
    let mut evaluations = evaluate_enums(module).into_iter();
    let tokens = lex_module(module)?;
    let namespaces = NamespaceLowering {
        module,
        tokens: &tokens,
        exports: NamespaceExports::of(module),
        define_class_fields,
    };
    // The names a namespace may merge into, declared so far at run time.
    let mut declared: BTreeSet<String> = BTreeSet::new();
    for original in &module.declarations {
        let declaration =
            &rewrite_declaration(original, &BTreeMap::new()).map_err(from_diagnostic)?;
        match declaration {
            Declaration::Ambient(_)
            | Declaration::TypeAlias(_)
            | Declaration::Interface(_)
            | Declaration::TypeExport(_)
            | Declaration::UmdExport(_) => {}
            Declaration::Variable(variable) if !variable.declared && !variable.exported => {
                body.push(lower_variable(module, variable)?);
                provenance.push((variable.span.clone(), LoweringProvenanceKind::LoweredSyntax));
            }
            Declaration::Raw(raw) => {
                body.push(bluejs::Stmt::Expr(
                    ExpressionLowerer::for_module(module, &raw.tokens).parse()?,
                ));
                provenance.push((raw.span.clone(), LoweringProvenanceKind::Copied));
            }
            Declaration::Import(import) if import.is_type_only() => {}
            Declaration::DefaultExport(export) => {
                return Err(unsupported(
                    export.span.clone(),
                    "ESM default exports require the module bridge",
                ));
            }
            Declaration::ValueExport(export) if export.is_type_only() => {}
            Declaration::ValueExport(export) => {
                return Err(unsupported(
                    export.span.clone(),
                    "ESM named exports require the module bridge",
                ));
            }
            Declaration::Import(import) => {
                return Err(unsupported(
                    import.span.clone(),
                    "runtime imports require the module bridge",
                ));
            }
            Declaration::Variable(variable) => {
                return Err(unsupported(
                    variable.span.clone(),
                    "declared or exported variables require a non-script bridge mode",
                ));
            }
            Declaration::Function(function)
                if !function.exported
                    && !function.default_export
                    && !function.declared
                    && !function.overload =>
            {
                body.push(lower_function(module, function)?);
                declared.insert(function.name.clone());
                provenance.push((function.span.clone(), LoweringProvenanceKind::LoweredSyntax));
            }
            Declaration::Function(function) => {
                return Err(unsupported(
                    function.span.clone(),
                    "declared, overloaded, or exported functions require a non-script bridge mode",
                ));
            }
            Declaration::Class(class) if !class.exported => {
                declared.insert(class.name.clone());
                body.push(lower_class(module, class, define_class_fields)?);
                provenance.push((class.span.clone(), LoweringProvenanceKind::LoweredSyntax));
            }
            Declaration::Class(class) => {
                return Err(unsupported(
                    class.span.clone(),
                    "an exported class requires the module bridge",
                ));
            }
            Declaration::Namespace(namespace) if namespace.exported => {
                return Err(unsupported(
                    namespace.span.clone(),
                    "an exported namespace requires the module bridge",
                ));
            }
            Declaration::Namespace(namespace) => {
                for statement in namespaces.namespace(
                    namespace,
                    &NamespaceScope::Module,
                    &mut declared,
                    "",
                    &BTreeMap::new(),
                )? {
                    body.push(statement);
                    provenance.push((
                        namespace.span.clone(),
                        LoweringProvenanceKind::LoweredSyntax,
                    ));
                }
            }
            Declaration::Enum(declaration) => {
                if declaration.exported {
                    return Err(unsupported(
                        declaration.span.clone(),
                        "an exported enum requires the module bridge",
                    ));
                }
                if let Some(statement) = lower_enum(module, declaration, &mut evaluations)? {
                    declared.insert(declaration.name.clone());
                    body.push(statement);
                    provenance.push((
                        declaration.span.clone(),
                        LoweringProvenanceKind::LoweredSyntax,
                    ));
                }
            }
        }
    }
    computed_fields::root(module, &mut body, &mut provenance);
    Ok((body, finalize_provenance(module, provenance)?))
}

pub(super) fn lower_module(
    project: Option<&Project>,
    module: &Module,
    define_class_fields: bool,
) -> Result<(bluejs::Module, Vec<LoweringProvenance>), BridgeError> {
    let mut body = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut requests = Vec::new();
    let mut provenance = Vec::new();
    let mut evaluations = evaluate_enums(module).into_iter();
    let tokens = lex_module(module)?;
    let namespaces = NamespaceLowering {
        module,
        tokens: &tokens,
        exports: NamespaceExports::of(module),
        define_class_fields,
    };
    let mut declared: BTreeSet<String> = BTreeSet::new();
    for original in &module.declarations {
        let declaration =
            &rewrite_declaration(original, &BTreeMap::new()).map_err(from_diagnostic)?;
        match declaration {
            Declaration::Ambient(_)
            | Declaration::TypeAlias(_)
            | Declaration::Interface(_)
            | Declaration::TypeExport(_)
            | Declaration::UmdExport(_) => {}
            Declaration::Variable(variable) if !variable.declared => {
                body.push(module_exports::variable(module, variable)?);
                provenance.push((variable.span.clone(), LoweringProvenanceKind::LoweredSyntax));
                if variable.exported {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: variable.name.clone(),
                        local_name: variable.name.clone(),
                    });
                }
            }
            Declaration::Function(function) if !function.declared && !function.overload => {
                if function.default_export && function.anonymous {
                    body.push(module_exports::anonymous_function(module, function)?);
                } else {
                    body.push(lower_function(module, function)?);
                }
                declared.insert(function.name.clone());
                provenance.push((function.span.clone(), LoweringProvenanceKind::LoweredSyntax));
                if function.default_export {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: "default".to_string(),
                        local_name: if function.anonymous {
                            module_exports::DEFAULT_BINDING.to_string()
                        } else {
                            function.name.clone()
                        },
                    });
                } else if function.exported {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: function.name.clone(),
                        local_name: function.name.clone(),
                    });
                }
            }
            Declaration::Raw(raw) => {
                body.push(bluejs::Stmt::Expr(
                    ExpressionLowerer::for_module(module, &raw.tokens).parse()?,
                ));
                provenance.push((raw.span.clone(), LoweringProvenanceKind::Copied));
            }
            Declaration::DefaultExport(export) => {
                let local_name = module_exports::DEFAULT_BINDING.to_string();
                if !export.expression {
                    body.push(bluejs::Stmt::VarDecl(
                        bluejs::DeclKind::Const,
                        vec![bluejs::VarDeclarator {
                            pattern: bluejs::Pattern::Identifier(local_name.clone()),
                            init: Some(bluejs::Expr::Identifier(export.name.clone())),
                        }],
                    ));
                }
                exports.push(bluejs::ExportEntry::Local {
                    export_name: "default".to_string(),
                    local_name,
                });
            }
            Declaration::ValueExport(export) if export.is_type_only() => {}
            Declaration::ValueExport(export) => {
                if let Some(specifier) = &export.specifier {
                    let request = project
                        .and_then(|project| project.resolved_module(&module.id, specifier))
                        .ok_or_else(|| {
                            unsupported(
                                export.span.clone(),
                                "re-exports require a retained module-graph edge",
                            )
                        })?
                        .to_string();
                    if !requests.contains(&request) {
                        requests.push(request.clone());
                    }
                    if let Some(name) = &export.namespace {
                        exports.push(bluejs::ExportEntry::Namespace {
                            export_name: name.clone(),
                            module_request: request,
                            module_type: bluejs::ModuleType::JavaScript,
                        });
                    } else if export.star {
                        exports.push(bluejs::ExportEntry::Star {
                            module_request: request,
                            module_type: bluejs::ModuleType::JavaScript,
                        });
                    } else {
                        exports.extend(
                            export
                                .bindings
                                .iter()
                                .filter(|binding| !binding.type_only)
                                .map(|binding| bluejs::ExportEntry::Indirect {
                                    export_name: binding.exported.clone(),
                                    module_request: request.clone(),
                                    import_name: binding.local.clone(),
                                    module_type: bluejs::ModuleType::JavaScript,
                                }),
                        );
                    }
                    continue;
                }
                exports.extend(
                    export
                        .bindings
                        .iter()
                        .filter(|binding| !binding.type_only)
                        .map(|binding| bluejs::ExportEntry::Local {
                            export_name: binding.exported.clone(),
                            local_name: binding.local.clone(),
                        }),
                );
            }
            Declaration::Import(import) if import.is_type_only() => {}
            Declaration::Import(import) => {
                let Some(project) = project else {
                    return Err(unsupported(
                        import.span.clone(),
                        "runtime imports require the direct module-graph bridge",
                    ));
                };
                let request = project
                    .resolved_module(&module.id, &import.specifier)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        unsupported(
                            import.specifier_span.clone(),
                            "BlueTS did not retain a canonical target for this runtime import",
                        )
                    })?;
                if !requests.contains(&request) {
                    requests.push(request.clone());
                }
                if import.bindings.is_empty() {
                    imports.push(bluejs::ImportEntry {
                        module_request: request,
                        import_name: bluejs::ImportName::Named("default".to_string()),
                        local_name: None,
                        module_type: bluejs::ModuleType::JavaScript,
                    });
                } else {
                    imports.extend(
                        import
                            .bindings
                            .iter()
                            .filter(|binding| !binding.type_only)
                            .map(|binding| bluejs::ImportEntry {
                                module_request: request.clone(),
                                import_name: if binding.imported == "*" {
                                    bluejs::ImportName::Namespace
                                } else {
                                    bluejs::ImportName::Named(binding.imported.clone())
                                },
                                local_name: Some(binding.local.clone()),
                                module_type: bluejs::ModuleType::JavaScript,
                            }),
                    );
                }
            }
            Declaration::Variable(variable) => {
                return Err(unsupported(
                    variable.span.clone(),
                    "declared variables cannot be lowered to a direct module",
                ));
            }
            Declaration::Function(function) => {
                return Err(unsupported(
                    function.span.clone(),
                    "declared or overloaded functions cannot be lowered to a direct module",
                ));
            }
            Declaration::Class(class) => {
                declared.insert(class.name.clone());
                if class.default_export && class.anonymous {
                    body.push(module_exports::anonymous_class(
                        module,
                        class,
                        define_class_fields,
                    )?);
                } else {
                    body.push(lower_class(module, class, define_class_fields)?);
                }
                provenance.push((class.span.clone(), LoweringProvenanceKind::LoweredSyntax));
                if class.exported {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: class.export_name().to_string(),
                        local_name: if class.default_export && class.anonymous {
                            module_exports::DEFAULT_BINDING.to_string()
                        } else {
                            class.name.clone()
                        },
                    });
                }
            }
            Declaration::Namespace(namespace) => {
                let statements = namespaces.namespace(
                    namespace,
                    &NamespaceScope::Module,
                    &mut declared,
                    "",
                    &BTreeMap::new(),
                )?;
                if !statements.is_empty()
                    && namespace.exported
                    && !exports.iter().any(|entry| {
                        matches!(entry, bluejs::ExportEntry::Local { export_name, .. }
                            if *export_name == namespace.name)
                    })
                {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: namespace.name.clone(),
                        local_name: namespace.name.clone(),
                    });
                }
                for statement in statements {
                    body.push(statement);
                    provenance.push((
                        namespace.span.clone(),
                        LoweringProvenanceKind::LoweredSyntax,
                    ));
                }
            }
            Declaration::Enum(declaration) => {
                if let Some(statement) = lower_enum(module, declaration, &mut evaluations)? {
                    declared.insert(declaration.name.clone());
                    body.push(statement);
                    provenance.push((
                        declaration.span.clone(),
                        LoweringProvenanceKind::LoweredSyntax,
                    ));
                    if declaration.exported
                        && !exports.iter().any(|entry| {
                            matches!(entry, bluejs::ExportEntry::Local { export_name, .. }
                                if *export_name == declaration.name)
                        })
                    {
                        exports.push(bluejs::ExportEntry::Local {
                            export_name: declaration.name.clone(),
                            local_name: declaration.name.clone(),
                        });
                    }
                }
            }
        }
    }
    computed_fields::root(module, &mut body, &mut provenance);
    Ok((
        bluejs::Module {
            body,
            imports,
            exports,
            // BlueTS has no `import defer` / source-phase syntax: every runtime
            // import is an ordinary evaluation-phase request.
            requests: requests
                .into_iter()
                .map(|specifier| bluejs::RequestedModule {
                    specifier,
                    module_type: bluejs::ModuleType::JavaScript,
                    phase: bluejs::ImportPhase::Evaluation,
                })
                .collect(),
        },
        finalize_provenance(module, provenance)?,
    ))
}

fn finalize_provenance(
    module: &Module,
    raw: Vec<(SourceSpan, LoweringProvenanceKind)>,
) -> Result<Vec<LoweringProvenance>, BridgeError> {
    let locations = source_locations_for_spans(&module.source, raw.iter().map(|(span, _)| span));
    raw.into_iter()
        .zip(locations)
        .map(|((source, kind), location)| {
            let location = location
                .filter(|_| source.module == module.id && source.start < source.end)
                .ok_or_else(|| {
                    BridgeError::ProvenanceAttachment(
                        "a lowered statement has no exact original-source location".to_string(),
                    )
                })?;
            Ok(LoweringProvenance {
                source,
                kind,
                location,
            })
        })
        .collect()
}

fn lower_function(
    module: &Module,
    function: &FunctionDeclaration,
) -> Result<bluejs::Stmt, BridgeError> {
    Ok(bluejs::Stmt::FunctionDecl(lower_function_value(
        module,
        Some(function.name.clone()),
        &function.parameters,
        &function.body,
        function.async_function,
        function.generator,
    )?))
}

fn lower_function_value(
    module: &Module,
    name: Option<String>,
    parameters: &[Parameter],
    body_items: &[FunctionBodyItem],
    is_async: bool,
    generator: bool,
) -> Result<bluejs::Function, BridgeError> {
    let mut params = Vec::with_capacity(parameters.len());
    for (index, parameter) in parameters.iter().enumerate() {
        if parameter.pattern.is_some() {
            return Err(unsupported(
                parameter.span.clone(),
                "destructured parameters are not yet in the v1 direct bridge subset",
            ));
        }
        if parameter.rest && index + 1 != parameters.len() {
            return Err(unsupported(
                parameter.span.clone(),
                "a rest parameter must be the final direct function parameter",
            ));
        }
        params.push(bluejs::Param {
            pattern: bluejs::Pattern::Identifier(parameter.name.clone()),
            default: parameter
                .default
                .as_deref()
                .map(|tokens| ExpressionLowerer::for_module(module, tokens).parse())
                .transpose()?,
            rest: parameter.rest,
        });
    }

    let body = lower_function_body(module, body_items)?;

    Ok(bluejs::Function {
        name,
        params,
        body,
        generator,
        is_async,
        // This function is synthesized from BlueTSC's own lowered AST, not
        // parsed from BlueJS-tokenized source text, so it has no
        // `[[SourceText]]`: `Function.prototype.toString` reports it as a
        // NativeFunction, matching how the compiler treats every other
        // synthesized function.
        source_text: Default::default(),
    })
}

fn lower_function_body(
    module: &Module,
    items: &[FunctionBodyItem],
) -> Result<Vec<bluejs::Stmt>, BridgeError> {
    let mut body = Vec::with_capacity(items.len());
    for item in items {
        match item {
            FunctionBodyItem::Variable(variable) => body.push(lower_variable(module, variable)?),
            FunctionBodyItem::Expression { tokens, .. } => body.push(bluejs::Stmt::Expr(
                ExpressionLowerer::for_module(module, tokens).parse()?,
            )),
            FunctionBodyItem::Throw { tokens, .. } => body.push(bluejs::Stmt::Throw(
                ExpressionLowerer::for_module(module, tokens).parse()?,
            )),
            FunctionBodyItem::Return { tokens, .. } => {
                let value = (!tokens.is_empty())
                    .then(|| ExpressionLowerer::for_module(module, tokens).parse())
                    .transpose()?;
                body.push(bluejs::Stmt::Return(value));
            }
            FunctionBodyItem::If(statement) => body.push(lower_function_if(module, statement)?),
            FunctionBodyItem::While(statement) => {
                body.push(lower_function_while(module, statement)?)
            }
            FunctionBodyItem::Try(statement) => {
                body.push(lower_function_try(module, statement)?);
            }
            FunctionBodyItem::Function(function) => {
                return Err(unsupported(
                    function.span.clone(),
                    "nested function declarations are not yet in the v1 direct bridge subset",
                ));
            }
            FunctionBodyItem::Opaque(span) => {
                return Err(unsupported(
                    span.clone(),
                    "function body syntax is not yet in the v1 direct bridge subset",
                ));
            }
        }
    }
    computed_fields::body(module, items, &mut body);
    Ok(body)
}

fn lower_function_if(
    module: &Module,
    statement: &FunctionIfStatement,
) -> Result<bluejs::Stmt, BridgeError> {
    let test = ExpressionLowerer::for_module(module, &statement.test).parse()?;
    let consequent = Box::new(bluejs::Stmt::Block(lower_function_body(
        module,
        &statement.consequent,
    )?));
    let alternate = match &statement.alternate {
        Some(FunctionElseBranch::Braced(alternate)) => Some(Box::new(bluejs::Stmt::Block(
            lower_function_body(module, alternate)?,
        ))),
        Some(FunctionElseBranch::ElseIf(alternate)) => {
            Some(Box::new(lower_function_if(module, alternate)?))
        }
        None => None,
    };
    Ok(bluejs::Stmt::If {
        test,
        consequent,
        alternate,
    })
}

fn lower_function_while(
    module: &Module,
    statement: &FunctionWhileStatement,
) -> Result<bluejs::Stmt, BridgeError> {
    ensure_supported_while_body(&statement.body)?;
    let test = ExpressionLowerer::for_module(module, &statement.test).parse()?;
    let body = bluejs::Stmt::Block(lower_function_body(module, &statement.body)?);
    Ok(bluejs::Stmt::While {
        test,
        body: Box::new(body),
    })
}

fn lower_function_try(
    module: &Module,
    statement: &FunctionTryStatement,
) -> Result<bluejs::Stmt, BridgeError> {
    ensure_supported_try_body(&statement.block)?;
    if let Some(handler) = &statement.handler {
        ensure_supported_try_body(&handler.body)?;
    }
    if let Some(finalizer) = &statement.finalizer {
        ensure_supported_try_body(finalizer)?;
    }
    let block = lower_function_body(module, &statement.block)?;
    let handler = statement
        .handler
        .as_ref()
        .map(|handler| {
            Ok(bluejs::CatchClause {
                param: Some(bluejs::Pattern::Identifier(handler.binding.clone())),
                body: lower_function_body(module, &handler.body)?,
            })
        })
        .transpose()?;
    let finalizer = statement
        .finalizer
        .as_ref()
        .map(|body| lower_function_body(module, body))
        .transpose()?;
    Ok(bluejs::Stmt::Try {
        block,
        handler,
        finalizer,
    })
}

fn ensure_supported_try_body(items: &[FunctionBodyItem]) -> Result<(), BridgeError> {
    for item in items {
        match item {
            FunctionBodyItem::Variable(variable) => {
                return Err(unsupported(
                    variable.span.clone(),
                    "block-local declarations are outside the direct try subset",
                ));
            }
            FunctionBodyItem::While(statement) => {
                return Err(unsupported(
                    statement.span.clone(),
                    "loops are outside the direct try subset",
                ));
            }
            FunctionBodyItem::Try(statement) => {
                return Err(unsupported(
                    statement.span.clone(),
                    "nested try statements are outside the direct try subset",
                ));
            }
            FunctionBodyItem::If(statement) => ensure_supported_try_if(statement)?,
            FunctionBodyItem::Function(function) => {
                return Err(unsupported(
                    function.span.clone(),
                    "a nested function declaration is outside the direct try subset",
                ));
            }
            FunctionBodyItem::Opaque(span) => {
                return Err(unsupported(
                    span.clone(),
                    "body syntax is outside the direct try subset",
                ));
            }
            FunctionBodyItem::Expression { .. }
            | FunctionBodyItem::Throw { .. }
            | FunctionBodyItem::Return { .. } => {}
        }
    }
    Ok(())
}

fn ensure_supported_try_if(statement: &FunctionIfStatement) -> Result<(), BridgeError> {
    ensure_supported_try_body(&statement.consequent)?;
    match &statement.alternate {
        Some(FunctionElseBranch::Braced(body)) => ensure_supported_try_body(body),
        Some(FunctionElseBranch::ElseIf(branch)) => ensure_supported_try_if(branch),
        None => Ok(()),
    }
}

fn ensure_supported_while_body(items: &[FunctionBodyItem]) -> Result<(), BridgeError> {
    for item in items {
        match item {
            FunctionBodyItem::Variable(variable) => {
                return Err(unsupported(
                    variable.span.clone(),
                    "loop-local declarations are outside the direct while subset",
                ));
            }
            FunctionBodyItem::While(statement) => {
                return Err(unsupported(
                    statement.span.clone(),
                    "nested loops are outside the direct while subset",
                ));
            }
            FunctionBodyItem::Try(statement) => {
                return Err(unsupported(
                    statement.span.clone(),
                    "try statements are outside the direct while body subset",
                ));
            }
            FunctionBodyItem::If(statement) => {
                ensure_supported_while_if(statement)?;
            }
            FunctionBodyItem::Function(function) => {
                return Err(unsupported(
                    function.span.clone(),
                    "a nested function declaration is outside the direct while subset",
                ));
            }
            FunctionBodyItem::Opaque(span) => {
                return Err(unsupported(
                    span.clone(),
                    "loop body syntax is outside the direct while subset",
                ));
            }
            FunctionBodyItem::Expression { .. }
            | FunctionBodyItem::Throw { .. }
            | FunctionBodyItem::Return { .. } => {}
        }
    }
    Ok(())
}

fn ensure_supported_while_if(statement: &FunctionIfStatement) -> Result<(), BridgeError> {
    ensure_supported_while_body(&statement.consequent)?;
    match &statement.alternate {
        Some(FunctionElseBranch::Braced(body)) => ensure_supported_while_body(body),
        Some(FunctionElseBranch::ElseIf(branch)) => ensure_supported_while_if(branch),
        None => Ok(()),
    }
}

fn lower_variable(
    module: &Module,
    variable: &VariableDeclaration,
) -> Result<bluejs::Stmt, BridgeError> {
    let init = (!variable.initializer.is_empty())
        .then(|| ExpressionLowerer::for_module(module, &variable.initializer).parse())
        .transpose()?;
    Ok(bluejs::Stmt::VarDecl(
        match variable.kind {
            VariableKind::Const => bluejs::DeclKind::Const,
            VariableKind::Let => bluejs::DeclKind::Let,
            VariableKind::Var => bluejs::DeclKind::Var,
        },
        vec![bluejs::VarDeclarator {
            pattern: bluejs::Pattern::Identifier(variable.name.clone()),
            init,
        }],
    ))
}

fn lex_module(module: &Module) -> Result<Vec<Token>, BridgeError> {
    blueice_bluets::lex(&module.id, &module.source).map_err(BridgeError::BlueTs)
}

fn from_diagnostic(diagnostic: blueice_bluets::Diagnostic) -> BridgeError {
    unsupported(diagnostic.span, diagnostic.message)
}
