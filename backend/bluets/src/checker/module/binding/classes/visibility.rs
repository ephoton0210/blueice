// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `private` and `protected` class members.
//!
//! A restricted member exists in a class's type record only under a marker
//! name that carries its declaring class (`private Box@main.ts secret`), never
//! under its plain name. Two consequences follow without special cases in
//! assignability: a structural target that asks for a public `secret` is not
//! satisfied by a class whose `secret` is private, and a class type that has a
//! restricted member is only satisfied by a class that inherited that very
//! declaration, which is TypeScript's nominal rule for such classes.
//!
//! While a class body is checked, the markers that body may use are exposed
//! under the plain name (`with_class_access`), and only then.

use super::*;
use crate::parser::Visibility;

/// The declaring class as it appears in a marker name: distinct modules may
/// each declare a class of the same name.
pub(in crate::checker) fn class_owner_key(class: &ClassDeclaration) -> String {
    format!("{}@{}", class.name, class.span.module)
}

/// The record field name of a member: its own name when public, else a
/// marker no identifier can spell.
pub(in crate::checker) fn member_field_name(
    class: &ClassDeclaration,
    visibility: Visibility,
    member: &str,
) -> String {
    match visibility {
        Visibility::Public => member.to_string(),
        restricted => format!(
            "{} {} {member}",
            restricted.keyword(),
            class_owner_key(class)
        ),
    }
}

/// The synthetic type name under which `super` is bound while a class body is
/// checked.
pub(in crate::checker) fn super_type_name(class: &ClassDeclaration) -> String {
    format!("super {}", class_owner_key(class))
}

