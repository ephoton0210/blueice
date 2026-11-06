// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Separate an original callable's source end from its generated class close.

use super::super::relocations;
use super::*;

pub(super) fn retain(module: &Module, options: &CompilerOptions, edits: &mut Vec<TextEdit>) {
    if options.target < EcmaTarget::Es2015 {
        return;
    }
    // Only the source declaration pass owns this boundary. Decorator-produced
    // classes and source class expressions have separate construction phases.
    for (declaration, _) in super::super::runtime_declarations(&module.declarations) {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        if super::super::decorators::lowers_class(class, options) {
            continue;
        }
        let Some((index, member)) = class
            .members
            .iter()
            .enumerate()
            .rfind(|(_, member)| retained_body(member, options) && !erased(member, edits))
        else {
            continue;
        };
        let mut end = member.span.end;
        let collapsed_tail = class.members[index + 1..].iter().all(|later| {
            // Consume only real erasures already chosen by the member pass.
            // A native field or generated static block therefore stops this
            // proof, and an erased member's surviving line breaks stop it too.
            let collapsed = later.span.start == end
                && erased(later, edits)
                && !module.source[later.span.start..later.span.end].contains(['\r', '\n']);
            end = later.span.end;
            collapsed
        });
        if collapsed_tail && end + 1 == class.body_span.end {
            // Native keeps callable.end mapped and the following class brace
            // unmapped. A physical space gives both points their own offset.
            super::insert(
                edits,
                member.span.end,
                format!(
                    "{} {}",
                    relocations::source_anchor(member.span.end),
                    relocations::unmapped()
                ),
            );
        }
    }
}

fn erased(member: &ClassMemberShell, edits: &[TextEdit]) -> bool {
    edits.iter().any(|edit| {
        edit.start == member.span.start
            && edit.end == member.span.end
            && edit.replacement.is_empty()
    })
}

fn retained_body(member: &ClassMemberShell, options: &CompilerOptions) -> bool {
    member
        .constructor
        .as_ref()
        .is_some_and(|constructor| constructor.body.is_some())
        || member.method.as_ref().is_some_and(|method| {
            method.body.is_some()
                && (options.target >= EcmaTarget::Es2022 || !method.name.starts_with('#'))
        })
        || member.accessor.as_ref().is_some_and(|accessor| {
            accessor.body_present
                && (options.target >= EcmaTarget::Es2022 || !accessor.name.starts_with('#'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_original_retained_adjacent_callable_receives_a_carrier() {
        for (source, target, expected) in [
            ("export class C{read(){return 1;}}", EcmaTarget::Es2015, 1),
            ("export class C{constructor(){}}", EcmaTarget::Es2022, 1),
            (
                "export class C{get value(){return 1;}}",
                EcmaTarget::Es2022,
                1,
            ),
            (
                "export class C{set value(value:number){}}",
                EcmaTarget::EsNext,
                1,
            ),
            ("export class C{read(){return 1;} }", EcmaTarget::Es2015, 0),
            (
                "export class C{read(){return 1;} value=2;}",
                EcmaTarget::Es2015,
                0,
            ),
            ("export class C{read(){return 1;}}", EcmaTarget::Es5, 0),
            (
                "export const C=class C{read(){return 1;}};",
                EcmaTarget::Es2015,
                0,
            ),
            ("export class C{#read(){return 1;}}", EcmaTarget::Es2015, 0),
            ("export class C{#read(){return 1;}}", EcmaTarget::Es2022, 1),
            (
                "@dec export class C{read(){return 1;}}",
                EcmaTarget::Es2022,
                0,
            ),
        ] {
            let module = crate::parse_module("memory:///main.ts", source).unwrap();
            let mut edits = Vec::new();
            retain(
                &module,
                &CompilerOptions {
                    target,
                    ..CompilerOptions::default()
                },
                &mut edits,
            );
            assert_eq!(edits.len(), expected, "{target:?}: {source}");
        }
    }

    #[test]
    fn a_trailing_field_requires_the_existing_exact_erasure() {
        let module = crate::parse_module(
            "memory:///main.ts",
            "export class C{read(){return 1;}value=2;}",
        )
        .unwrap();
        let Declaration::Class(class) = &module.declarations[0] else {
            unreachable!()
        };
        let field = &class.members[1];
        for replacement in ["", "static {this.value=2;}"] {
            let mut edits = vec![TextEdit {
                start: field.span.start,
                end: field.span.end,
                replacement: replacement.to_string(),
            }];
            retain(
                &module,
                &CompilerOptions {
                    target: EcmaTarget::Es2015,
                    ..CompilerOptions::default()
                },
                &mut edits,
            );
            assert_eq!(edits.len(), if replacement.is_empty() { 2 } else { 1 });
        }
    }
}
