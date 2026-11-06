// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source phases for a named forwarding getter, including string-named keys.
use crate::parser::{Module, ValueExportBinding};
use crate::TokenKind;

pub(super) struct Binding {
    pub(super) start: usize,
    pub(super) local_end: usize,
    pub(super) exported_end: usize,
    pub(super) dot: bool,
}

pub(super) fn binding(module: &Module, binding: &ValueExportBinding) -> Option<Binding> {
    let tokens = crate::lex(
        &module.id,
        &module.source[binding.span.start..binding.span.end],
    )
    .ok()?;
    let local = tokens.iter().find(|token| token.kind != TokenKind::Eof)?;
    Some(Binding {
        start: binding.span.start + local.start,
        local_end: binding.span.start + local.end,
        exported_end: binding.span.end,
        dot: matches!(local.kind, TokenKind::Identifier | TokenKind::Keyword)
            && local.text == binding.local,
    })
}
