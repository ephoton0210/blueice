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
    let parameter_properties = if is_static {
        Vec::new()
    } else {
        class.parameter_property_fields()
    };
    parameter_properties
        .iter()
        .chain(
            class
                .members
                .iter()
                .filter_map(|member| member.field.as_ref())
                .filter(|field| field.is_static == is_static),
        )
        .map(|field| TypeField {
            method: false,
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
        let parameter_properties = class.parameter_property_fields();
        let fields: Vec<&ClassField> = parameter_properties
            .iter()
            .chain(
                class
                    .members
                    .iter()
                    .filter_map(|member| member.field.as_ref()),
            )
            .collect();
        // A field an interface of the same name declares again must keep its type.
        for merged in &class.merged_interface_fields {
            let declared = fields
                .iter()
                .find(|field| !field.is_static && field.name == merged.name)
                .and_then(|field| class_field_type(field));
            if let Some(declared) = declared {
                if declared != merged.value
                    && declared != Type::Unknown
                    && merged.value != Type::Unknown
                {
                    self.typescript_type_error(
                        &merged.span,
                        format!(
                            "subsequent declarations of property `{}` must have the same type",
                            merged.name
                        ),
                        DiagnosticCode::TypeMismatch,
                        2717,
                        vec![
                            merged.name.clone(),
                            crate::diagnostic::type_text::render_in(&merged.value, self.project),
                            crate::diagnostic::type_text::render_in(&declared, self.project),
                        ],
                    );
                }
            }
        }
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
                self.typescript_type_error(
                    &field.name_span,
                    "static property `prototype` conflicts with the built-in property of the constructor function".to_string(),
                    DiagnosticCode::DuplicateDeclaration,
                    2699,
                    vec!["prototype".into(), class.name.clone()],
                );
            }
            if let (Some(annotation), Some(span)) = (&field.annotation, &field.annotation_span) {
                self.check_type(annotation, span);
            }
            if field.annotation.is_none() && class_field_type(field).is_none() {
                if field.initializer.is_none() && !field.from_default {
                    if !self.checking.no_implicit_any {
                        continue;
                    }
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
                             or default is a number, string or boolean literal",
                            field.name
                        ),
                    ));
                }
            }
            if let Some(initializer) = &field.initializer {
                self.check_field_initializer_order(
                    &class.name,
                    &fields,
                    position,
                    field,
                    initializer,
                );
                let mut scope = self.values.clone();
                scope.insert(
                    "this".to_string(),
                    if field.is_static {
                        constructor_side.clone()
                    } else {
                        class_body_this_type(class)
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
                // A field initializer runs in no function: `await` is not valid in it.
                let previous_async = self.async_context.replace(false);
                self.check_variable_in_scope(&variable, &scope);
                self.async_context = previous_async;
            }
        }
        self.check_definite_assignment(class, &fields);
        self.check_class_field_overrides(class, &fields);
        self.check_redeclared_field_overwrite(class, &fields);
        self.check_static_block_order(class);
    }

    /// With `useDefineForClassFields`, a derived class that declares an
    /// instance property its base already declares, with no initializer and no
    /// constructor assignment, redefines it as `undefined` and so overwrites
    /// the base's value. TypeScript reports that (TS2612); when fields are
    /// assigned, the declaration emits nothing and is harmless.
    fn check_redeclared_field_overwrite(
        &mut self,
        class: &ClassDeclaration,
        fields: &[&ClassField],
    ) {
        if !self.define_class_fields || class.extends_name.is_none() {
            return;
        }
        for field in fields {
            if field.is_static
                || field.declared
                || field.initializer.is_some()
                || field.from_default
                || field.name.starts_with('#')
                || class
                    .parameter_property_fields()
                    .iter()
                    .any(|property| property.name == field.name)
            {
                continue;
            }
            if !matches!(
                self.base_member_kind(class, &field.name, false),
                Ok(Some(super::accessors::BaseMemberKind::Field))
            ) {
                continue;
            }
            let assigned = class
                .members
                .iter()
                .filter_map(|member| member.constructor.as_ref())
                .filter_map(|constructor| constructor.body.as_deref())
                .any(|body| {
                    body.iter().any(|item| {
                        item_assigns_this_field(item, &field.name)
                            || nested_item_assigns_this_field(item, &field.name)
                    })
                });
            if !assigned {
                self.type_error(
                    &field.name_span,
                    format!(
                        "property `{}` will overwrite the base property; add an initializer or \
                         remove the redundant declaration",
                        field.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    /// A static block runs at its place among the static fields, so it cannot
    /// read a static field declared after it.
    fn check_static_block_order(&mut self, class: &ClassDeclaration) {
        for (block_index, member) in class.members.iter().enumerate() {
            let Some(block) = &member.static_block else {
                continue;
            };
            let later: BTreeSet<&str> = class
                .members
                .iter()
                .skip(block_index + 1)
                .filter_map(|other| other.field.as_ref())
                .filter(|field| field.is_static)
                .map(|field| field.name.as_str())
                .collect();
            if later.is_empty() {
                continue;
            }
            let mut flat = Vec::new();
            super::super_calls::statements(&block.body, &mut flat);
            for (tokens, span) in flat {
                let defers = tokens
                    .iter()
                    .any(|token| token.is("=>") || token.is("function"));
                for (index, token) in tokens.iter().enumerate() {
                    if !(token.is("this") || token.text == class.name)
                        || !tokens.get(index + 1).is_some_and(|dot| dot.is("."))
                    {
                        continue;
                    }
                    let Some(name) = tokens
                        .get(index + 2)
                        .filter(|name| later.contains(name.text.as_str()))
                    else {
                        continue;
                    };
                    if defers {
                        self.diagnostics.push(Diagnostic::error(
                            DiagnosticCode::UnsupportedSyntax,
                            span.clone(),
                            "a static block that refers to a later static field inside a nested \
                             function is not supported yet",
                        ));
                    } else {
                        self.type_error(
                            &SourceSpan::new(&span.module, name.start, name.end),
                            format!("property `{}` is used before its initialization", name.text),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                }
            }
        }
    }

    /// A field may not share a name and placement with another field or a
    /// method.
    fn check_class_member_names(&mut self, class: &ClassDeclaration) {
        let mut seen: BTreeSet<(bool, String)> = BTreeSet::new();
        let mut method_groups: BTreeSet<(bool, String)> = BTreeSet::new();
        for group in &class.method_groups {
            method_groups.insert((group.is_static, group.name.clone()));
        }
        let parameter_properties = class.parameter_property_fields();
        for field in parameter_properties.iter().chain(
            class
                .members
                .iter()
                .filter_map(|member| member.field.as_ref()),
        ) {
            let key = (field.is_static, field.name.clone());
            if !seen.insert(key.clone()) || method_groups.contains(&key) {
                self.duplicate(&field.name, field.name_span.clone());
            }
        }
    }

    /// `this.later` (or `Class.later` in a static initializer) in an
    /// initializer that runs before `later` is defined.
    fn check_field_initializer_order(
        &mut self,
        class_name: &str,
        fields: &[&ClassField],
        position: usize,
        field: &ClassField,
        initializer: &[Token],
    ) {
        let defers = initializer
            .iter()
            .any(|token| token.is("=>") || token.is("function"));
        for (index, token) in initializer.iter().enumerate() {
            let names_the_class = token.is("this") || (field.is_static && token.text == class_name);
            if !names_the_class || !initializer.get(index + 1).is_some_and(|dot| dot.is(".")) {
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
        if !self.checking.strict_property_initialization {
            return;
        }
        let constructor_bodies: Vec<&[FunctionBodyItem]> = class
            .members
            .iter()
            .filter_map(|member| member.constructor.as_ref())
            .filter_map(|constructor| constructor.body.as_deref())
            .collect();
        for field in fields {
            if field.declared {
                continue;
            }
            if class.members.iter().any(|member| {
                member.abstract_modifier.is_some()
                    && member
                        .field
                        .as_ref()
                        .is_some_and(|item| item.name == field.name)
            }) {
                continue;
            }
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
            if field.name.starts_with('#') {
                continue;
            }
            let Some(own_type) = class_field_type(field) else {
                continue;
            };
            let inherited = self.inherited_class_member(class, &field.name, field.is_static);
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
                self.field_method_error(class, field);
                continue;
            }
            let declared = if field.optional {
                Type::Union(vec![own_type, Type::Undefined])
            } else {
                own_type
            };
            if !self.is_assignable_bounded(&declared, &inherited, &field.span) {
                let (code, arguments, span) = if field.is_static {
                    (
                        2417,
                        vec![
                            format!("typeof {}", class.name),
                            format!("typeof {base_name}"),
                        ],
                        &class.name_span,
                    )
                } else {
                    (
                        2416,
                        vec![
                            field.name.clone(),
                            class.name.clone(),
                            type_label(&Type::Named {
                                name: base_name.into(),
                                arguments: class.extends_arguments.clone(),
                            }),
                        ],
                        &field.name_span,
                    )
                };
                self.typescript_type_error(
                    span,
                    format!(
                        "property `{}` of type `{}` is not assignable to the base property type `{}`",
                        field.name,
                        type_label(&declared),
                        type_label(&inherited)
                    ),
                    DiagnosticCode::TypeMismatch,
                    code,
                    arguments,
                );
            }
        }
    }

    pub(super) fn field_method_error(&mut self, class: &ClassDeclaration, field: &ClassField) {
        let message = format!(
            "class defines `{}` as a property but the base class defines it as a method",
            field.name,
        );
        let base_name = class.extends_name.as_deref().unwrap_or_default();
        let incompatible = class_field_type(field)
            .zip(self.inherited_class_member(class, &field.name, field.is_static))
            .is_some_and(|(own, inherited)| {
                !self.is_assignable_bounded(&own, &inherited, &field.span)
            });
        if incompatible {
            let (code, arguments) = if field.is_static {
                (
                    2417,
                    vec![
                        format!("typeof {}", class.name),
                        format!("typeof {base_name}"),
                    ],
                )
            } else {
                (
                    2416,
                    vec![field.name.clone(), class.name.clone(), base_name.into()],
                )
            };
            self.typescript_type_error(
                &field.name_span,
                message,
                DiagnosticCode::TypeMismatch,
                code,
                arguments,
            );
        } else {
            self.blue_only_type_error(
                &field.name_span, message, DiagnosticCode::TypeMismatch,
                "BlueTSC refuses a field in place of a base method even where TypeScript permits a compatible function property.",
            );
        }
    }

    /// The type of `name` on the named base class, found through its own or
    /// inherited members.
    pub(super) fn inherited_member(
        &self,
        base_name: &str,
        name: &str,
        is_static: bool,
    ) -> Option<Type> {
        let surface = if is_static {
            self.values.get(base_name)
        } else {
            self.types
                .get(base_name)
                .filter(|definition| definition.kind == TypeDefinitionKind::Class)
                .map(|definition| &definition.value)
        };
        let Some(Type::Record(fields) | Type::CallableRecord { fields, .. }) =
            surface.map(Type::object_type)
        else {
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

    fn inherited_class_member(
        &self,
        class: &ClassDeclaration,
        name: &str,
        is_static: bool,
    ) -> Option<Type> {
        let surface = self.specialized_base_surface(class, is_static)?;
        let (Type::Record(fields) | Type::CallableRecord { fields, .. }) = surface.object_type()
        else {
            return None;
        };
        fields
            .iter()
            .find(|field| field.name == name)
            .or_else(|| {
                fields.iter().find(|field| {
                    super::visibility::parse_restricted_name(&field.name)
                        .is_some_and(|(_, _, member)| member == name)
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
