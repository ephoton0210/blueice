// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX factories are implicit value uses at the element's lexical location.
use super::*;
use crate::jsx::{JsxAttribute, JsxChild, JsxElement, JsxValue, Pragmas};

pub(super) fn references(
    model: &ScopeModel<'_>,
    mode: Option<crate::JsxMode>,
    factory: Option<&str>,
    fragment: Option<&str>,
) -> Vec<Reference> {
    let mut references = Vec::new();
    let Some(mode) = mode.filter(|_| model.module.id.ends_with(".tsx")) else {
        return references;
    };
    let pragmas = Pragmas::of(&model.module.source);
    let automatic = match pragmas.runtime.as_deref() {
        Some("classic") => false,
        Some("automatic") => true,
        _ => {
            matches!(mode, crate::JsxMode::ReactJsx | crate::JsxMode::ReactJsxDev)
                || pragmas.import_source.is_some()
        }
    };
    if automatic {
        return references;
    }
    // Preserved JSX also retains its classic factory's alias, as native does.
    let factory = pragmas
        .factory
        .as_deref()
        .or(factory)
        .unwrap_or("React.createElement");
    let fragment = pragmas
        .fragment
        .as_deref()
        .or(fragment)
        .unwrap_or("React.Fragment");
    for token in &model.tokens {
        if token.kind == TokenKind::JsxElement {
            if let Ok(element) =
                crate::syntax::parse_jsx(&model.module.id, &model.module.source, token.start)
            {
                element_references(model, &element, factory, fragment, &mut references);
            }
        }
    }
    references
}

fn element_references(
    model: &ScopeModel<'_>,
    element: &JsxElement,
    factory: &str,
    fragment: &str,
    references: &mut Vec<Reference>,
) {
    for name in [Some(factory), element.is_fragment().then_some(fragment)]
        .into_iter()
        .flatten()
    {
        references.push(Reference {
            scope: model.scope_at(element.start),
            name: name.into(),
            meaning: Meaning::Value,
            span: SourceSpan::new(&model.module.id, element.start, element.opening_end),
        });
    }
    for attribute in &element.attributes {
        if let JsxAttribute::Named {
            value: Some(JsxValue::Element(child)),
            ..
        } = attribute
        {
            element_references(model, child, factory, fragment, references);
        }
    }
    for child in &element.children {
        if let JsxChild::Element(child) = child {
            element_references(model, child, factory, fragment, references);
        }
    }
}
