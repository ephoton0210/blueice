// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Attach type operations only while constructing embedded library modules.

use crate::{Declaration, Module, Type};

pub(super) fn lower(module: &mut Module) {
    for declaration in &mut module.declarations {
        let Declaration::TypeAlias(alias) = declaration else {
            continue;
        };
        if !matches!(&alias.value, Type::Intrinsic(value) if value.kind == crate::parser::IntrinsicKind::NoInfer)
        {
            continue;
        }
        let [parameter] = alias.type_parameters.as_slice() else {
            panic!("embedded intrinsic must declare one type parameter");
        };
        alias.value = Type::inference_barrier(Type::Named {
            name: parameter.name.clone(),
            arguments: Vec::new(),
        });
    }
}
