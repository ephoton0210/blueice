// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static decorator arguments and replacements share retained class syntax.
use crate::parser::{ClassDeclaration, ClassMemberShell, Parameter, Type, TypeField};
use crate::SourceSpan;

fn named(name: &str, arguments: Vec<Type>) -> Type {
    Type::Named {
        name: name.into(),
        arguments,
    }
}

pub(crate) fn constructor_type(class: &ClassDeclaration) -> Type {
    named(&format!("typeof {}", class.name), Vec::new())
}

fn parameter(name: &str, value: Type, span: &SourceSpan) -> Parameter {
    Parameter {
        decorators: Vec::new(),
        name: name.into(),
        pattern: None,
        rest: false,
        optional: false,
        annotation: Some(value),
        default: None,
        span: span.clone(),
    }
}

fn callable(parameters: Vec<Parameter>, result: Type) -> Type {
    Type::Function {
        parameters,
        result: Box::new(result),
    }
}

pub(crate) fn replacement_type(
    class: &ClassDeclaration,
    member: Option<&ClassMemberShell>,
    legacy: bool,
) -> Option<Type> {
    let Some(member) = member else {
        return Some(constructor_type(class));
    };
    let value = if let Some(method) = &member.method {
        callable(
            method.parameters.clone(),
            method.return_type.clone().unwrap_or(Type::Any),
        )
    } else if let Some(accessor) = &member.accessor {
        let value = if accessor.getter {
            accessor.return_type.clone().unwrap_or(Type::Any)
        } else {
            accessor
                .parameters
                .first()
                .and_then(|p| p.annotation.clone())
                .unwrap_or(Type::Any)
        };
        if legacy {
            value
        } else if accessor.getter {
            callable(Vec::new(), value)
        } else {
            callable(vec![parameter("value", value, &member.span)], Type::Void)
        }
    } else if let Some(field) = &member.field {
        let value = field.declared_type().unwrap_or(Type::Any);
        if legacy {
            return None;
        }
        if field.accessor {
            return Some(named(
                "ClassAccessorDecoratorResult",
                vec![class.this_type(), value],
            ));
        }
        return Some(callable(
            vec![
                parameter("this", class.this_type(), &member.span),
                parameter("value", value.clone(), &member.span),
            ],
            value,
        ));
    } else {
        return None;
    };
    Some(if legacy {
        named("TypedPropertyDescriptor", vec![value])
    } else {
        value
    })
}

pub(crate) fn context_type(
    class: &ClassDeclaration,
    member: Option<&ClassMemberShell>,
) -> Option<Type> {
    let Some(member) = member else {
        return Some(named(
            "ClassDecoratorContext",
            vec![constructor_type(class)],
        ));
    };
    let (kind, is_static, value) = if let Some(method) = &member.method {
        (
            "Method",
            method.is_static,
            callable(
                method.parameters.clone(),
                method.return_type.clone().unwrap_or(Type::Any),
            ),
        )
    } else if let Some(accessor) = &member.accessor {
        (
            if accessor.getter { "Getter" } else { "Setter" },
            accessor.is_static,
            if accessor.getter {
                accessor.return_type.clone().unwrap_or(Type::Any)
            } else {
                accessor
                    .parameters
                    .first()
                    .and_then(|p| p.annotation.clone())
                    .unwrap_or(Type::Any)
            },
        )
    } else if let Some(field) = &member.field {
        (
            if field.accessor { "Accessor" } else { "Field" },
            field.is_static,
            field.declared_type().unwrap_or(Type::Any),
        )
    } else {
        return None;
    };
    let receiver = if is_static {
        constructor_type(class)
    } else {
        class.this_type()
    };
    let provider = named(
        &format!("Class{kind}DecoratorContext"),
        vec![receiver, value],
    );
    let name = member.name.as_deref().unwrap_or("");
    let fields = [
        ("name", serde_json::to_string(name).unwrap()),
        ("private", name.starts_with('#').to_string()),
        ("static", is_static.to_string()),
    ]
    .into_iter()
    .map(|(name, value)| TypeField {
        accessor_write_type: None,
        method: false,
        name: name.into(),
        readonly: false,
        optional: false,
        value: Type::Literal(value),
        span: member.span.clone(),
    })
    .collect();
    Some(Type::Intersection(vec![provider, Type::Record(fields)]))
}
