// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static initialization keeps the decorated constructor as the super receiver.

use super::*;

impl Lowerer<'_, '_> {
    pub(super) fn rewrite_static_super(
        &mut self,
        class: &ClassDeclaration,
        base: &str,
        receiver: &str,
    ) -> Result<(), Diagnostic> {
        for shell in &class.members {
            let tokens = if shell.kind == ClassMemberKind::StaticBlock {
                &class.body[shell.token_start..shell.token_end]
            } else if let Some(field) = shell.field.as_ref().filter(|field| field.is_static) {
                let Some(tokens) = &field.initializer else {
                    continue;
                };
                tokens.as_slice()
            } else {
                // Retained methods and accessors use their dynamic `this` and
                // native home object, including calls on a replacement subclass.
                continue;
            };
            let mut index = 0;
            while index < tokens.len() {
                if matches!(tokens[index].text.as_str(), "class" | "function") {
                    if let Some(open) = tokens[index..].iter().position(|token| token.is("{")) {
                        if let Some(close) = matching_brace(tokens, index + open) {
                            index = close + 1;
                            continue;
                        }
                    }
                }
                if !tokens[index].is("super") {
                    index += 1;
                    continue;
                }
                let first = index;
                if !tokens.get(index + 1).is_some_and(|token| token.is(".")) {
                    return Err(unsupported(
                        &shell.span,
                        "this static super reference needs a retained named property",
                    ));
                };
                let Some(key) = tokens.get(index + 2) else {
                    return Err(unsupported(
                        &shell.span,
                        "a static super property has no key",
                    ));
                };
                index += 3;
                if tokens.get(index).is_some_and(|token| {
                    matches!(
                        token.text.as_str(),
                        "=" | "+="
                            | "-="
                            | "*="
                            | "/="
                            | "%="
                            | "**="
                            | "&&="
                            | "||="
                            | "??="
                            | "&="
                            | "|="
                            | "^="
                            | "<<="
                            | ">>="
                            | ">>>="
                            | "++"
                            | "--"
                    )
                }) {
                    return Err(unsupported(
                        &shell.span,
                        "static super property writes need receiver-aware assignment lowering",
                    ));
                }
                let read = format!("Reflect.get({base}, {}, {receiver})", quoted(&key.text));
                if let Some(call) = tokens.get(index).filter(|token| token.is("(")) {
                    let comma = if tokens.get(index + 1).is_some_and(|token| token.is(")")) {
                        ""
                    } else {
                        ", "
                    };
                    self.push(
                        tokens[first].start,
                        call.end,
                        format!("{read}.call({receiver}{comma}"),
                    );
                    index += 1;
                } else {
                    self.push(tokens[first].start, key.end, read);
                }
            }
        }
        Ok(())
    }
}