/// The token index where the receiver expression ending just before the `.`
/// at `dot` begins: a chain of names, calls, index accesses and `new`.
fn receiver_start(tokens: &[Token], dot: usize) -> Option<usize> {
    let mut end = dot;
    loop {
        let last = tokens.get(end.checked_sub(1)?)?;
        if last.is(")") || last.is("]") {
            let (open, close) = if last.is(")") { ("(", ")") } else { ("[", "]") };
            let mut depth = 0usize;
            let mut index = end - 1;
            loop {
                let token = &tokens[index];
                if token.is(close) {
                    depth += 1;
                } else if token.is(open) {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                index = index.checked_sub(1)?;
            }
            end = index;
            // The callee or indexed value, unless this is a parenthesized
            // group standing on its own.
            let before = end.checked_sub(1).and_then(|before| tokens.get(before));
            let standalone = open == "("
                && before.is_none_or(|before| {
                    !(matches!(before.kind, TokenKind::Identifier | TokenKind::Keyword)
                        && !before.is("new")
                        || before.is(")")
                        || before.is("]"))
                });
            if standalone {
                return Some(end);
            }
            if before.is_some_and(|before| before.is("new")) {
                return Some(end - 1);
            }
            continue;
        }
        if matches!(
            last.kind,
            TokenKind::Identifier | TokenKind::Keyword | TokenKind::String | TokenKind::Number
        ) {
            end -= 1;
            let before = end.checked_sub(1).and_then(|before| tokens.get(before));
            match before {
                Some(before) if before.is(".") || before.is("?.") => {
                    end -= 1;
                    continue;
                }
                Some(before) if before.is("new") => return Some(end - 1),
                _ => return Some(end),
            }
        }
        return None;
    }
}

/// `(visibility, declaring class key, member)` of a marker name.
pub(in crate::checker) fn parse_restricted_name(name: &str) -> Option<(Visibility, &str, &str)> {
    let (keyword, rest) = name.split_once(' ')?;
    let (owner, member) = rest.split_once(' ')?;
    let visibility = match keyword {
        "private" => Visibility::Private,
        "protected" => Visibility::Protected,
        _ => return None,
    };
    Some((visibility, owner, member))
}

/// The accessibility of `member` in a class's record: public when the plain
/// name is present (an override may widen a protected member to public),
/// otherwise the most restrictive marker, or `None` when the member is absent.
pub(in crate::checker) fn effective_visibility(
    fields: &[TypeField],
    member: &str,
) -> Option<Visibility> {
    if fields.iter().any(|field| field.name == member) {
        return Some(Visibility::Public);
    }
    fields
        .iter()
        .filter_map(|field| parse_restricted_name(&field.name))
        .filter(|(_, _, name)| *name == member)
        .map(|(visibility, _, _)| visibility)
        .max()
}

impl ModuleChecker<'_> {
    /// A member may not be redeclared with less accessibility than the base
    /// class gives it, and a base's private member cannot be redeclared at
    /// all. Overload signatures of one method must agree on accessibility.
    pub(in crate::checker::module) fn validate_class_visibility(
        &mut self,
        class: &ClassDeclaration,
    ) {
        let mut members: Vec<(String, bool, Visibility, SourceSpan)> = Vec::new();
        let parameter_properties = class.parameter_property_fields();
        for field in parameter_properties.iter().chain(
            class
                .members
                .iter()
                .filter_map(|member| member.field.as_ref()),
        ) {
            members.push((
                field.name.clone(),
                field.is_static,
                field.visibility,
                field.name_span.clone(),
            ));
        }
        for accessor in class
            .members
            .iter()
            .filter_map(|member| member.accessor.as_ref())
        {
            members.push((
                accessor.name.clone(),
                accessor.is_static,
                accessor.visibility,
                accessor.name_span.clone(),
            ));
        }
        for group in &class.method_groups {
            let indices: Vec<usize> = group
                .signature_member_indices
                .iter()
                .copied()
                .chain(group.implementation_member_index)
                .collect();
            let visibilities: Vec<Visibility> = indices
                .iter()
                .filter_map(|&index| class.members[index].method.as_ref())
                .map(|method| method.visibility)
                .collect();
            let Some(&first) = visibilities.first() else {
                continue;
            };
            if visibilities.iter().any(|visibility| *visibility != first) {
                self.type_error(
                    &group.span,
                    format!(
                        "overload signatures of `{}` must all be public, private or protected",
                        group.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
            members.push((
                group.name.clone(),
                group.is_static,
                first,
                group.span.clone(),
            ));
        }
        let constructor_visibilities: Vec<Visibility> = class
            .members
            .iter()
            .filter_map(|member| member.constructor.as_ref())
            .map(|constructor| constructor.visibility)
            .collect();
        if let Some(&first) = constructor_visibilities.first() {
            if constructor_visibilities
                .iter()
                .any(|visibility| *visibility != first)
            {
                self.type_error(
                    &class.name_span,
                    "overload signatures of the constructor must all be public, private or protected"
                        .to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
        let Some(base_name) = class.extends_name.as_deref() else {
            return;
        };
        if self
            .class_constructors
            .get(base_name)
            .is_some_and(|binding| binding.visibility == Visibility::Private)
        {
            self.type_error(
                class.extends_span.as_ref().unwrap_or(&class.name_span),
                format!("cannot extend class `{base_name}`: its constructor is private"),
                DiagnosticCode::TypeMismatch,
            );
        }
        for (name, is_static, visibility, span) in members {
            let record = if is_static {
                self.values.get(base_name)
            } else {
                self.types
                    .get(base_name)
                    .filter(|definition| definition.kind == TypeDefinitionKind::Class)
                    .map(|definition| &definition.value)
            };
            let Some(Type::Record(fields)) = record else {
                continue;
            };
            let Some(base_visibility) = effective_visibility(fields, &name) else {
                continue;
            };
            let message = match (base_visibility, visibility) {
                (Visibility::Public, Visibility::Public) => continue,
                (Visibility::Protected, Visibility::Protected | Visibility::Public) => {
                    // The signature or type comparison against a protected
                    // member needs the base's declaration, which an imported
                    // base does not provide.
                    let imported = fields
                        .iter()
                        .filter_map(|field| parse_restricted_name(&field.name))
                        .filter(|(kind, _, member)| {
                            *kind == Visibility::Protected && *member == name
                        })
                        .any(|(_, owner, _)| {
                            owner.rsplit_once('@').map(|(_, module)| module)
                                != Some(class.span.module.as_str())
                        });
                    if imported {
                        self.diagnostics.push(Diagnostic::error(
                            DiagnosticCode::UnsupportedSyntax,
                            span,
                            format!(
                                "redeclaring the protected member `{name}` of an imported base \
                                 class is not supported yet"
                            ),
                        ));
                    }
                    continue;
                }
                (Visibility::Private, _) => format!(
                    "class `{}` incorrectly extends `{base_name}`: types have separate \
                     declarations of the private property `{name}`",
                    class.name
                ),
                (base, derived) => format!(
                    "property `{name}` is {} in class `{}` but {} in the base class `{base_name}`",
                    derived.keyword(),
                    class.name,
                    base.keyword()
                ),
            };
            self.type_error(&span, message, DiagnosticCode::TypeMismatch);
        }
    }

    /// Whether code at the current position may `new` the class: a private
    /// constructor only inside the class, a protected one also inside a
    /// subclass.
    pub(in crate::checker::module) fn constructor_is_accessible(
        &self,
        class_name: &str,
        visibility: Visibility,
    ) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Private => self.access_class.as_deref() == Some(class_name),
            Visibility::Protected => self
                .access_class
                .as_deref()
                .is_some_and(|current| self.local_class_derives_from(current, class_name)),
        }
    }

    /// The names of every private or protected member of any class in scope,
    /// used to find the member accesses that need an accessibility check.
    pub(in crate::checker::module) fn collect_restricted_member_names(&mut self) {
        let mut names = BTreeSet::new();
        let records = self
            .types
            .values()
            .filter(|definition| definition.kind == TypeDefinitionKind::Class)
            .map(|definition| &definition.value)
            .chain(
                self.class_constructors
                    .keys()
                    .filter_map(|name| self.values.get(name)),
            );
        for record in records {
            if let Type::Record(fields) = record {
                names.extend(
                    fields
                        .iter()
                        .filter_map(|field| parse_restricted_name(&field.name))
                        .map(|(_, _, member)| member.to_string()),
                );
            }
        }
        self.restricted_member_names = names;
    }

    /// The restricted declaration of `member` on the record `value` resolves
    /// to, when it has one (and, being restricted, not under its plain name).
    pub(in crate::checker::module) fn hidden_member(
        &self,
        value: &Type,
        member: &str,
    ) -> Option<(Visibility, String)> {
        let mut resolved = value.clone();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let mut visited = HashSet::new();
        while matches!(resolved, Type::Named { .. }) {
            resolved =
                instantiate_named(&resolved, &self.types, &mut visited, &mut budget, "member")?;
        }
        let Type::Record(fields) = resolved else {
            return None;
        };
        if fields.iter().any(|field| field.name == member) {
            return None;
        }
        fields
            .iter()
            .filter_map(|field| parse_restricted_name(&field.name))
            .filter(|(_, _, name)| *name == member)
            .map(|(visibility, owner, _)| (visibility, owner.to_string()))
            .max()
    }

    /// Every `.member` / `?.member` in `tokens` whose name is restricted
    /// somewhere must resolve, from the receiver's type, to a member the
    /// current class body may use. A receiver whose type cannot be settled is
    /// refused rather than assumed accessible.
    pub(in crate::checker::module) fn check_restricted_member_access(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if self.restricted_member_names.is_empty() {
            return;
        }
        for (index, token) in tokens.iter().enumerate() {
            if !(token.is(".") || token.is("?.")) {
                continue;
            }
            let Some(member) = tokens.get(index + 1).filter(|member| {
                matches!(member.kind, TokenKind::Identifier | TokenKind::Keyword)
                    && self.restricted_member_names.contains(&member.text)
            }) else {
                continue;
            };
            let member_span = SourceSpan::new(&span.module, member.start, member.end);
            let Some(start) = receiver_start(tokens, index) else {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    member_span,
                    format!(
                        "cannot prove that access to `{}` is permitted for this receiver",
                        member.text
                    ),
                ));
                continue;
            };
            let receiver = self.infer_expression(&tokens[start..index], scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            match property_type(
                &receiver,
                &member.text,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                PropertyType::Found { .. } => {}
                PropertyType::Missing => {
                    if let Some((visibility, owner)) = self.hidden_member(&receiver, &member.text) {
                        let owner = owner.split('@').next().unwrap_or(&owner).to_string();
                        self.type_error(
                            &member_span,
                            format!(
                                "property `{}` is {} and only accessible within class `{owner}`",
                                member.text,
                                visibility.keyword(),
                            ),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                }
                PropertyType::Exhausted => self.type_error(
                    &member_span,
                    format!(
                        "property lookup exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                ),
                PropertyType::Indeterminate => {
                    if !matches!(receiver, Type::Any) {
                        self.diagnostics.push(Diagnostic::error(
                            DiagnosticCode::UnsupportedSyntax,
                            member_span,
                            format!(
                                "cannot prove that access to `{}` is permitted for this receiver",
                                member.text
                            ),
                        ));
                    }
                }
            }
        }
    }

    fn local_class(&self, name: &str) -> Option<&ClassDeclaration> {
        self.module
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Class(class) if class.name == name => Some(class),
                _ => None,
            })
    }

    /// Whether the local class `name` is `ancestor` or has it in its chain of
    /// local base classes.
    fn local_class_derives_from(&self, name: &str, ancestor: &str) -> bool {
        let mut current = name;
        let mut steps = 0usize;
        loop {
            if current == ancestor {
                return true;
            }
            steps += 1;
            if steps > self.max_type_expansions {
                return false;
            }
            let Some(next) = self
                .local_class(current)
                .and_then(|class| class.extends_name.as_deref())
            else {
                return false;
            };
            current = next;
        }
    }

    /// `record` with the markers a body of `class` may use added under their
    /// plain names. `owner_record` is the class's own record of the same side,
    /// which says which protected declarations it inherited; `holder` is the
    /// class the record belongs to.
    fn expose_accessible_members(
        &self,
        record: &Type,
        class: &ClassDeclaration,
        owner_record: &[TypeField],
        holder: &str,
        is_static: bool,
    ) -> Option<Type> {
        let Type::Record(fields) = record else {
            return None;
        };
        let own_key = class_owner_key(class);
        let mut exposed = Vec::new();
        for field in fields {
            let Some((visibility, owner, member)) = parse_restricted_name(&field.name) else {
                continue;
            };
            if fields.iter().any(|other| other.name == member)
                || exposed.iter().any(|other: &TypeField| other.name == member)
            {
                continue;
            }
            let allowed = match visibility {
                Visibility::Private => owner == own_key,
                Visibility::Protected => {
                    // An instance member must be reached through the class
                    // being checked or a subclass; a static one through any
                    // class constructor.
                    (owner == own_key || owner_record.iter().any(|other| other.name == field.name))
                        && (is_static || self.local_class_derives_from(holder, &class.name))
                }
                Visibility::Public => false,
            };
            if allowed {
                exposed.push(TypeField {
                    name: member.to_string(),
                    ..field.clone()
                });
            }
        }
        if exposed.is_empty() {
            return None;
        }
        let mut fields = fields.clone();
        fields.extend(exposed);
        Some(Type::Record(fields))
    }

    /// The base class's record as `super` sees it from `class`: the plain
    /// members plus the protected *methods* of the base and its ancestors,
    /// which `super.method()` may call. `None` without a bound local base.
    fn exposed_super_record(
        &self,
        class: &ClassDeclaration,
        is_static: bool,
        owner_record: &[TypeField],
    ) -> Option<Type> {
        let base = class.extends_name.as_deref()?;
        let record = if is_static {
            self.values.get(base)
        } else {
            self.types
                .get(base)
                .filter(|definition| definition.kind == TypeDefinitionKind::Class)
                .map(|definition| &definition.value)
        };
        let Type::Record(fields) = record? else {
            return None;
        };
        let mut result = fields.clone();
        for field in fields {
            let Some((Visibility::Protected, _, member)) = parse_restricted_name(&field.name)
            else {
                continue;
            };
            if matches!(field.value, Type::Function { .. })
                && owner_record.iter().any(|other| other.name == field.name)
                && !result.iter().any(|other| other.name == member)
            {
                result.push(TypeField {
                    name: member.to_string(),
                    ..field.clone()
                });
            }
        }
        Some(Type::Record(result))
    }

    /// Runs `body` with the restricted members `class` may reach visible under
    /// their plain names on every class record, then restores the records.
    pub(in crate::checker::module) fn with_class_access<R>(
        &mut self,
        class: &ClassDeclaration,
        body: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let instance_owner: Vec<TypeField> = match self.types.get(&class.name) {
            Some(TypeDefinition {
                kind: TypeDefinitionKind::Class,
                value: Type::Record(fields),
                ..
            }) => fields.clone(),
            _ => Vec::new(),
        };
        let static_owner: Vec<TypeField> = match self.values.get(&class.name) {
            Some(Type::Record(fields)) => fields.clone(),
            _ => Vec::new(),
        };
        let mut instance_changes = Vec::new();
        for (name, definition) in &self.types {
            if definition.kind != TypeDefinitionKind::Class {
                continue;
            }
            if let Some(exposed) = self.expose_accessible_members(
                &definition.value,
                class,
                &instance_owner,
                name,
                false,
            ) {
                instance_changes.push((name.clone(), exposed));
            }
        }
        let mut static_changes = Vec::new();
        for name in self.class_constructors.keys() {
            let Some(value) = self.values.get(name) else {
                continue;
            };
            if let Some(exposed) =
                self.expose_accessible_members(value, class, &static_owner, name, true)
            {
                static_changes.push((name.clone(), exposed));
            }
        }
        let super_key = super_type_name(class);
        let super_instance = self.exposed_super_record(class, false, &instance_owner);
        let super_static = self.exposed_super_record(class, true, &static_owner);
        if let Some(value) = super_instance {
            self.types.insert(
                super_key.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Class,
                    parameters: Vec::new(),
                    value,
                },
            );
        }
        if let Some(value) = super_static {
            self.values.insert(super_key.clone(), value);
        }
        let mut saved_instances = Vec::new();
        for (name, exposed) in instance_changes {
            if let Some(definition) = self.types.get_mut(&name) {
                saved_instances.push((name, std::mem::replace(&mut definition.value, exposed)));
            }
        }
        let mut saved_statics = Vec::new();
        for (name, exposed) in static_changes {
            if let Some(previous) = self.values.insert(name.clone(), exposed) {
                saved_statics.push((name, previous));
            }
        }
        let previous_access = self.access_class.replace(class.name.clone());
        let result = body(self);
        self.access_class = previous_access;
        self.types.remove(&super_key);
        self.values.remove(&super_key);
        for (name, previous) in saved_instances {
            if let Some(definition) = self.types.get_mut(&name) {
                definition.value = previous;
            }
        }
        for (name, previous) in saved_statics {
            self.values.insert(name, previous);
        }
        result
    }
}
