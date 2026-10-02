// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Typing of JSX elements (`.tsx`), against the declared `JSX` namespace.
//!
//! The rules follow TypeScript 5.9.3's: an intrinsic tag (`div`) takes the props
//! its entry in `JSX.IntrinsicElements` declares; a value tag (`Foo`) is a
//! function whose first parameter is its props; the attributes written, spread
//! and carried as children are checked against those props (value types,
//! unknown names, missing required props), with `JSX.IntrinsicAttributes` names
//! (such as `key`) always allowed and hyphenated names never checked; and an
//! element's own type is `JSX.Element`.
//!
//! Where BlueTS lacks the type machinery a check needs (generic components,
//! class components, props that are intersections or unions) the element is
//! still resolved and its embedded expressions are still checked, but its
//! attributes are not compared with props; `PLAN.md` records each such gap.

use super::*;
use crate::jsx::{self, JsxAttribute, JsxChild, JsxElement, JsxName, JsxValue};

/// What an element's attributes are checked against.
enum Props {
    /// No further checking: the target is not resolvable here.
    Open,
    /// The props an element's attributes are checked against, and whether it is
    /// a value-based element (`JSX.IntrinsicAttributes` such as `key` apply to
    /// those only).
    Known {
        fields: Vec<TypeField>,
        value_based: bool,
    },
}

impl<'a> ModuleChecker<'a> {
    /// Checks every JSX element among `tokens` (the embedded expressions inside
    /// them are checked as part of their element).
    pub(super) fn check_jsx_elements(&mut self, tokens: &[Token], scope: &BTreeMap<String, Type>) {
        for token in tokens
            .iter()
            .filter(|token| token.kind == TokenKind::JsxElement)
        {
            if self.checked_jsx_elements.contains(&token.start) {
                continue;
            }
            self.checked_jsx_elements.insert(token.start);
            let Ok(element) =
                crate::syntax::parse_jsx(&self.module.id, &self.module.source, token.start)
            else {
                continue;
            };
            if self.jsx_mode.is_none() {
                self.type_error(
                    &SourceSpan::new(&self.module.id, element.start, element.end),
                    "cannot use JSX unless the `jsx` option is provided".to_string(),
                    DiagnosticCode::UnsupportedSyntax,
                );
                continue;
            }
            self.check_jsx_element(&element, scope);
        }
    }

    fn jsx_span(&self, start: usize, end: usize) -> SourceSpan {
        SourceSpan::new(&self.module.id, start, end.max(start))
    }

    /// The declared name of a member of the `JSX` namespace: the factory
    /// namespace's own (`React.JSX.Element`) when the program has one, else the
    /// global (`JSX.Element`), as TypeScript looks it up.
    fn jsx_key(&self, member: &str) -> Option<String> {
        let factory = self
            .jsx_pragmas
            .factory
            .clone()
            .or_else(|| self.jsx_factory.clone())
            .unwrap_or_else(|| "React".to_string());
        let root = factory.split('.').next().unwrap_or("React");
        [format!("{root}.JSX.{member}"), format!("JSX.{member}")]
            .into_iter()
            .find(|key| self.types.contains_key(key))
    }

    /// `JSX.Element` when the program declares it, otherwise `any`.
    pub(super) fn jsx_element_type(&self) -> Type {
        match self.jsx_key("Element") {
            Some(name) => Type::Named {
                name,
                arguments: Vec::new(),
            },
            None => Type::Any,
        }
    }

    fn jsx_interface_fields(&self, member: &str) -> Option<Vec<TypeField>> {
        let named = Type::Named {
            name: self.jsx_key(member)?,
            arguments: Vec::new(),
        };
        self.expanded_record_fields(named).0
    }

    /// The name `JSX.ElementChildrenAttribute` declares for children, if any.
    fn jsx_children_name(&self) -> Option<String> {
        self.jsx_interface_fields("ElementChildrenAttribute")?
            .into_iter()
            .next()
            .map(|field| field.name)
    }

    pub(super) fn check_jsx_element(
        &mut self,
        element: &JsxElement,
        scope: &BTreeMap<String, Type>,
    ) -> Type {
        let span = self.jsx_span(element.start, element.opening_end);
        self.check_jsx_factory(&span, element);
        let props = match &element.name {
            None => Props::Open,
            Some(name) => self.jsx_props(name, &span, scope),
        };
        self.check_jsx_attributes(element, &props, scope);
        self.check_jsx_children(element, &props, scope);
        self.jsx_element_type()
    }

