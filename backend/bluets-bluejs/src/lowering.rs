// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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
            Declaration::TypeAlias(_) | Declaration::Interface(_) | Declaration::TypeExport(_) => {}
            Declaration::Variable(variable) if !variable.declared && !variable.exported => {
                body.push(lower_variable(module, variable)?);
                provenance.push((variable.span.clone(), LoweringProvenanceKind::LoweredSyntax));
            }
            Declaration::Raw(raw) => {
                body.push(bluejs::Stmt::Expr(
                    ExpressionLowerer::new(&module.id, &raw.tokens).parse()?,
                ));
                provenance.push((raw.span.clone(), LoweringProvenanceKind::Copied));
            }
            Declaration::Import(import) if import.type_only => {}
            Declaration::DefaultExport(export) => {
                return Err(unsupported(
                    export.span.clone(),
                    "ESM default exports require the module bridge",
                ));
            }
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
            Declaration::TypeAlias(_) | Declaration::Interface(_) | Declaration::TypeExport(_) => {}
            Declaration::Variable(variable) if !variable.declared => {
                body.push(lower_variable(module, variable)?);
                provenance.push((variable.span.clone(), LoweringProvenanceKind::LoweredSyntax));
                if variable.exported {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: variable.name.clone(),
                        local_name: variable.name.clone(),
                    });
                }
            }
            Declaration::Function(function) if !function.declared && !function.overload => {
                body.push(lower_function(module, function)?);
                declared.insert(function.name.clone());
                provenance.push((function.span.clone(), LoweringProvenanceKind::LoweredSyntax));
                if function.default_export {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: "default".to_string(),
                        local_name: function.name.clone(),
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
                    ExpressionLowerer::new(&module.id, &raw.tokens).parse()?,
                ));
                provenance.push((raw.span.clone(), LoweringProvenanceKind::Copied));
            }
            Declaration::DefaultExport(export) => exports.push(bluejs::ExportEntry::Local {
                export_name: "default".to_string(),
                local_name: export.name.clone(),
            }),
            Declaration::ValueExport(export) => {
                exports.extend(
                    export
                        .bindings
                        .iter()
                        .map(|binding| bluejs::ExportEntry::Local {
                            export_name: binding.exported.clone(),
                            local_name: binding.local.clone(),
                        }),
                );
            }
            Declaration::Import(import) if import.type_only => {}
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
                    imports.extend(import.bindings.iter().map(|binding| bluejs::ImportEntry {
                        module_request: request.clone(),
                        import_name: if binding.imported == "*" {
                            bluejs::ImportName::Namespace
                        } else {
                            bluejs::ImportName::Named(binding.imported.clone())
                        },
                        local_name: Some(binding.local.clone()),
                        module_type: bluejs::ModuleType::JavaScript,
                    }));
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
                body.push(lower_class(module, class, define_class_fields)?);
                provenance.push((class.span.clone(), LoweringProvenanceKind::LoweredSyntax));
                if class.exported {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: class.name.clone(),
                        local_name: class.name.clone(),
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
    )?))
}

