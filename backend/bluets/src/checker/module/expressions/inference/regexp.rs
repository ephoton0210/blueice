// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recognize an entire regexp operand; do not mistake a later division for it.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn infer_regexp_literal(&self, tokens: &[Token]) -> Option<Type> {
        if tokens.len() == 1 && tokens[0].kind == TokenKind::RegExp {
            return Some(Type::Named {
                name: "RegExp".into(),
                arguments: Vec::new(),
            });
        }
        if !tokens.first()?.is("/") {
            return None;
        }
        let source = self
            .module
            .source
            .get(tokens.first()?.start..tokens.last()?.end)?;
        let mut escaped = false;
        let mut class = false;
        for (offset, character) in source.char_indices().skip(1) {
            if escaped {
                escaped = false;
                continue;
            }
            match character {
                '\\' => escaped = true,
                '[' => class = true,
                ']' => class = false,
                '\n' | '\r' | '\u{2028}' | '\u{2029}' => return None,
                '/' if !class => {
                    if source[offset + 1..]
                        .chars()
                        .all(|flag| "dgimsuvy".contains(flag))
                    {
                        return Some(Type::Named {
                            name: "RegExp".into(),
                            arguments: Vec::new(),
                        });
                    }
                    return None;
                }
                _ => {}
            }
        }
        None
    }
}