    /// Classic mode calls a factory (and, for a fragment, a fragment factory)
    /// that must be in scope; the automatic runtime and `preserve` need none.
    fn check_jsx_factory(&mut self, span: &SourceSpan, element: &JsxElement) {
        if self.jsx_mode != Some(crate::compiler::JsxMode::React) {
            return;
        }
        let (factory, option) = if element.is_fragment() {
            (
                self.jsx_pragmas
                    .fragment
                    .clone()
                    .or_else(|| self.jsx_fragment_factory.clone())
                    .unwrap_or_else(|| "React.Fragment".to_string()),
                "jsxFragmentFactory",
            )
        } else {
            (
                self.jsx_pragmas
                    .factory
                    .clone()
                    .or_else(|| self.jsx_factory.clone())
                    .unwrap_or_else(|| "React.createElement".to_string()),
                "jsxFactory",
            )
        };
        let root = factory.split('.').next().unwrap_or(&factory).to_string();
        if !self.name_is_in_scope(&root) {
            self.type_error(
                span,
                format!(
                    "cannot find the name `{root}` that the JSX `{option}` `{factory}` needs in scope"
                ),
                DiagnosticCode::UnknownName,
            );
        }
    }

    pub(super) fn name_is_in_scope(&self, name: &str) -> bool {
        self.values.contains_key(name)
            || self.functions.contains_key(name)
            || self.class_constructors.contains_key(name)
            || self.types.contains_key(name)
            || self.namespaces.contains_key(name)
            || self.const_enums.contains(name)
            || self.enum_members.contains_key(name)
    }

    fn jsx_props(
        &mut self,
        name: &JsxName,
        span: &SourceSpan,
        scope: &BTreeMap<String, Type>,
    ) -> Props {
        if jsx::is_intrinsic_name(&name.text) {
            let Some(fields) = self.jsx_interface_fields("IntrinsicElements") else {
                if self.jsx_key("IntrinsicElements").is_some() {
                    return Props::Open;
                }
                self.type_error(
                    span,
                    "JSX element implicitly has type `any` because no interface `JSX.IntrinsicElements` exists"
                        .to_string(),
                    DiagnosticCode::TypeMismatch,
                );
                return Props::Open;
            };
            return match fields.into_iter().find(|field| field.name == name.text) {
                Some(field) => self.props_of(&field.value, false),
                None => {
                    self.type_error(
                        span,
                        format!(
                            "property `{}` does not exist on type `JSX.IntrinsicElements`",
                            name.text
                        ),
                        DiagnosticCode::UnknownName,
                    );
                    Props::Open
                }
            };
        }
        // A value tag: a function component, whose first parameter is its props.
        if let Some(signatures) = self.functions.get(&name.text) {
            let Some(signature) = signatures.first().cloned() else {
                return Props::Open;
            };
            return self.component_props(
                &signature.parameters,
                &signature.return_type,
                &signature.type_parameters,
                span,
            );
        }
        if self.class_constructors.contains_key(&name.text) {
            return self.class_props(&name.text, span);
        }
        let bound = scope
            .get(&name.text)
            .or_else(|| self.values.get(&name.text))
            .cloned();
        match bound {
            Some(Type::Function { parameters, result }) => {
                self.component_props(&parameters, &result, &[], span)
            }
            Some(_) => Props::Open,
            None if self.name_is_in_scope(name.text.split('.').next().unwrap_or(&name.text)) => {
                Props::Open
            }
            None => {
                self.type_error(
                    span,
                    format!("cannot find the JSX tag name `{}`", name.text),
                    DiagnosticCode::UnknownName,
                );
                Props::Open
            }
        }
    }