/// A class as a BlueJS class declaration. Overload signatures have no runtime
/// form and are skipped; the constructor is the non-static method named
/// `constructor`, as BlueJS represents it.
///
/// With `define_class_fields` the fields are BlueJS class fields, which have
/// define semantics. Without it they are lowered as TypeScript does for
/// `useDefineForClassFields: false`: an instance field becomes `this.x = init`
/// at the start of the constructor (after `super(...)`), a static field a
/// static block that assigns it, and a field with no initializer disappears.
fn lower_class(
    module: &Module,
    class: &ClassDeclaration,
    define_class_fields: bool,
) -> Result<bluejs::Stmt, BridgeError> {
    let mut elements = Vec::new();
    let properties = class.parameter_property_fields();
    // A constructor's parameter properties declare fields ahead of every other
    // member, as TypeScript emits them, when fields are defined.
    if define_class_fields {
        for field in &properties {
            elements.push(bluejs::ClassElement::Field {
                key: bluejs::PropertyKey::Identifier(field.name.clone()),
                initializer: None,
                is_static: false,
                accessor: false,
                decorators: Vec::new(),
            });
        }
    }
    // Instance fields lowered to constructor assignments, in source order. All
    // of them are known before any member is lowered, since the constructor may
    // be written ahead of the fields.
    let mut field_assignments = Vec::new();
    if !define_class_fields {
        for field in class
            .members
            .iter()
            .filter_map(|member| member.field.as_ref())
            .filter(|field| !field.is_static && !field.name.starts_with('#'))
        {
            if let Some(tokens) = field.initializer.as_deref() {
                let initializer = ExpressionLowerer::new(&module.id, tokens).parse()?;
                field_assignments.push(assign_this_member(&field.name, initializer));
            }
        }
    }
    for member in &class.members {
        if let Some(block) = &member.static_block {
            elements.push(bluejs::ClassElement::StaticBlock(lower_function_body(
                module,
                &block.body,
            )?));
            continue;
        }
        if let Some(accessor) = &member.accessor {
            elements.push(bluejs::ClassElement::Accessor {
                key: bluejs::PropertyKey::Identifier(accessor.name.clone()),
                function: lower_function_value(
                    module,
                    Some(accessor.name.clone()),
                    &accessor.parameters,
                    &accessor.body,
                )?,
                getter: accessor.getter,
                is_static: accessor.is_static,
                decorators: Vec::new(),
            });
            continue;
        }
        if let Some(field) = &member.field {
            let initializer = field
                .initializer
                .as_deref()
                .map(|tokens| ExpressionLowerer::new(&module.id, tokens).parse())
                .transpose()?;
            if define_class_fields || field.name.starts_with('#') {
                elements.push(bluejs::ClassElement::Field {
                    key: bluejs::PropertyKey::Identifier(field.name.clone()),
                    initializer,
                    is_static: field.is_static,
                    accessor: false,
                    decorators: Vec::new(),
                });
            } else if let (true, Some(initializer)) = (field.is_static, initializer) {
                elements.push(bluejs::ClassElement::StaticBlock(vec![assign_this_member(
                    &field.name,
                    initializer,
                )]));
            }
            continue;
        }
        let (name, is_static, parameters, body) = if let Some(constructor) = &member.constructor {
            (
                "constructor".to_string(),
                false,
                &constructor.parameters,
                &constructor.body,
            )
        } else if let Some(method) = &member.method {
            (
                method.name.clone(),
                method.is_static,
                &method.parameters,
                &method.body,
            )
        } else {
            return Err(unsupported(
                member.span.clone(),
                "this class member has no direct lowering",
            ));
        };
        let Some(body) = body else {
            continue;
        };
        let mut function = lower_function_value(module, Some(name.clone()), parameters, body)?;
        if let Some(constructor) = &member.constructor {
            insert_constructor_prologue(
                module,
                constructor,
                body,
                &field_assignments,
                &mut function,
            )?;
        }
        elements.push(bluejs::ClassElement::Method {
            key: bluejs::PropertyKey::Identifier(name.clone()),
            function,
            is_static,
            decorators: Vec::new(),
        });
    }
    // Instance fields lowered to assignments, but no constructor to hold them.
    let has_constructor = class.members.iter().any(|member| {
        member
            .constructor
            .as_ref()
            .is_some_and(|ctor| ctor.body.is_some())
    });
    if !field_assignments.is_empty() && !has_constructor {
        elements.insert(0, synthesized_constructor(class, field_assignments));
    }
    Ok(bluejs::Stmt::ClassDecl(bluejs::Class {
        name: Some(class.name.clone()),
        extends: class.extends_name.as_ref().map(|base| {
            // A qualified name, `N.Base`, is a member read.
            let mut names = base.split('.');
            let mut expression = bluejs::Expr::Identifier(names.next().unwrap_or(base).to_string());
            for name in names {
                expression = bluejs::Expr::Member {
                    object: Box::new(expression),
                    property: Box::new(bluejs::Expr::Identifier(name.to_string())),
                    computed: false,
                };
            }
            Box::new(expression)
        }),
        elements,
        decorators: Vec::new(),
        // Synthesized from BlueTSC's own lowered AST: no `[[SourceText]]`.
        source_text: Default::default(),
    }))
}

