// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime providers contribute a private, per-module type namespace only.
use super::*;

pub(super) const RUNTIME_NAMESPACE: &str = "\0jsx-runtime.JSX";

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn bind_jsx_runtime_namespace(&mut self) {
        let Some(export) = self
            .project
            .jsx_runtime_modules
            .get(&self.module.id)
            .and_then(|target| self.namespace_exports.get(target))
            .and_then(|namespaces| namespaces.get("JSX"))
            .cloned()
        else {
            return;
        };
        self.bind_imported_namespace(
            RUNTIME_NAMESPACE,
            &export,
            &SourceSpan::new(&self.module.id, 0, 0),
            false,
            false,
        );
        self.jsx_runtime_namespace = true;
    }
}
