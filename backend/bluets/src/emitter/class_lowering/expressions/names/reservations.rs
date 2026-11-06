// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! File-level static private captures reserve names in nested evaluation scopes.

use super::*;

pub(in crate::emitter::class_lowering) fn file_captures(
    module: &Module,
    options: &CompilerOptions,
) -> usize {
    if options.target >= EcmaTarget::Es2022 {
        return 0;
    }
    module
        .declarations
        .iter()
        .filter_map(|declaration| {
            if let Declaration::Class(class) = declaration {
                Some(class)
            } else {
                None
            }
        })
        .filter(|class| {
            let standard_factory = !options.experimental_decorators
                && (!class.decorators.is_empty()
                    || class
                        .members
                        .iter()
                        .any(|member| !member.decorators.is_empty()));
            !standard_factory
                && class.members.iter().any(|member| {
                    member.field.as_ref().is_some_and(|field| {
                        !field.declared
                            && field.is_static
                            && (field.accessor || field.name.starts_with('#'))
                    }) || member
                        .method
                        .as_ref()
                        .is_some_and(|method| method.is_static && method.name.starts_with('#'))
                        || member.accessor.as_ref().is_some_and(|accessor| {
                            accessor.is_static && accessor.name.starts_with('#')
                        })
                })
        })
        .count()
}

fn temporary(index: usize) -> Option<String> {
    if matches!(index, 8 | 13) {
        None
    } else if index < 26 {
        Some(format!("_{}", (b'a' + index as u8) as char))
    } else {
        Some(format!("_{}", index - 26))
    }
}

pub(in crate::emitter::class_lowering::expressions) fn anonymous_name(
    tokens: &[Token],
    captures: usize,
) -> String {
    let mut index = 0;
    let mut reserved = 0;
    loop {
        let candidate = temporary(index);
        index += 1;
        let Some(candidate) = candidate else { continue };
        if tokens.iter().any(|token| token.text == candidate) {
            continue;
        }
        if reserved == captures {
            return candidate;
        }
        reserved += 1;
    }
}
