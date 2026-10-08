// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic selection has no role in JavaScript emission or runtime grants.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckingOptions {
    pub no_implicit_any: bool,
    pub no_implicit_this: bool,
    pub strict_null_checks: bool,
    pub strict_function_types: bool,
    pub strict_bind_call_apply: bool,
    pub strict_property_initialization: bool,
    pub strict_builtin_iterator_return: bool,
    pub always_strict: bool,
    pub use_unknown_in_catch_variables: bool,
    pub no_unused_locals: bool,
    pub no_unused_parameters: bool,
    pub no_implicit_returns: bool,
    pub no_implicit_override: bool,
    pub no_fallthrough_cases_in_switch: bool,
    pub exact_optional_property_types: bool,
    pub no_unchecked_indexed_access: bool,
}

impl Default for CheckingOptions {
    fn default() -> Self {
        Self {
            no_implicit_any: true,
            no_implicit_this: true,
            strict_null_checks: true,
            strict_function_types: true,
            strict_bind_call_apply: true,
            strict_property_initialization: true,
            strict_builtin_iterator_return: true,
            always_strict: true,
            use_unknown_in_catch_variables: true,
            no_unused_locals: false,
            no_unused_parameters: false,
            no_implicit_returns: false,
            no_implicit_override: false,
            no_fallthrough_cases_in_switch: false,
            exact_optional_property_types: false,
            no_unchecked_indexed_access: false,
        }
    }
}

impl CheckingOptions {
    pub(crate) fn legacy() -> Self {
        Self {
            exact_optional_property_types: true,
            ..Self::default()
        }
    }
}