/// An enum as `var E = (function (E) { E[E["A"] = 0] = "A"; ...; return E; })(E || {});`.
/// One statement per declaration keeps a root statement and its source span one
/// to one, and merged declarations reuse the object through `E || {}`. A
/// `const enum` is lowered like any other: its object exists at run time and
/// every use reads it, which behaves as the inlined emit does. An ambient enum
/// has no runtime form (`None`), except an ambient `const enum`, whose uses
/// would need their values inlined, which the bridge does not do.
fn lower_enum(
    module: &Module,
    declaration: &blueice_bluets::EnumDeclaration,
    evaluations: &mut std::vec::IntoIter<blueice_bluets::EvaluatedEnum>,
) -> Result<Option<bluejs::Stmt>, BridgeError> {
    let Some(function) = lower_enum_function(module, declaration, evaluations)? else {
        return Ok(None);
    };
    let name = declaration.name.clone();
    let call = iife(function, plain_argument(&name));
    Ok(Some(bluejs::Stmt::VarDecl(
        bluejs::DeclKind::Var,
        vec![bluejs::VarDeclarator {
            pattern: bluejs::Pattern::Identifier(name),
            init: Some(call),
        }],
    )))
}

/// The function an enum declaration builds its object in.
fn lower_enum_function(
    module: &Module,
    declaration: &blueice_bluets::EnumDeclaration,
    evaluations: &mut std::vec::IntoIter<blueice_bluets::EvaluatedEnum>,
) -> Result<Option<bluejs::Function>, BridgeError> {
    let evaluation = evaluations
        .next()
        .expect("every enum declaration was evaluated");
    if declaration.declared {
        if declaration.is_const {
            return Err(unsupported(
                declaration.span.clone(),
                "an ambient const enum needs its uses inlined, which the direct bridge does not do",
            ));
        }
        return Ok(None);
    }
    let name = declaration.name.clone();
    let string = |text: &str| bluejs::Expr::String(bluejs::JsString::from(text));
    let object = || bluejs::Expr::Identifier(name.clone());
    let member_key = |member: &str| bluejs::Expr::Member {
        object: Box::new(object()),
        property: Box::new(string(member)),
        computed: true,
    };
    let mut statements = Vec::new();
    for (member, evaluated) in declaration.members.iter().zip(&evaluation.members) {
        let statement = match &evaluated.value {
            Some(blueice_bluets::EnumValue::Number(number)) => {
                // E[E["A"] = 0] = "A"
                let forward = assign_expr(member_key(&member.name), bluejs::Expr::Number(*number));
                assign_expr(
                    bluejs::Expr::Member {
                        object: Box::new(object()),
                        property: Box::new(forward),
                        computed: true,
                    },
                    string(&member.name),
                )
            }
            Some(blueice_bluets::EnumValue::Text(text)) => {
                assign_expr(member_key(&member.name), string(text))
            }
            None => {
                let value = member
                    .initializer
                    .as_deref()
                    .map(|tokens| ExpressionLowerer::new(&module.id, tokens).parse())
                    .transpose()?
                    .unwrap_or(bluejs::Expr::Identifier("undefined".to_string()));
                let forward = assign_expr(member_key(&member.name), value);
                assign_expr(
                    bluejs::Expr::Member {
                        object: Box::new(object()),
                        property: Box::new(forward),
                        computed: true,
                    },
                    string(&member.name),
                )
            }
        };
        statements.push(bluejs::Stmt::Expr(statement));
    }
    statements.push(bluejs::Stmt::Return(Some(object())));
    Ok(Some(function_over(&name, statements)))
}

fn assign_expr(target: bluejs::Expr, value: bluejs::Expr) -> bluejs::Expr {
    bluejs::Expr::Assign {
        op: bluejs::AssignOp::Assign,
        target: Box::new(target),
        value: Box::new(value),
    }
}

fn identifier_expr(name: &str) -> bluejs::Expr {
    bluejs::Expr::Identifier(name.to_string())
}

fn property_expr(object: &str, property: &str) -> bluejs::Expr {
    bluejs::Expr::Member {
        object: Box::new(identifier_expr(object)),
        property: Box::new(identifier_expr(property)),
        computed: false,
    }
}

