// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete pinned CLDR unit-pattern provider.
//!
//! `full_cldr_compound` supplies simple-unit and generic `-per-` cells from
//! one 766-locale data source. The former language-family fallback modules
//! were superseded by this table and deliberately removed: retaining an
//! unreachable second provider would make its stale records look supported.

mod full_cldr_compound;

pub(super) use full_cldr_compound::{
    cldr_full_generic_compound_hides_number, cldr_full_generic_compound_unit_pattern,
    cldr_full_unit_pattern, has_cldr_full_generic_compound_data,
};
