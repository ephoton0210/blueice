// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original private operands and generated helper arguments have distinct phases.
use super::*;
use crate::emitter::relocations::{relocated_verbatim, source_anchor, unmapped};
use crate::parser::ClassField;

fn moved(source: &str, edits: &[TextEdit], start: usize, end: usize) -> (String, Vec<TextEdit>) {
    super::super::class_lowering::render_relocated_span(source, edits, start, end)
}

pub(super) fn field_store(
    variable: &str,
    field: &ClassField,
    source: &str,
    edits: &[TextEdit],
) -> (String, Vec<TextEdit>) {
    let (value, nested, end) = match field
        .initializer
        .as_deref()
        .and_then(|tokens| Some((tokens.first()?, tokens.last()?)))
    {
        Some((first, last)) => {
            let (value, nested) = moved(source, edits, first.start, last.end);
            (value, nested, last.end)
        }
        None => ("void 0".to_string(), Vec::new(), field.name_span.end),
    };
    let text = format!(
        " {}{variable}.set(this, {value}{});{}",
        source_anchor(field.name_span.start),
        source_anchor(end),
        source_anchor(field.span.end)
    );
    (text, nested)
}

pub(super) fn read(helper: &str, receiver: &Token, state: &str, tail: &str, end: usize) -> String {
    format!(
        "{}{helper}({}{}, {state}, {tail}){}",
        source_anchor(receiver.start),
        relocated_verbatim(receiver.start, receiver.end),
        source_anchor(receiver.end),
        source_anchor(end)
    )
}

pub(super) fn write(
    names: &PrivateNames<'_>,
    entry: &PrivateName,
    receiver: &Token,
    source: &str,
    edits: &[TextEdit],
    first: &Token,
    last: &Token,
) -> (String, Vec<TextEdit>) {
    let (value, nested) = moved(source, edits, first.start, last.end);
    let text = format!(
        "{}{SET_HELPER}({}{}, {}, {}{}, {}){}",
        source_anchor(receiver.start),
        relocated_verbatim(receiver.start, receiver.end),
        source_anchor(receiver.end),
        names.state(entry),
        value,
        source_anchor(last.end),
        names.write_tail(entry),
        source_anchor(last.end)
    );
    (text, nested)
}

pub(super) fn membership(state: &str, operand: &Token, inherited: usize) -> String {
    format!(
        "{}{IN_HELPER}({state}, {}{}){}",
        source_anchor(inherited),
        relocated_verbatim(operand.start, operand.end),
        source_anchor(operand.end),
        source_anchor(operand.end)
    )
}

pub(super) fn instance_setup(class: &ClassDeclaration, setup: String) -> String {
    let no_following_initializer = class.decorators.is_empty()
        && class.members.iter().all(|member| {
            member.decorators.is_empty()
                && member.static_block.is_none()
                && !member.field.as_ref().is_some_and(|field| {
                    field.is_static && !field.declared && field.initializer.is_some()
                })
        });
    if no_following_initializer {
        if let Some(rest) = setup.strip_prefix(' ') {
            return format!("{} {}{rest}", source_anchor(class.span.end), unmapped());
        }
    }
    setup
}

/// The helper tail follows the receiver; `.call` follows the complete private key.
pub(super) fn call(
    helper: &str,
    receiver: &Token,
    state: &str,
    tail: &str,
    key_end: usize,
    closing_end: Option<usize>,
) -> String {
    format!(
        "{}.call({}{}{}",
        read(helper, receiver, state, tail, key_end),
        relocated_verbatim(receiver.start, receiver.end),
        source_anchor(receiver.end),
        closing_end.map_or_else(
            || ", ".to_string(),
            |end| format!("){}", source_anchor(end))
        )
    )
}