/// `function (name) { .. }`, the function a namespace or enum body runs in.
fn function_over(name: &str, body: Vec<bluejs::Stmt>) -> bluejs::Function {
    bluejs::Function {
        name: None,
        params: vec![bluejs::Param {
            pattern: bluejs::Pattern::Identifier(name.to_string()),
            default: None,
            rest: false,
        }],
        body,
        generator: false,
        is_async: false,
        source_text: Default::default(),
    }
}

fn iife(function: bluejs::Function, argument: bluejs::Expr) -> bluejs::Expr {
    bluejs::Expr::Call {
        callee: Box::new(bluejs::Expr::Parenthesized(Box::new(
            bluejs::Expr::Function(function),
        ))),
        args: vec![bluejs::Argument::Normal(argument)],
    }
}

/// `name || (name = {})`
fn plain_argument(name: &str) -> bluejs::Expr {
    bluejs::Expr::Logical {
        op: bluejs::LogicalOp::Or,
        left: Box::new(identifier_expr(name)),
        right: Box::new(bluejs::Expr::Parenthesized(Box::new(assign_expr(
            identifier_expr(name),
            bluejs::Expr::Object(Vec::new()),
        )))),
    }
}

/// `name = parent.name || (parent.name = {})`
fn member_argument(parent: &str, name: &str) -> bluejs::Expr {
    assign_expr(
        identifier_expr(name),
        bluejs::Expr::Logical {
            op: bluejs::LogicalOp::Or,
            left: Box::new(property_expr(parent, name)),
            right: Box::new(bluejs::Expr::Parenthesized(Box::new(assign_expr(
                property_expr(parent, name),
                bluejs::Expr::Object(Vec::new()),
            )))),
        },
    )
}

/// `this.name = value;`
fn assign_this_member(name: &str, value: bluejs::Expr) -> bluejs::Stmt {
    bluejs::Stmt::Expr(bluejs::Expr::Assign {
        op: bluejs::AssignOp::Assign,
        target: Box::new(bluejs::Expr::Member {
            object: Box::new(bluejs::Expr::This),
            property: Box::new(bluejs::Expr::Identifier(name.to_string())),
            computed: false,
        }),
        value: Box::new(value),
    })
}

/// The constructor a class with lowered instance fields needs when it declares
/// none: `constructor() { .. }`, or in a derived class
/// `constructor(...args) { super(...args); .. }`.
fn synthesized_constructor(
    class: &ClassDeclaration,
    assignments: Vec<bluejs::Stmt>,
) -> bluejs::ClassElement {
    let derived = class.extends_name.is_some();
    let mut body = Vec::new();
    let mut params = Vec::new();
    if derived {
        params.push(bluejs::Param {
            pattern: bluejs::Pattern::Identifier("args".to_string()),
            default: None,
            rest: true,
        });
        body.push(bluejs::Stmt::Expr(bluejs::Expr::Call {
            callee: Box::new(bluejs::Expr::Super),
            args: vec![bluejs::Argument::Spread(bluejs::Expr::Identifier(
                "args".to_string(),
            ))],
        }));
    }
    body.extend(assignments);
    bluejs::ClassElement::Method {
        key: bluejs::PropertyKey::Identifier("constructor".to_string()),
        function: bluejs::Function {
            name: Some("constructor".to_string()),
            params,
            body,
            generator: false,
            is_async: false,
            source_text: Default::default(),
        },
        is_static: false,
        decorators: Vec::new(),
    }
}

/// The statements a constructor starts with, after `super(...)` in a derived
/// class: `this.p = p;` for each parameter property, then the lowered field
/// assignments. They go where the emitter inserts the same text.
fn insert_constructor_prologue(
    module: &Module,
    constructor: &ClassConstructor,
    body_items: &[FunctionBodyItem],
    field_assignments: &[bluejs::Stmt],
    function: &mut bluejs::Function,
) -> Result<(), BridgeError> {
    if constructor.parameter_properties.is_empty() && field_assignments.is_empty() {
        return Ok(());
    }
    let Some(insertion) = constructor.prologue_insertion else {
        return Err(unsupported(
            constructor.span.clone(),
            "a derived constructor needs a top-level super call for the statements that must follow it",
        ));
    };
    let mut prologue: Vec<bluejs::Stmt> = constructor
        .parameter_properties
        .iter()
        .map(|property| {
            let name = constructor.parameters[property.parameter_index]
                .name
                .clone();
            assign_this_member(&name, bluejs::Expr::Identifier(name.clone()))
        })
        .collect();
    prologue.extend(field_assignments.iter().cloned());
    let mut body = lower_function_body(module, &body_items[..insertion.item_index])?;
    body.extend(prologue);
    body.extend(lower_function_body(
        module,
        &body_items[insertion.item_index..],
    )?);
    function.body = body;
    Ok(())
}

