// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `get` / `set` class accessors.
//!
//! An accessor pair is one property in the class's type record. It reads as
//! the getter's type and is read-only when there is no setter. A getter and a
//! setter with unrelated types (which TypeScript allows) would need a separate
//! write type, so that pair is refused as unsupported instead.

use super::visibility::member_field_name;
use super::*;
use crate::parser::ClassAccessor;

/// The accessors sharing one name and placement.
struct AccessorGroup<'a> {
    getter: Option<&'a ClassAccessor>,
    setter: Option<&'a ClassAccessor>,
}

fn accessor_groups(class: &ClassDeclaration, is_static: bool) -> Vec<(String, AccessorGroup<'_>)> {
    let mut groups: Vec<(String, AccessorGroup<'_>)> = Vec::new();
    for accessor in class
        .members
        .iter()
        .filter_map(|member| member.accessor.as_ref())
        .filter(|accessor| accessor.is_static == is_static)
    {
        let index = match groups.iter().position(|(name, _)| *name == accessor.name) {
            Some(index) => index,
            None => {
                groups.push((
                    accessor.name.clone(),
                    AccessorGroup {
                        getter: None,
                        setter: None,
                    },
                ));
                groups.len() - 1
            }
        };
        let slot = if accessor.getter {
            &mut groups[index].1.getter
        } else {
            &mut groups[index].1.setter
        };
        slot.get_or_insert(accessor);
    }
    groups
}

/// The type a setter's parameter carries.
fn setter_type(setter: &ClassAccessor) -> Option<Type> {
    setter
        .parameters
        .first()
        .and_then(|parameter| parameter.annotation.clone())
}

/// The value type of an accessor group: the getter's annotation, or, without
/// one, the setter's parameter annotation.
fn group_type(group: &AccessorGroup<'_>) -> Option<Type> {
    group
        .getter
        .and_then(|getter| getter.return_type.clone())
        .or_else(|| group.setter.and_then(setter_type))
}

/// Each accessor group of one placement as a record member.
pub(super) fn class_accessor_type_fields(
    class: &ClassDeclaration,
    is_static: bool,
) -> Vec<TypeField> {
    accessor_groups(class, is_static)
        .into_iter()
        .map(|(name, group)| {
            let first = group
                .getter
                .or(group.setter)
                .expect("a group has an accessor");
            TypeField {
                name: member_field_name(class, first.visibility, &name),
                readonly: group.setter.is_none(),
                optional: false,
                value: group_type(&group).unwrap_or(Type::Unknown),
                span: first.span.clone(),
            }
        })
        .collect()
}

/// How the nearest declaration of a member in a local base class is written.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BaseMemberKind {
    Field,
    Method,
    Accessor,
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn validate_class_accessors(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let mut placed: Vec<(bool, String, bool)> = Vec::new();
        for accessor in class
            .members
            .iter()
            .filter_map(|member| member.accessor.as_ref())
        {
            if accessor.getter && accessor.return_type.is_none() {
                let setter_annotated = class
                    .members
                    .iter()
                    .filter_map(|member| member.accessor.as_ref())
                    .any(|other| {
                        !other.getter
                            && other.name == accessor.name
                            && other.is_static == accessor.is_static
                            && setter_type(other).is_some()
                    });
                if !setter_annotated {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        accessor.name_span.clone(),
                        format!(
                            "getter `{}` needs a return type annotation; inferring it from the \\
                             body is not supported yet",
                            accessor.name
                        ),
                    ));
                }
            }
            if !accessor.getter
                && accessor
                    .parameters
                    .first()
                    .is_some_and(|parameter| parameter.annotation.is_none())
            {
                self.type_error(
                    &accessor.name_span,
                    format!(
                        "setter `{}` has an implicitly `any` parameter",
                        accessor.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if let Some(return_type) = &accessor.return_type {
                self.check_type(
                    return_type,
                    accessor.return_type_span.as_ref().unwrap_or(&accessor.span),
                );
            }
            let key = (accessor.is_static, accessor.name.clone(), accessor.getter);
            if placed.contains(&key) {
                self.duplicate(&accessor.name, accessor.name_span.clone());
            }
            placed.push(key);
        }
        for is_static in [false, true] {
            for (name, group) in accessor_groups(class, is_static) {
                if let (Some(getter), Some(setter)) = (group.getter, group.setter) {
                    if getter.visibility != setter.visibility {
                        self.type_error(
                            &setter.name_span,
                            format!("the getter and setter of `{name}` must have the same accessibility"),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                    if let (Some(read), Some(write)) =
                        (getter.return_type.as_ref(), setter_type(setter))
                    {
                        if *read != write {
                            self.diagnostics.push(Diagnostic::error(
                                DiagnosticCode::UnsupportedSyntax,
                                setter.name_span.clone(),
                                format!(
                                    "the getter and setter of `{name}` have different types, \\
                                     which is not supported yet"
                                ),
                            ));
                        }
                    }
                }
            }
        }
        // An accessor may not share a name and placement with a field, a
        // method or a parameter property.
        let mut others: BTreeSet<(bool, String)> = BTreeSet::new();
        for group in &class.method_groups {
            others.insert((group.is_static, group.name.clone()));
        }
        let parameter_properties = class.parameter_property_fields();
        for field in parameter_properties.iter().chain(
            class
                .members
                .iter()
                .filter_map(|member| member.field.as_ref()),
        ) {
            others.insert((field.is_static, field.name.clone()));
        }
        for accessor in class
            .members
            .iter()
            .filter_map(|member| member.accessor.as_ref())
        {
            if others.contains(&(accessor.is_static, accessor.name.clone())) {
                self.duplicate(&accessor.name, accessor.name_span.clone());
            }
        }
        self.validate_member_kind_conflicts(class);
    }

    /// The kind of the nearest declaration of `name` in the chain of local
    /// base classes. `Err(())` when the chain leaves the module (or a base is
    /// not local) before a declaration is found but the base's type record has
    /// the member, so its kind cannot be told.
    fn base_member_kind(
        &self,
        class: &ClassDeclaration,
        name: &str,
        is_static: bool,
    ) -> Result<Option<BaseMemberKind>, ()> {
        let mut current = class.extends_name.as_deref();
        let mut steps = 0usize;
        while let Some(base_name) = current {
            steps += 1;
            if steps > self.max_type_expansions {
                return Err(());
            }
            let Some(base) =
                self.module
                    .declarations
                    .iter()
                    .find_map(|declaration| match declaration {
                        Declaration::Class(base) if base.name == base_name => Some(base),
                        _ => None,
                    })
            else {
                let record = if is_static {
                    self.values.get(base_name)
                } else {
                    self.types
                        .get(base_name)
                        .filter(|definition| definition.kind == TypeDefinitionKind::Class)
                        .map(|definition| &definition.value)
                };
                return match record {
                    Some(Type::Record(fields))
                        if super::visibility::effective_visibility(fields, name).is_some() =>
                    {
                        Err(())
                    }
                    _ => Ok(None),
                };
            };
            if base
                .method_groups
                .iter()
                .any(|group| group.name == name && group.is_static == is_static)
            {
                return Ok(Some(BaseMemberKind::Method));
            }
            if base
                .members
                .iter()
                .filter_map(|member| member.accessor.as_ref())
                .any(|accessor| accessor.name == name && accessor.is_static == is_static)
            {
                return Ok(Some(BaseMemberKind::Accessor));
            }
            if base
                .parameter_property_fields()
                .iter()
                .chain(
                    base.members
                        .iter()
                        .filter_map(|member| member.field.as_ref()),
                )
                .any(|field| field.name == name && field.is_static == is_static)
            {
                return Ok(Some(BaseMemberKind::Field));
            }
            current = base.extends_name.as_deref();
        }
        Ok(None)
    }

    /// A member may not be redeclared as a different kind: a property over an
    /// accessor or method, an accessor over a property or method, a method over
    /// a property or accessor. (An accessor over an accessor, a property over a
    /// property and a method over a method are compared by their types.)
    fn validate_member_kind_conflicts(&mut self, class: &ClassDeclaration) {
        if class.extends_name.is_none() {
            return;
        }
        let parameter_properties = class.parameter_property_fields();
        let mut own: Vec<(BaseMemberKind, String, bool, SourceSpan)> = Vec::new();
        for field in parameter_properties.iter().chain(
            class
                .members
                .iter()
                .filter_map(|member| member.field.as_ref()),
        ) {
            own.push((
                BaseMemberKind::Field,
                field.name.clone(),
                field.is_static,
                field.name_span.clone(),
            ));
        }
        for group in &class.method_groups {
            own.push((
                BaseMemberKind::Method,
                group.name.clone(),
                group.is_static,
                group.span.clone(),
            ));
        }
        for accessor in class
            .members
            .iter()
            .filter_map(|member| member.accessor.as_ref())
        {
            own.push((
                BaseMemberKind::Accessor,
                accessor.name.clone(),
                accessor.is_static,
                accessor.name_span.clone(),
            ));
        }
        for (kind, name, is_static, span) in own {
            let base_kind = match self.base_member_kind(class, &name, is_static) {
                Ok(None) => continue,
                Ok(Some(base_kind)) => base_kind,
                Err(()) => {
                    // Only the type record of a base outside this module is
                    // known; an accessor over it cannot be checked.
                    if kind == BaseMemberKind::Accessor {
                        self.diagnostics.push(Diagnostic::error(
                            DiagnosticCode::UnsupportedSyntax,
                            span,
                            format!(
                                "redeclaring `{name}` as an accessor over a member of an \\
                                 imported base class is not supported yet"
                            ),
                        ));
                    }
                    continue;
                }
            };
            if base_kind == kind {
                continue;
            }
            let describe = |kind: BaseMemberKind| match kind {
                BaseMemberKind::Field => "a property",
                BaseMemberKind::Method => "a method",
                BaseMemberKind::Accessor => "an accessor",
            };
            self.type_error(
                &span,
                format!(
                    "class defines `{name}` as {} but the base class defines it as {}",
                    describe(kind),
                    describe(base_kind)
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
        // An accessor's type must be assignable to the base property it
        // overrides.
        if let Some(base_name) = class.extends_name.as_deref() {
            for is_static in [false, true] {
                for (name, group) in accessor_groups(class, is_static) {
                    let Some(own_type) = group_type(&group) else {
                        continue;
                    };
                    let Ok(Some(BaseMemberKind::Accessor)) =
                        self.base_member_kind(class, &name, is_static)
                    else {
                        continue;
                    };
                    let Some(inherited) = self.inherited_member(base_name, &name, is_static) else {
                        continue;
                    };
                    let span = group
                        .getter
                        .or(group.setter)
                        .expect("a group has an accessor")
                        .name_span
                        .clone();
                    if !self.is_assignable_bounded(&own_type, &inherited, &span) {
                        self.type_error(
                            &span,
                            format!(
                                "accessor `{name}` of type `{}` is not assignable to the base property type `{}`",
                                type_label(&own_type),
                                type_label(&inherited)
                            ),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                }
            }
        }
    }
}