    /// A class component's props: the member of its instance type that
    /// `JSX.ElementAttributesProperty` names, or the instance type itself.
    fn class_props(&mut self, class: &str, span: &SourceSpan) -> Props {
        let instance = Type::Named {
            name: class.to_string(),
            arguments: Vec::new(),
        };
        if let Some(class_type) = self.jsx_key("ElementClass") {
            let required = Type::Named {
                name: class_type,
                arguments: Vec::new(),
            };
            if !self.is_assignable_bounded(&instance, &required, span) {
                self.type_error(
                    span,
                    format!("class `{class}` cannot be used as a JSX component: its instance type is not a `JSX.ElementClass`"),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
        let Some(attribute) = self
            .jsx_interface_fields("ElementAttributesProperty")
            .and_then(|fields| fields.into_iter().next())
        else {
            return self.props_of(&instance, true);
        };
        let mut visited = HashSet::new();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        match property_type(
            &instance,
            &attribute.name,
            &self.types,
            &mut visited,
            &mut budget,
        ) {
            PropertyType::Found { value, .. } => self.props_of(&value, true),
            _ => Props::Open,
        }
    }

    fn component_props(
        &mut self,
        parameters: &[Parameter],
        result: &Type,
        type_parameters: &[TypeParameter],
        span: &SourceSpan,
    ) -> Props {
        if self.jsx_key("Element").is_some() {
            let valid = Type::Union(vec![self.jsx_element_type(), Type::Null]);
            if !matches!(result, Type::Any | Type::Unknown)
                && !self.is_assignable_bounded(result, &valid, span)
            {
                self.type_error(
                    span,
                    format!(
                        "this component's return type `{}` is not a valid JSX element",
                        type_label(result)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
        if !type_parameters.is_empty() {
            return Props::Open;
        }
        match parameters.first() {
            None => Props::Known {
                fields: Vec::new(),
                value_based: true,
            },
            Some(parameter) => match &parameter.annotation {
                Some(annotation) => self.props_of(annotation, true),
                None => Props::Open,
            },
        }
    }

    fn props_of(&self, value: &Type, value_based: bool) -> Props {
        match value {
            Type::Any | Type::Unknown => Props::Open,
            other => match self.expanded_record_fields(other.clone()).0 {
                Some(fields) => Props::Known {
                    fields,
                    value_based,
                },
                None => Props::Open,
            },
        }
    }

    fn check_jsx_attributes(
        &mut self,
        element: &JsxElement,
        props: &Props,
        scope: &BTreeMap<String, Type>,
    ) {
        let value_based = matches!(
            props,
            Props::Known {
                value_based: true,
                ..
            }
        );
        let intrinsic: Vec<String> = if value_based {
            self.jsx_interface_fields("IntrinsicAttributes")
                .unwrap_or_default()
                .into_iter()
                .map(|field| field.name)
                .collect()
        } else {
            Vec::new()
        };
        let mut provided: BTreeSet<String> = BTreeSet::new();
        let mut open_spread = false;
        for attribute in &element.attributes {
            match attribute {
                JsxAttribute::Spread {
                    start, end, tokens, ..
                } => {
                    let span = self.jsx_span(*start, *end);
                    self.check_direct_runtime_expression(tokens, scope, &span);
                    let spread = self.infer_expression(tokens, scope);
                    match &spread {
                        Type::Any | Type::Unknown => open_spread = true,
                        other => match self.expanded_record_fields(other.clone()).0 {
                            Some(fields) => {
                                for field in fields {
                                    if let Props::Known {
                                        fields: expected, ..
                                    } = props
                                    {
                                        self.check_jsx_attribute_value(
                                            &field.name,
                                            &field.value,
                                            expected,
                                            &span,
                                        );
                                    }
                                    if !field.optional {
                                        provided.insert(field.name);
                                    }
                                }
                            }
                            None => {
                                self.type_error(
                                    &span,
                                    format!(
                                        "spread types may only be created from object types, not `{}`",
                                        type_label(other)
                                    ),
                                    DiagnosticCode::TypeMismatch,
                                );
                                open_spread = true;
                            }
                        },
                    }
                }
                JsxAttribute::Named {
                    name,
                    value,
                    start,
                    end,
                } => {
                    let span = self.jsx_span(*start, *end);
                    let actual_tokens;
                    let expected_field = match props {
                        Props::Known { fields, .. } => {
                            fields.iter().find(|field| field.name == name.text)
                        }
                        Props::Open => None,
                    };
                    let actual = match value {
                        None => Type::Boolean,
                        Some(JsxValue::String { start, end }) => {
                            Type::Literal(self.module.source[*start..*end].to_string())
                        }
                        Some(JsxValue::Expression {
                            tokens, start, end, ..
                        }) => {
                            let expression_span = self.jsx_span(*start, *end);
                            self.check_direct_runtime_expression(tokens, scope, &expression_span);
                            actual_tokens = tokens;
                            match expected_field {
                                Some(field) => {
                                    self.infer_in_context(actual_tokens, scope, &field.value)
                                }
                                None => self.infer_expression(actual_tokens, scope),
                            }
                        }
                        Some(JsxValue::Element(inner)) => self.check_jsx_element(inner, scope),
                    };
                    provided.insert(name.text.clone());
                    if let Props::Known { fields, .. } = props {
                        match expected_field {
                            Some(field) => {
                                let expected = field.value.clone();
                                if !matches!(actual, Type::Any | Type::Unknown)
                                    && !self.is_assignable_bounded(&actual, &expected, &span)
                                {
                                    self.type_error(
                                        &span,
                                        format!(
                                            "attribute `{}` has type `{}`, which is not assignable to `{}`",
                                            name.text,
                                            type_label(&actual),
                                            type_label(&expected)
                                        ),
                                        DiagnosticCode::TypeMismatch,
                                    );
                                }
                            }
                            None if name.text.contains('-')
                                || name.text.contains(':')
                                || intrinsic.contains(&name.text) => {}
                            None => {
                                let known = fields
                                    .iter()
                                    .map(|field| field.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                self.type_error(
                                    &span,
                                    format!(
                                        "attribute `{}` does not exist on the element's props (`{known}`)",
                                        name.text
                                    ),
                                    DiagnosticCode::TypeMismatch,
                                );
                            }
                        }
                    }
                }
            }
        }
        if let (Props::Known { fields, .. }, false) = (props, open_spread) {
            let children = self.jsx_children_name();
            let has_children = children.is_some() && Self::has_semantic_children(element);
            let missing: Vec<&str> = fields
                .iter()
                .filter(|field| !field.optional && !matches!(field.value, Type::Undefined))
                .filter(|field| {
                    let given_as_children =
                        has_children && children.as_deref() == Some(field.name.as_str());
                    !provided.contains(&field.name) && !given_as_children
                })
                .map(|field| field.name.as_str())
                .collect();
            if !missing.is_empty() {
                let span = self.jsx_span(element.start, element.opening_end);
                self.type_error(
                    &span,
                    format!("missing required props: {}", missing.join(", ")),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    fn check_jsx_attribute_value(
        &mut self,
        name: &str,
        actual: &Type,
        expected: &[TypeField],
        span: &SourceSpan,
    ) {
        if let Some(field) = expected.iter().find(|field| field.name == name) {
            let target = field.value.clone();
            if !matches!(actual, Type::Any | Type::Unknown)
                && !self.is_assignable_bounded(actual, &target, span)
            {
                self.type_error(
                    span,
                    format!(
                        "spread attribute `{name}` has type `{}`, which is not assignable to `{}`",
                        type_label(actual),
                        type_label(&target)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    /// Children that count: text that is not whitespace between lines, and any
    /// non-empty expression or element.
    fn has_semantic_children(element: &JsxElement) -> bool {
        !Self::semantic_children(element).is_empty()
    }

    fn semantic_children(element: &JsxElement) -> Vec<&JsxChild> {
        element
            .children
            .iter()
            .filter(|child| match child {
                JsxChild::Text { .. } => true,
                JsxChild::Expression { tokens, spread, .. } => *spread || !tokens.is_empty(),
                JsxChild::Element(_) => true,
            })
            .collect()
    }

    fn check_jsx_children(
        &mut self,
        element: &JsxElement,
        props: &Props,
        scope: &BTreeMap<String, Type>,
    ) {
        let mut types: Vec<Type> = Vec::new();
        for child in &element.children {
            match child {
                JsxChild::Text { start, end } => {
                    let raw = &self.module.source[*start..*end];
                    if !jsx::is_blank_with_newline(raw) {
                        types.push(Type::String);
                    }
                }
                JsxChild::Expression {
                    start,
                    end,
                    tokens,
                    spread,
                    ..
                } => {
                    if tokens.is_empty() {
                        continue;
                    }
                    let span = self.jsx_span(*start, *end);
                    self.check_direct_runtime_expression(tokens, scope, &span);
                    let inferred = self.infer_expression(tokens, scope);
                    types.push(if *spread { Type::Any } else { inferred });
                }
                JsxChild::Element(inner) => {
                    let inner_type = self.check_jsx_element(inner, scope);
                    types.push(inner_type);
                }
            }
        }
        let (Some(children_name), Props::Known { fields, .. }) = (self.jsx_children_name(), props)
        else {
            return;
        };
        if types.is_empty() {
            return;
        }
        let span = self.jsx_span(element.opening_end, element.closing_start);
        let Some(field) = fields.iter().find(|field| field.name == children_name) else {
            self.type_error(
                &span,
                format!("this element has children but its props declare no `{children_name}`"),
                DiagnosticCode::TypeMismatch,
            );
            return;
        };
        let expected = field.value.clone();
        let actual = if types.len() == 1 {
            types.remove(0)
        } else {
            Type::Array(Box::new(Type::Union(types)))
        };
        if !matches!(actual, Type::Any | Type::Unknown)
            && !self.is_assignable_bounded(&actual, &expected, &span)
        {
            self.type_error(
                &span,
                format!(
                    "the element's children have type `{}`, which is not assignable to `{children_name}: {}`",
                    type_label(&actual),
                    type_label(&expected)
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }
}