fn lower_function_value(
    module: &Module,
    name: Option<String>,
    parameters: &[Parameter],
    body_items: &[FunctionBodyItem],
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
                .map(|tokens| ExpressionLowerer::new(&module.id, tokens).parse())
                .transpose()?,
            rest: parameter.rest,
        });
    }

    let body = lower_function_body(module, body_items)?;

    Ok(bluejs::Function {
        name,
        params,
        body,
        generator: false,
        is_async: false,
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
                ExpressionLowerer::new(&module.id, tokens).parse()?,
            )),
            FunctionBodyItem::Throw { tokens, .. } => body.push(bluejs::Stmt::Throw(
                ExpressionLowerer::new(&module.id, tokens).parse()?,
            )),
            FunctionBodyItem::Return { tokens, .. } => {
                let value = (!tokens.is_empty())
                    .then(|| ExpressionLowerer::new(&module.id, tokens).parse())
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
    Ok(body)
}

fn lower_function_if(
    module: &Module,
    statement: &FunctionIfStatement,
) -> Result<bluejs::Stmt, BridgeError> {
    let test = ExpressionLowerer::new(&module.id, &statement.test).parse()?;
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
    let test = ExpressionLowerer::new(&module.id, &statement.test).parse()?;
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
        .then(|| ExpressionLowerer::new(&module.id, &variable.initializer).parse())
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

/// Where a namespace is declared.
enum NamespaceScope {
    Module,
    Namespace { parent: String },
}

/// Lowers namespaces the way TypeScript emits them: a function over the
/// namespace object, called with the object or a new one, in which an exported
/// variable is a property and every reference to it a property read.
struct NamespaceLowering<'a> {
    module: &'a Module,
    tokens: &'a [Token],
    exports: NamespaceExports,
    define_class_fields: bool,
}

