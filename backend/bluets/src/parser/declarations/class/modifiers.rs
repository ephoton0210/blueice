// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class modifier origins, erasure and grammar obligations.

use super::*;

pub(super) struct MemberModifiers {
    pub(super) visibility: Visibility,
    pub(super) visibility_token: Option<usize>,
    pub(super) is_static: bool,
    pub(super) static_token: Option<usize>,
    pub(super) accessor_token: Option<usize>,
    pub(super) readonly_token: Option<usize>,
    pub(super) abstract_token: Option<usize>,
    pub(super) override_token: Option<usize>,
    pub(super) name_index: usize,
    order_error: Option<(usize, &'static str, &'static str)>,
}

pub(super) fn scan_member_modifiers(
    tokens: &[Token],
    start: usize,
    end: usize,
) -> Option<MemberModifiers> {
    let mut result = MemberModifiers {
        visibility: Visibility::Public,
        visibility_token: None,
        is_static: false,
        static_token: None,
        accessor_token: None,
        readonly_token: None,
        abstract_token: None,
        override_token: None,
        name_index: start,
        order_error: None,
    };
    let mut previous = (0, "");
    while result.name_index + 1 < end {
        let index = result.name_index;
        if !tokens
            .get(index + 1)
            .is_some_and(|token| matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword))
        {
            break;
        }
        let (rank, word, slot) = match tokens.get(index)?.text.as_str() {
            "public" => {
                result.visibility = Visibility::Public;
                (0, "public", &mut result.visibility_token)
            }
            "protected" => {
                result.visibility = Visibility::Protected;
                (0, "protected", &mut result.visibility_token)
            }
            "private" => {
                result.visibility = Visibility::Private;
                (0, "private", &mut result.visibility_token)
            }
            "static" => {
                result.is_static = true;
                (1, "static", &mut result.static_token)
            }
            "abstract" => (2, "abstract", &mut result.abstract_token),
            "override" => (3, "override", &mut result.override_token),
            "accessor" => (4, "accessor", &mut result.accessor_token),
            "readonly" => (5, "readonly", &mut result.readonly_token),
            _ => break,
        };
        if slot.replace(index).is_some() {
            return None;
        }
        if rank < previous.0 && result.order_error.is_none() {
            result.order_error = Some((index, word, previous.1));
        }
        previous = (rank, word);
        result.name_index += 1;
    }
    Some(result)
}

impl Parser {
    pub(super) fn erase_member_type_modifiers(&mut self, modifiers: &MemberModifiers) {
        for index in [modifiers.abstract_token, modifiers.override_token]
            .into_iter()
            .flatten()
        {
            self.edits.push(TextEdit {
                start: self.tokens[index].start,
                end: self.tokens[index + 1].start,
                replacement: String::new(),
            });
        }
    }

    pub(super) fn validate_member_modifiers(
        &mut self,
        start: usize,
        member: &ClassMemberShell,
        abstract_class: bool,
    ) {
        let end = start + member.token_end - member.token_start;
        let Some(modifiers) = scan_member_modifiers(&self.tokens, start, end) else {
            return;
        };
        self.erase_member_type_modifiers(&modifiers);
        if let Some((index, word, previous)) = modifiers.order_error {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    self.tokens[index].span(&self.id),
                    "class modifier is out of order",
                )
                .with_typescript(1029, vec![word.into(), previous.into()]),
            );
        }
        if member.kind == ClassMemberKind::Constructor {
            for (index, code, args) in [
                (modifiers.abstract_token, 1242, Vec::new()),
                (modifiers.override_token, 1089, vec!["override".into()]),
            ] {
                if let Some(index) = index {
                    self.diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParseError,
                            self.tokens[index].span(&self.id),
                            "invalid constructor modifier",
                        )
                        .with_typescript(code, args),
                    );
                }
            }
            return;
        }
        let Some(index) = modifiers.abstract_token else {
            return;
        };
        let modifier_span = self.tokens[index].span(&self.id);
        let conflict = if modifiers.visibility == Visibility::Private {
            Some("private")
        } else if modifiers.is_static {
            Some("static")
        } else {
            None
        };
        if let Some(conflict) = conflict {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    modifier_span.clone(),
                    "incompatible abstract member modifier",
                )
                .with_typescript(1243, vec![conflict.into(), "abstract".into()]),
            );
        }
        if !abstract_class {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    modifier_span,
                    "abstract member requires an abstract class",
                )
                .with_typescript(
                    if member.method.is_some() { 1244 } else { 1253 },
                    Vec::new(),
                ),
            );
        }
        let error = if let Some(method) = &member.method {
            method.body.is_some().then(|| {
                (
                    1245,
                    vec![method.name.clone()],
                    self.tokens[modifiers.name_index].span(&self.id),
                )
            })
        } else if let Some(accessor) = &member.accessor {
            accessor
                .body_present
                .then(|| (1318, Vec::new(), accessor.name_span.clone()))
        } else if let Some(field) = &member.field {
            field
                .initializer
                .is_some()
                .then(|| (1267, vec![field.name.clone()], field.name_span.clone()))
        } else {
            None
        };
        if let Some((code, args, span)) = error {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    span,
                    "abstract member cannot have an implementation",
                )
                .with_typescript(code, args),
            );
        }
    }
}
