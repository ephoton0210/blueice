// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Erased computed fields capture their runtime key in a lexical binding.

use super::*;

pub(super) fn temporary(member: &blueice_bluets::ClassMemberShell) -> Option<String> {
    (member.field.is_some()
        && member.abstract_modifier.is_none()
        && !member.field.as_ref().is_some_and(|field| field.declared)
        && member.key.first().is_some_and(|key| key.is("[")))
    .then(|| format!("\0btsKey{}", member.span.start))
}

fn classes<'a>(declarations: &'a [Declaration], result: &mut Vec<&'a ClassDeclaration>) {
    for declaration in declarations {
        match declaration {
            Declaration::Class(class) => result.push(class),
            Declaration::Namespace(namespace) => classes(&namespace.body, result),
            _ => {}
        }
    }
}

fn bindings(
    module: &Module,
    start: usize,
    end: usize,
    kind: bluejs::DeclKind,
) -> Option<(bluejs::Stmt, SourceSpan)> {
    if super::super::jsx_direct::defines_class_fields() {
        return None;
    }
    let mut declarations = Vec::new();
    classes(&module.declarations, &mut declarations);
    declarations.extend(
        module
            .class_expressions()
            .map(|expression| &expression.class),
    );
    let mut bindings = Vec::new();
    let mut span = None;
    for class in declarations {
        if class.span.start < start || class.span.end > end {
            continue;
        }
        for member in &class.members {
            if let Some(name) = temporary(member) {
                span.get_or_insert_with(|| member.span.clone());
                bindings.push(bluejs::VarDeclarator {
                    pattern: bluejs::Pattern::Identifier(name),
                    init: None,
                });
            }
        }
    }
    span.map(|span| (bluejs::Stmt::VarDecl(kind, bindings), span))
}

pub(super) fn root(
    module: &Module,
    body: &mut Vec<bluejs::Stmt>,
    provenance: &mut Vec<(SourceSpan, LoweringProvenanceKind)>,
) {
    if let Some((statement, span)) = bindings(module, 0, module.source.len(), bluejs::DeclKind::Let)
    {
        let at = body
            .iter()
            .take_while(|statement| {
                matches!(statement, bluejs::Stmt::Expr(bluejs::Expr::String(_)))
            })
            .count();
        body.insert(at, statement);
        provenance.insert(at, (span, LoweringProvenanceKind::LoweredSyntax));
    }
}

fn item_span(item: &FunctionBodyItem) -> &SourceSpan {
    match item {
        FunctionBodyItem::Variable(variable) => &variable.span,
        FunctionBodyItem::Expression { span, .. }
        | FunctionBodyItem::Return { span, .. }
        | FunctionBodyItem::Throw { span, .. }
        | FunctionBodyItem::Opaque(span) => span,
        FunctionBodyItem::If(statement) => &statement.span,
        FunctionBodyItem::While(statement) => &statement.span,
        FunctionBodyItem::Try(statement) => &statement.span,
        FunctionBodyItem::Function(function) => &function.span,
    }
}

pub(super) fn body(module: &Module, items: &[FunctionBodyItem], body: &mut Vec<bluejs::Stmt>) {
    if let Some((first, last)) = items.first().zip(items.last()) {
        if let Some((statement, _)) = bindings(
            module,
            item_span(first).start,
            item_span(last).end,
            bluejs::DeclKind::Var,
        ) {
            // A tail var declaration is hoisted and preserves directive prologues.
            body.push(statement);
        }
    }
}

pub(super) fn assign_key(name: String, value: bluejs::Expr) -> bluejs::Expr {
    bluejs::Expr::Assign {
        op: bluejs::AssignOp::Assign,
        target: Box::new(bluejs::Expr::Identifier(name)),
        value: Box::new(value),
    }
}

pub(super) fn ordered_key(
    key: bluejs::PropertyKey,
    pending: &mut Vec<bluejs::Expr>,
) -> bluejs::PropertyKey {
    if let bluejs::PropertyKey::Computed(expression) = key {
        if !pending.is_empty() {
            let mut expressions = std::mem::take(pending);
            expressions.push(*expression);
            return bluejs::PropertyKey::Computed(Box::new(bluejs::Expr::Sequence(expressions)));
        }
        bluejs::PropertyKey::Computed(expression)
    } else {
        key
    }
}

pub(super) fn assign_this_key(key: bluejs::PropertyKey, value: bluejs::Expr) -> bluejs::Stmt {
    // Assignment emit still performs the field's NamedEvaluation. An object
    // data property supplies that name without introducing a class self binding
    // or a new function scope. Computed field keys are already captured above.
    fn anonymous_class(value: &bluejs::Expr) -> bool {
        match value {
            bluejs::Expr::Class(class) => class.name.is_none(),
            bluejs::Expr::Parenthesized(value) => anonymous_class(value),
            _ => false,
        }
    }
    let value = if anonymous_class(&value) {
        let object = bluejs::Expr::Object(vec![bluejs::ObjectProp::KeyValue {
            key: key.clone(),
            value,
            // This synthetic property always defines data, including __proto__.
            shorthand: true,
        }]);
        let (property, computed) = match &key {
            bluejs::PropertyKey::Identifier(name) => {
                (bluejs::Expr::Identifier(name.clone()), false)
            }
            bluejs::PropertyKey::String(value) => (bluejs::Expr::String(value.clone()), true),
            bluejs::PropertyKey::Number(value) => (bluejs::Expr::Number(*value), true),
            bluejs::PropertyKey::Computed(value) => (*value.clone(), true),
        };
        bluejs::Expr::Member {
            object: Box::new(object),
            property: Box::new(property),
            computed,
        }
    } else {
        value
    };
    let (property, computed) = match key {
        bluejs::PropertyKey::Identifier(name) => (bluejs::Expr::Identifier(name), false),
        bluejs::PropertyKey::String(value) => (bluejs::Expr::String(value), true),
        bluejs::PropertyKey::Number(value) => (bluejs::Expr::Number(value), true),
        bluejs::PropertyKey::Computed(value) => (*value, true),
    };
    bluejs::Stmt::Expr(bluejs::Expr::Assign {
        op: bluejs::AssignOp::Assign,
        target: Box::new(bluejs::Expr::Member {
            object: Box::new(bluejs::Expr::This),
            property: Box::new(property),
            computed,
        }),
        value: Box::new(value),
    })
}