impl NamespaceLowering<'_> {
    /// The statements for one namespace declaration, none when it has nothing at
    /// run time or is ambient.
    fn namespace(
        &self,
        namespace: &blueice_bluets::NamespaceDeclaration,
        scope: &NamespaceScope,
        declared: &mut BTreeSet<String>,
        parent_path: &str,
        outer: &BTreeMap<String, String>,
    ) -> Result<Vec<bluejs::Stmt>, BridgeError> {
        if namespace.declared || !has_runtime_values(&namespace.body) {
            return Ok(Vec::new());
        }
        let mut chain = vec![namespace];
        while let [Declaration::Namespace(inner)] = chain[chain.len() - 1].body.as_slice() {
            if !inner.implicit {
                break;
            }
            chain.push(inner);
        }
        let innermost = chain[chain.len() - 1];
        let first = declared.insert(namespace.name.clone());
        let param = innermost.name.clone();
        let own_path = chain.iter().fold(parent_path.to_string(), |path, segment| {
            if path.is_empty() {
                segment.name.clone()
            } else {
                format!("{path}.{}", segment.name)
            }
        });
        let references = self
            .exports
            .references(
                self.module,
                self.tokens,
                &BodyInput {
                    own_path: &own_path,
                    param: &param,
                    namespace,
                    innermost,
                    outer,
                },
            )
            .map_err(from_diagnostic)?;
        let mut statements = self.body(&innermost.body, &param, &own_path, &references)?;
        // Wrap the body in one function per segment, innermost first.
        for index in (0..chain.len()).rev() {
            let name = &chain[index].name;
            let argument = if index == 0 {
                match scope {
                    NamespaceScope::Namespace { parent } if namespace.exported => {
                        member_argument(parent, name)
                    }
                    _ => plain_argument(name),
                }
            } else {
                member_argument(&chain[index - 1].name, name)
            };
            let call = bluejs::Stmt::Expr(iife(function_over(name, statements), argument));
            if index == 0 {
                statements = Vec::new();
                if first {
                    let keyword = match scope {
                        NamespaceScope::Module => bluejs::DeclKind::Var,
                        NamespaceScope::Namespace { .. } => bluejs::DeclKind::Let,
                    };
                    statements.push(bluejs::Stmt::VarDecl(
                        keyword,
                        vec![bluejs::VarDeclarator {
                            pattern: bluejs::Pattern::Identifier(name.clone()),
                            init: None,
                        }],
                    ));
                }
                statements.push(call);
            } else {
                statements = vec![
                    bluejs::Stmt::VarDecl(
                        bluejs::DeclKind::Var,
                        vec![bluejs::VarDeclarator {
                            pattern: bluejs::Pattern::Identifier(name.clone()),
                            init: None,
                        }],
                    ),
                    call,
                ];
            }
        }
        Ok(statements)
    }

    fn body(
        &self,
        body: &[Declaration],
        param: &str,
        own_path: &str,
        references: &BTreeMap<String, String>,
    ) -> Result<Vec<bluejs::Stmt>, BridgeError> {
        let module = self.module;
        let evaluations = evaluate_enums_in(body);
        let mut evaluations = evaluations.into_iter();
        let mut statements = Vec::new();
        let mut declared_here: BTreeSet<String> = BTreeSet::new();
        let export = |name: &str| {
            bluejs::Stmt::Expr(assign_expr(
                property_expr(param, name),
                identifier_expr(name),
            ))
        };
        for declaration in body {
            let rewritten =
                rewrite_declaration(declaration, references).map_err(from_diagnostic)?;
            match &rewritten {
                Declaration::Variable(variable) if !variable.declared => {
                    if variable.exported {
                        if !variable.initializer.is_empty() {
                            let value = ExpressionLowerer::new(&module.id, &variable.initializer)
                                .parse()?;
                            statements.push(bluejs::Stmt::Expr(assign_expr(
                                property_expr(param, &variable.name),
                                value,
                            )));
                        }
                    } else {
                        statements.push(lower_variable(module, variable)?);
                    }
                }
                Declaration::Function(function) if !function.declared && !function.overload => {
                    statements.push(lower_function(module, function)?);
                    declared_here.insert(function.name.clone());
                    if function.exported {
                        statements.push(export(&function.name));
                    }
                }
                Declaration::Class(class) => {
                    statements.push(lower_class(module, class, self.define_class_fields)?);
                    declared_here.insert(class.name.clone());
                    if class.exported {
                        statements.push(export(&class.name));
                    }
                }
                Declaration::Enum(declaration) => {
                    if let Some(function) =
                        lower_enum_function(module, declaration, &mut evaluations)?
                    {
                        let name = &declaration.name;
                        if declared_here.insert(name.clone()) {
                            statements.push(bluejs::Stmt::VarDecl(
                                bluejs::DeclKind::Let,
                                vec![bluejs::VarDeclarator {
                                    pattern: bluejs::Pattern::Identifier(name.clone()),
                                    init: None,
                                }],
                            ));
                        }
                        let argument = if declaration.exported {
                            member_argument(param, name)
                        } else {
                            plain_argument(name)
                        };
                        statements.push(bluejs::Stmt::Expr(iife(function, argument)));
                    } else {
                        // An ambient enum has no run-time form.
                    }
                }
                Declaration::Namespace(inner) => {
                    statements.extend(self.namespace(
                        inner,
                        &NamespaceScope::Namespace {
                            parent: param.to_string(),
                        },
                        &mut declared_here,
                        own_path,
                        references,
                    )?);
                }
                Declaration::Raw(raw) => {
                    statements.push(bluejs::Stmt::Expr(
                        ExpressionLowerer::new(&module.id, &raw.tokens).parse()?,
                    ));
                }
                Declaration::TypeAlias(_) | Declaration::Interface(_) => {}
                Declaration::Variable(_) | Declaration::Function(_) => {}
                other => {
                    return Err(unsupported(
                        other.span().clone(),
                        "this declaration cannot appear in a namespace body lowered directly",
                    ));
                }
            }
        }
        Ok(statements)
    }
}
