// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Backing storage and each synthesized callable retain distinct original roles.

use super::*;
use crate::parser::ClassField;

fn static_modifier(class: &ClassDeclaration, shell: &ClassMemberShell) -> String {
    let token = class.body[shell.token_start..shell.token_end]
        .iter()
        .find(|token| {
            token.is("static")
                && !shell.decorators.iter().any(|decorator| {
                    decorator.span.start <= token.start && token.end <= decorator.span.end
                })
        })
        .expect("a static auto-accessor has its original modifier");
    format!(
        "{}static{} ",
        super::super::relocations::source_anchor(token.start),
        super::super::relocations::source_anchor(token.end)
    )
}

pub(in crate::emitter) fn storage_prefix(
    class: &ClassDeclaration,
    shell: &ClassMemberShell,
    field: &ClassField,
    storage: &str,
    legacy: bool,
    lowered: bool,
) -> String {
    let modifier = if field.is_static {
        static_modifier(class, shell)
    } else {
        super::super::relocations::source_anchor(if legacy || lowered {
            field.name_span.start
        } else {
            member_origins::logical_member_start(class, shell)
        })
    };
    let field_name = if lowered && field.is_static {
        super::super::relocations::source_anchor(field.name_span.start)
    } else {
        String::new()
    };
    format!("{modifier}{field_name}{storage} = ")
}

pub(in crate::emitter) fn callable_pair(
    class: &ClassDeclaration,
    shell: &ClassMemberShell,
    field: &ClassField,
    storage: &str,
    legacy: bool,
    lowered: bool,
) -> String {
    let anchor = super::super::relocations::source_anchor;
    let start = if legacy {
        field.name_span.start
    } else {
        member_origins::logical_member_start(class, shell)
    };
    let getter = if field.is_static {
        static_modifier(class, shell)
    } else {
        anchor(start)
    };
    // TypeScript emits the static getter from the modifier node, while its
    // synthesized setter begins at the original declaration/name phase.
    let setter = if field.is_static {
        format!("{}static ", anchor(start))
    } else {
        anchor(start)
    };
    let key = super::relocated(field.name_span.start, field.name_span.end);
    let key_end = anchor(field.name_span.end);
    let completion = anchor(shell.span.end);
    let receiver = if field.is_static {
        let start = if lowered {
            field.name_span.end
        } else {
            class.name_span.start
        };
        let end = if lowered {
            field.name_span.end
        } else {
            class.name_span.end
        };
        format!("{}{}{}", anchor(start), class.name, anchor(end))
    } else {
        "this".to_string()
    };
    format!(
        "{getter}get {key}{key_end}() {{ return {receiver}.{storage}; }}{completion} {setter}set {key}{key_end}(value) {{ {receiver}.{storage} = value; }}{completion} "
    )
}
