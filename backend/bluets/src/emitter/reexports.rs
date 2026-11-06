// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CommonJS forwarding reads the source binding whenever an export is read.

use crate::parser::{Module, ValueExportDeclaration};
mod origins;

pub(super) fn class_parameter_shadows(module: &crate::parser::Module, name: &str) -> bool {
    module.declarations.iter().any(|declaration| {
        let crate::parser::Declaration::Class(class) = declaration else {
            return false;
        };
        class.members.iter().any(|member| {
            member.constructor.as_ref().is_some_and(|item| {
                item.parameters
                    .iter()
                    .any(|parameter| parameter.name == name)
            }) || member.method.as_ref().is_some_and(|item| {
                item.parameters
                    .iter()
                    .any(|parameter| parameter.name == name)
            }) || member.accessor.as_ref().is_some_and(|item| {
                item.parameters
                    .iter()
                    .any(|parameter| parameter.name == name)
            })
        })
    })
}

pub(super) fn commonjs(
    module: &Module,
    export: &ValueExportDeclaration,
    specifier: &str,
    name: &str,
    system_wrapper: bool,
) -> String {
    let require = format!("require({specifier:?})");
    if let Some(namespace) = &export.namespace {
        return format!("exports.{namespace} = {require};");
    }
    let anchor = |offset| {
        if system_wrapper {
            super::relocations::unmapped().to_string()
        } else {
            super::relocations::source_anchor(offset)
        }
    };
    let mut output = if export.star {
        format!("const {name} = {require}; ")
    } else {
        format!(
            "{}const {name} = {require};{} ",
            if system_wrapper {
                super::relocations::unmapped()
            } else {
                ""
            },
            anchor(export.span.end)
        )
    };
    if export.star {
        output.push_str(&format!(
            "for (const key in {name}) {{ if (key !== \"default\" && !Object.prototype.hasOwnProperty.call(exports, key)) (function (captured) {{ Object.defineProperty(exports, captured, {{ enumerable: true, get: function() {{ return {name}[captured]; }} }}); }})(key); }}"
        ));
    } else {
        for binding in export.bindings.iter().filter(|binding| !binding.type_only) {
            let origin = origins::binding(module, binding);
            let access = if !system_wrapper && origin.as_ref().is_some_and(|origin| origin.dot) {
                format!(".{0}", binding.local)
            } else {
                format!(
                    "[{:?}{}]",
                    binding.local,
                    origin
                        .as_ref()
                        .map_or_else(String::new, |origin| { anchor(origin.local_end) })
                )
            };
            output.push_str(&format!(
                "{}Object.defineProperty(exports, {:?}, {{ enumerable: true, get: function() {{ return {name}{access}{}; }} }});{} {}",
                origin.as_ref().map_or_else(String::new, |origin| anchor(origin.start)),
                binding.exported,
                origin.as_ref().map_or_else(String::new, |origin| anchor(origin.local_end)),
                origin.as_ref().map_or_else(String::new, |origin| anchor(origin.exported_end)),
                super::relocations::unmapped(),
            ));
        }
    }
    output
}
