// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public class fields: their types, initializers, definite assignment and
//! overrides.

use super::*;
use crate::parser::ClassField;

pub(in crate::checker) fn class_field_type(field: &ClassField) -> Option<Type> {
    field.declared_type()
}

/// The instance or static fields of a class as record members.
pub(super) fn class_field_type_fields(class: &ClassDeclaration, is_static: bool) -> Vec<TypeField> {
    class
        .members
        .iter()
        .filter_map(|member| member.field.as_ref())
        .filter(|field| field.is_static == is_static)
        .map(|field| TypeField {
            name: visibility::member_field_name(class, field.visibility, &field.name),
            readonly: field.readonly,
            optional: field.optional,
            value: class_field_type(field).unwrap_or(Type::Unknown),
            span: field.span.clone(),
        })
        .collect()
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn validate_class_fields(&mut self, class: &ClassDeclaration) {
        let fields: Vec<&ClassField> = class
            .members
            .iter()
            .filter_map(|member| member.field.as_ref())
            .collect();
        if fields.is_empty() {
            return;
        }
        self.check_class_member_names(class);
        let constructor_side = self
            .values
            .get(&class.name)
            .cloned()
            .unwrap_or_else(|| class_constructor_side_type(class));
        for (position, field) in fields.iter().enumerate() {
            if field.is_static && field.name == "prototype" {
                self.type_error(
                    &field.name_span,
                    "static property `prototype` conflicts with the built-in property of the constructor function".to_string(),
                    DiagnosticCode::DuplicateDeclaration,
                );
            }
            if let (Some(annotation), Some(span)) = (&field.annotation, &field.annotation_span) {
                self.check_type(annotation, span);
            }
            if field.annotation.is_none() && class_field_type(field).is_none() {
                if field.initializer.is_none() {
                    self.type_error(
                        &field.name_span,
                        format!("class field `{}` implicitly has an `any` type", field.name),
                        DiagnosticCode::TypeMismatch,
                    );
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        field.span.clone(),
                        format!(
                            "class field `{}` needs a type annotation unless its initializer \
                             is a number, string or boolean literal",
                            field.name
                        ),
                    ));
                }
            }
            if let Some(initializer) = &field.initializer {
                self.check_field_initializer_order(&fields, position, field, initializer);
                let mut scope = self.values.clone();
                scope.insert(
                    "this".to_string(),
                    if field.is_static {
                        constructor_side.clone()
                    } else {
                        Type::Named {
                            name: class.name.clone(),
                            arguments: Vec::new(),
                        }
                    },
                );
                let variable = crate::parser::VariableDeclaration {
                    name: field.name.clone(),
                    kind: crate::parser::VariableKind::Const,
                    annotation: field.annotation.clone(),
                    initializer: initializer.clone(),
                    exported: false,
                    declared: false,
                    span: field.span.clone(),
                };
                self.check_variable_in_scope(&variable, &scope);
            }
        }
        self.check_definite_assignment(class, &fields);
        self.check_class_field_overrides(class, &fields);
    }

    /// A field may not share a name and placement with another field or a
    /// method.
    fn check_class_member_names(&mut self, class: &ClassDeclaration) {
        let mut seen: BTreeSet<(bool, String)> = BTreeSet::new();
        let mut method_groups: BTreeSet<(bool, String)> = BTreeSet::new();
        for group in &class.method_groups {
            method_groups.insert((group.is_static, group.name.clone()));
        }
        for member in &class.members {
            let Some(field) = &member.field else {
                continue;
            };
            let key = (field.is_static, field.name.clone());
            if !seen.insert(key.clone()) || method_groups.contains(&key) {
                self.duplicate(&field.name, field.name_span.clone());
            }
        }
    }

    /// `this.later` in an initializer that runs before `later` is defined.
    fn check_field_initializer_order(
        &mut self,
        fields: &[&ClassField],
        position: usize,
        field: &ClassField,
        initializer: &[Token],
    ) {
        let defers = initializer
            .iter()
            .any(|token| token.is("=>") || token.is("function"));
        for (index, token) in initializer.iter().enumerate() {
            if !token.is("this") || !initializer.get(index + 1).is_some_and(|dot| dot.is(".")) {
                continue;
            }
            let Some(name) = initializer.get(index + 2) else {
                continue;
            };
            let forward = fields
                .iter()
                .skip(position)
                .any(|other| other.is_static == field.is_static && other.name == name.text);
            if !forward {
                continue;
            }
            if defers {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    field.span.clone(),
                    "a field initializer that refers to a later field inside a nested \
                     function is not supported yet",
                ));
            } else {
                self.type_error(
                    &SourceSpan::new(&field.span.module, name.start, name.end),
                    format!("property `{}` is used before its initialization", name.text),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    /// `strictPropertyInitialization`: a non-optional instance field with no
    /// initializer, no `!` and no `undefined`-admitting type must be assigned
    /// at the top level of the constructor. An assignment only inside a
    /// branch is not modeled and is refused as unsupported syntax.
    fn check_definite_assignment(&mut self, class: &ClassDeclaration, fields: &[&ClassField]) {
        let constructor_bodies: Vec<&[FunctionBodyItem]> = class
            .members
            .iter()
            .filter_map(|member| member.constructor.as_ref())
            .filter_map(|constructor| constructor.body.as_deref())
            .collect();
        for field in fields {
            let needs_assignment = !field.is_static
                && !field.optional
                && !field.definite
                && field.initializer.is_none()
                && field
                    .annotation
                    .as_ref()
                    .is_some_and(|annotation| !self.type_admits_undefined(annotation, &field.span));
            if !needs_assignment {
                continue;
            }
            let mut top_level = false;
            let mut nested = false;
            for body in &constructor_bodies {
                top_level |= body
                    .iter()
                    .any(|item| item_assigns_this_field(item, &field.name));
                nested |= body
                    .iter()
                    .any(|item| nested_item_assigns_this_field(item, &field.name));
            }
            if top_level {
                continue;
            }
            if nested {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    field.span.clone(),
                    format!(
                        "definite assignment of `{}` through a branch is not supported yet",
                        field.name
                    ),
                ));
            } else {
                self.type_error(
                    &field.name_span,
                    format!(
                        "property `{}` has no initializer and is not definitely assigned in the constructor",
                        field.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    fn type_admits_undefined(&mut self, value: &Type, span: &SourceSpan) -> bool {
        matches!(
            value,
            Type::Any | Type::Unknown | Type::Undefined | Type::Void
        ) || self.is_assignable_bounded(&Type::Undefined, value, span)
    }

    /// An own field must be assignable to the base class's same-named
    /// property, and may not replace a method. A base outside this module has
    /// only its type surface, so a same-named member there is refused.
    fn check_class_field_overrides(&mut self, class: &ClassDeclaration, fields: &[&ClassField]) {
        let Some(base_name) = class.extends_name.as_deref() else {
            return;
        };
        let local_base =
            self.module
                .declarations
                .iter()
                .find_map(|declaration| match declaration {
                    Declaration::Class(base) if base.name == base_name => Some(base),
                    _ => None,
                });
        for field in fields {
            let Some(own_type) = class_field_type(field) else {
                continue;
            };
            let inherited = self.inherited_member(base_name, &field.name, field.is_static);
            let Some(inherited) = inherited else {
                continue;
            };
            let Some(base) = local_base else {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    field.span.clone(),
                    format!(
                        "field `{}` redeclares a member of an imported base class, which is not supported yet",
                        field.name
                    ),
                ));
                continue;
            };
            if base_declares_method(base, &field.name, field.is_static) {
                self.type_error(
                    &field.name_span,
                    format!(
                        "class defines `{}` as a property but the base class defines it as a method",
                        field.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
                continue;
            }
            let declared = if field.optional {
                Type::Union(vec![own_type, Type::Undefined])
            } else {
                own_type
            };
            if !self.is_assignable_bounded(&declared, &inherited, &field.span) {
                self.type_error(
                    &field.name_span,
                    format!(
                        "property `{}` of type `{}` is not assignable to the base property type `{}`",
                        field.name,
                        type_label(&declared),
                        type_label(&inherited)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    /// The type of `name` on the named base class, found through its own or
    /// inherited members.
    fn inherited_member(&self, base_name: &str, name: &str, is_static: bool) -> Option<Type> {
        let surface = if is_static {
            self.values.get(base_name)
        } else {
            self.types
                .get(base_name)
                .filter(|definition| definition.kind == TypeDefinitionKind::Class)
                .map(|definition| &definition.value)
        };
        let Some(Type::Record(fields)) = surface else {
            return None;
        };
        fields
            .iter()
            .find(|field| field.name == name)
            .or_else(|| {
                // A protected member exists only under its marker name.
                fields.iter().find(|field| {
                    visibility::parse_restricted_name(&field.name).is_some_and(
                        |(visibility, _, member)| {
                            visibility == crate::parser::Visibility::Protected && member == name
                        },
                    )
                })
            })
            .map(|field| field.value.clone())
    }
}

fn base_declares_method(base: &ClassDeclaration, name: &str, is_static: bool) -> bool {
    base.method_groups
        .iter()
        .any(|group| group.name == name && group.is_static == is_static)
}

/// `this.name = ...;` (plain assignment) as a whole statement.
fn item_assigns_this_field(item: &FunctionBodyItem, name: &str) -> bool {
    let FunctionBodyItem::Expression { tokens, .. } = item else {
        return false;
    };
    matches!(tokens.as_slice(), [this, dot, field, assign, ..]
        if this.is("this") && dot.is(".") && field.text == name && assign.is("="))
}

fn nested_item_assigns_this_field(item: &FunctionBodyItem, name: &str) -> bool {
    match item {
        FunctionBodyItem::If(statement) => {
            statement.consequent.iter().any(|item| {
                item_assigns_this_field(item, name) || nested_item_assigns_this_field(item, name)
            }) || match &statement.alternate {
                Some(crate::parser::FunctionElseBranch::Braced(items)) => {
                    items.iter().any(|item| {
                        item_assigns_this_field(item, name)
                            || nested_item_assigns_this_field(item, name)
                    })
                }
                Some(crate::parser::FunctionElseBranch::ElseIf(next)) => {
                    nested_item_assigns_this_field(&FunctionBodyItem::If((**next).clone()), name)
                }
                None => false,
            }
        }
        FunctionBodyItem::While(statement) => statement.body.iter().any(|item| {
            item_assigns_this_field(item, name) || nested_item_assigns_this_field(item, name)
        }),
        FunctionBodyItem::Try(statement) => statement
            .block
            .iter()
            .chain(statement.finalizer.iter().flatten())
            .chain(
                statement
                    .handler
                    .iter()
                    .flat_map(|handler| handler.body.iter()),
            )
            .any(|item| {
                item_assigns_this_field(item, name) || nested_item_assigns_this_field(item, name)
            }),
        _ => false,
    }
}
