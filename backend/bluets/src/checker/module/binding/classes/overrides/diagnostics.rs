// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static-side and instance-member incompatibility have distinct TS codes.

use crate::parser::{ClassDeclaration, ClassMethodGroup};

pub(super) fn override_context(
    class: &ClassDeclaration,
    group: &ClassMethodGroup,
) -> (u32, Vec<String>) {
    let parent = crate::checker::type_label(&crate::parser::Type::Named {
        name: class.extends_name.clone().unwrap_or_default(),
        arguments: class.extends_arguments.clone(),
    });
    if group.is_static {
        (
            2417,
            vec![format!("typeof {}", class.name), format!("typeof {parent}")],
        )
    } else {
        (2416, vec![group.name.clone(), class.name.clone(), parent])
    }
}
