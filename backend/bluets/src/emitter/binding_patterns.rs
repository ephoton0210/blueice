// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration patterns keep binding structure while dropping runtime defaults.
use crate::parser::BindingPattern;

pub(super) fn declaration(pattern: &BindingPattern) -> String {
    match pattern {
        BindingPattern::Object(bindings) => format!(
            "{{ {} }}",
            bindings
                .iter()
                .map(|binding| {
                    if let Some(nested) = &binding.nested {
                        format!("{}: {}", binding.key, declaration(&nested.pattern))
                    } else if binding.key == binding.name {
                        binding.name.clone()
                    } else {
                        format!("{}: {}", binding.key, binding.name)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        BindingPattern::Array(bindings) => format!(
            "[{}]",
            bindings
                .iter()
                .map(|binding| binding
                    .as_ref()
                    .map(|binding| binding.nested.as_ref().map_or_else(
                        || binding.name.clone(),
                        |nested| declaration(&nested.pattern)
                    ))
                    .unwrap_or_default())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
