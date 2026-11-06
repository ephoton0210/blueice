// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorator helper source ranges follow the original member/class syntax kind.
use super::*;

pub(in crate::emitter) fn logical_member_start(
    class: &ClassDeclaration,
    shell: &ClassMemberShell,
) -> usize {
    class.body[shell.token_start..shell.token_end]
        .iter()
        .find(|token| {
            !shell.decorators.iter().any(|decorator| {
                decorator.span.start <= token.start && token.end <= decorator.span.end
            })
        })
        .map_or(shell.span.start, |token| token.start)
}

pub(super) fn member_header(class: &ClassDeclaration, plan: &Plan<'_>) -> usize {
    if plan.kind == Kind::Method {
        plan.shell
            .key
            .first()
            .expect("a decorated method has a key")
            .start
    } else {
        logical_member_start(class, plan.shell)
    }
}

pub(super) fn class_header(class: &ClassDeclaration, source: &str, expression: bool) -> usize {
    if expression {
        return class_keyword_start(class, source);
    }
    lex(
        "<decorator-class-header>",
        &source[class.span.start..class.name_span.start],
    )
    .ok()
    .and_then(|tokens| {
        tokens.into_iter().find(|token| {
            !class.decorators.iter().any(|decorator| {
                decorator.span.start <= class.span.start + token.start
                    && class.span.start + token.end <= decorator.span.end
            })
        })
    })
    .map_or_else(
        || class_keyword_start(class, source),
        |token| class.span.start + token.start,
    )
}

pub(in crate::emitter) fn retained_legacy_completion(
    class: &ClassDeclaration,
    options: &CompilerOptions,
) -> String {
    if options.module_kind != crate::ModuleKind::Esm
        || options.target < crate::EcmaTarget::Es2015
        || class.members.iter().any(|member| {
            member.field.as_ref().is_some_and(|field| {
                field.is_static && !field.declared && field.initializer.is_some()
            })
        })
    {
        return String::new();
    }
    // The existing separator before member decorators carries the retained
    // declaration's logical end. Lowered static initialization has its own
    // unmapped construction phase and keeps that producer's existing roles.
    super::super::relocations::source_anchor(class.body_span.end)
}
