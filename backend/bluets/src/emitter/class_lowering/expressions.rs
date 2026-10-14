// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Downlevel class expressions keep their stores and keys local to each evaluation.

use super::*;
use crate::parser::ClassExpression;

mod names;

pub(super) fn needs_wrapper(class: &ClassDeclaration) -> bool {
    class.members.iter().any(|member| {
        member.static_block.is_some()
            || member.field.as_ref().is_some_and(|field| {
                !field.declared
                    && (field.is_static
                        || field.name.starts_with('#')
                        || member.key.first().is_some_and(|token| token.is("[")))
            })
            || member
                .method
                .as_ref()
                .is_some_and(|method| method.name.starts_with('#'))
            || member
                .accessor
                .as_ref()
                .is_some_and(|accessor| accessor.name.starts_with('#'))
    })
}

pub(super) fn lower(
    module: &Module,
    expression: &ClassExpression,
    emit: FieldEmit,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    let mut class = expression.class.clone();
    for member in &class.members {
        if member
            .key
            .iter()
            .any(|token| token.is("await") || token.is("yield"))
        {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                member.span.clone(),
                "an ES2020 class expression with state and an await/yield key needs lexical suspension lowering",
            ));
        }
    }
    let tokens = crate::lex(&module.id, &module.source).map_err(|mut errors| errors.remove(0))?;
    class.name = expression.name.clone().unwrap_or_else(|| {
        let mut name = format!("_btsClass{}", class.span.start);
        while tokens.iter().any(|token| token.text == name) {
            name.push('_');
        }
        name
    });
    if expression.name.is_none() {
        let class_token = tokens
            .iter()
            .find(|token| token.start == class.span.start && token.is("class"))
            .expect("the owned expression starts at its class token");
        edits.push(TextEdit {
            start: class_token.end,
            end: class_token.end,
            replacement: format!(" {}", class.name),
        });
    }
    let private = PrivateNames::collect(module, &class)?;
    let mut helpers = BTreeSet::new();
    if let Some(private) = &private {
        refuse_helper_name_collisions(module)?;
        private.rewrite_accesses(module, edits, &mut helpers)?;
    }
    lower_class(module, &class, emit, private.as_ref(), edits)?;
    let mut before = String::new();
    let mut after = String::new();
    let mut inner = Vec::new();
    edits.retain(|edit| {
        if edit.start < class.span.start || edit.end > class.span.end {
            return true;
        }
        if edit.start == edit.end && edit.start == class.span.start {
            before.push_str(&edit.replacement);
        } else if edit.start == edit.end && edit.start == class.span.end {
            after.push_str(&edit.replacement);
        } else {
            inner.push(edit.clone());
        }
        false
    });
    let body = render_span(&module.source, &inner, class.span.start, class.span.end);
    let variables = private
        .as_ref()
        .map(|private| format!("var {}; ", private.variables().join(", ")))
        .unwrap_or_default();
    let name = if expression.name.is_none() {
        format!(
            "Object.defineProperty({}, \"name\", {{value: {}, configurable: true}});",
            class.name,
            names::contextual_name(module, expression, &tokens)
        )
    } else {
        String::new()
    };
    edits.push(TextEdit {
        start: class.span.start,
        end: class.span.end,
        replacement: format!(
            "(() => {{{variables}{}{before}const {} = {body};{name}{after}return {};}})()",
            helper_definitions(&helpers),
            class.name,
            class.name
        ),
    });
    Ok(())
}
