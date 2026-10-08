// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class members, decorators and synthesized constructor prologues.

use super::*;

/// A class as a BlueJS class declaration. Overload signatures have no runtime
/// form and are skipped; the constructor is the non-static method named
/// `constructor`, as BlueJS represents it.
///
/// With `define_class_fields` the fields are BlueJS class fields, which have
/// define semantics. Without it they are lowered as TypeScript does for
/// `useDefineForClassFields: false`: an instance field becomes `this.x = init`
/// at the start of the constructor (after `super(...)`), a static field a
/// static block that assigns it, and a field with no initializer disappears.
/// The expressions of a list of decorators, in source order. BlueJS evaluates
/// and applies them with the standard decorator semantics itself, so the direct
/// path needs no helper.
pub(super) fn lower_decorators(
    module: &Module,
    decorators: &[blueice_bluets::Decorator],
) -> Result<Vec<bluejs::Expr>, BridgeError> {
    if let (true, Some(first)) = (
        super::jsx_direct::experimental_decorators(),
        decorators.first(),
    ) {
        return Err(unsupported(
            first.span.clone(),
            "BlueJS implements the standard decorators; `experimentalDecorators` programs run as `bluetsc build --experimental-decorators` output",
        ));
    }
    decorators
        .iter()
        .map(|decorator| ExpressionLowerer::new(&module.id, &decorator.tokens).parse())
        .collect()
}

pub(super) fn lower_class(
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
        if member.abstract_modifier.is_some() {
            continue;
        }
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
                    false,
                    false,
                )?,
                getter: accessor.getter,
                is_static: accessor.is_static,
                decorators: lower_decorators(module, &member.decorators)?,
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
                    accessor: field.accessor,
                    decorators: lower_decorators(module, &member.decorators)?,
                });
            } else if !member.decorators.is_empty() || field.accessor {
                return Err(unsupported(
                    member.span.clone(),
                    "decorators and auto-accessors need class fields to be defined (the default for ES2022)",
                ));
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
        let mut function =
            lower_function_value(module, Some(name.clone()), parameters, body, false, false)?;
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
            decorators: lower_decorators(module, &member.decorators)?,
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
        decorators: lower_decorators(module, &class.decorators)?,
        // Synthesized from BlueTSC's own lowered AST: no `[[SourceText]]`.
        source_text: Default::default(),
    }))
}

/// `this.name = value;`
pub(super) fn assign_this_member(name: &str, value: bluejs::Expr) -> bluejs::Stmt {
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
pub(super) fn synthesized_constructor(
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
pub(super) fn insert_constructor_prologue(
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
