// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A static import query has an owner-local identity, never a runtime binding.
use super::Type;
use crate::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportTypeReference {
    pub(crate) name: String,
    pub(crate) specifier: String,
    pub(crate) specifier_span: SourceSpan,
    pub(crate) qualifier: String,
    pub(crate) member_span: SourceSpan,
    pub(crate) value_query: bool,
    pub(crate) arguments: Vec<Type>,
    pub(crate) argument_spans: Vec<SourceSpan>,
    pub(crate) span: SourceSpan,
}

impl ImportTypeReference {
    pub(crate) fn identity(module: &str, spelling: &str) -> String {
        // The delimiter and import grammar cannot be spelled as source binding
        // names. Relative queries from different modules must not share aliases.
        format!("\0import:{module}\0{spelling}")
    }
}
