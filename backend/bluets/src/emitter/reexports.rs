// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CommonJS forwarding reads the source binding whenever an export is read.

use crate::parser::ValueExportDeclaration;

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

pub(super) fn commonjs(export: &ValueExportDeclaration, specifier: &str, name: &str) -> String {
    let require = format!("require({specifier:?})");
    if let Some(namespace) = &export.namespace {
        return format!("exports.{namespace} = {require};");
    }
    let mut output = format!("const {name} = {require}; ");
    if export.star {
        output.push_str(&format!(
            "for (const key in {name}) {{ if (key !== \"default\" && !Object.prototype.hasOwnProperty.call(exports, key)) Object.defineProperty(exports, key, {{ enumerable: true, get: function() {{ return {name}[key]; }} }}); }}"
        ));
    } else {
        for binding in export.bindings.iter().filter(|binding| !binding.type_only) {
            output.push_str(&format!(
                "Object.defineProperty(exports, {:?}, {{ enumerable: true, get: function() {{ return {name}[{:?}]; }} }}); ",
                binding.exported, binding.local,
            ));
        }
    }
    output
}
