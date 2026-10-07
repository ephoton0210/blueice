// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A diagnostic's opening tag selects the existing component props declaration.
use crate::jsx::{JsxChild, JsxElement};
use crate::{ClassDeclaration, FunctionDeclaration, Project, SourceSpan, Type};
pub(super) fn owner(
    project: &Project,
    span: &SourceSpan,
    children: bool,
    classes: &[&ClassDeclaration],
    functions: &[&FunctionDeclaration],
) -> Option<String> {
    let source = project.source(&span.module)?;
    let tokens = crate::syntax::lex(&span.module, source).ok()?;
    let name = tokens
        .iter()
        .filter(|token| token.kind == crate::syntax::TokenKind::JsxElement)
        .filter_map(|token| crate::syntax::parse_jsx(&span.module, source, token.start).ok())
        .find_map(|element| name_at(&element, span, children))?;
    let named = |ty: &Type| match ty {
        Type::Named { name, .. } => Some(crate::parser::source_type_name(name).to_string()),
        _ => None,
    };
    if let Some(function) = functions.iter().find(|function| function.name == name) {
        return function
            .parameters
            .first()
            .and_then(|parameter| parameter.annotation.as_ref())
            .and_then(named);
    }
    let class = classes.iter().find(|class| class.name == name)?;
    class
        .members
        .iter()
        .filter_map(|member| member.field.as_ref())
        .find(|field| field.name == "props")?
        .annotation
        .as_ref()
        .and_then(named)
}
fn name_at(element: &JsxElement, span: &SourceSpan, children: bool) -> Option<String> {
    if element.start > span.start || element.end < span.end {
        return None;
    }
    if children
        && element
            .children
            .iter()
            .any(|child| matches!(child,JsxChild::Element(child) if child.start==span.start))
    {
        return element.name.as_ref().map(|name| name.text.clone());
    }
    if span.start < element.opening_end {
        return element.name.as_ref().map(|name| name.text.clone());
    }
    element.children.iter().find_map(|child| match child {
        JsxChild::Element(child) => name_at(child, span, children),
        _ => None,
    })
}
