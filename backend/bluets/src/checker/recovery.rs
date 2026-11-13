// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recovery diagnostics use lexical value bindings without admitting syntax.
use super::*;

pub(crate) fn recovery_value_diagnostics(
    project: &Project,
    module: &Module,
    references: &[Token],
    max_type_expansions: usize,
) -> Vec<Diagnostic> {
    let scopes = scopes::ScopeModel::new(
        project,
        module,
        &BTreeMap::new(),
        &BTreeMap::new(),
        None,
        None,
        max_type_expansions,
    );
    references
        .iter()
        .filter(|reference| !scopes.query_has_value_binding(&reference.text, reference.start))
        .map(|reference| {
            crate::Diagnostic::error(
                crate::DiagnosticCode::UnknownName,
                reference.span(&module.id),
                "a recovered expression uses a primitive type as a value",
            )
            .with_typescript(2693, vec![reference.text.clone()])
        })
        .collect()
}
