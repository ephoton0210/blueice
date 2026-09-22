// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `impl Vm` adapter methods that build, read and convert Temporal values.
//!
//! Split by concern into the `conversion/` child modules, each contributing its own
//! `impl Vm` block; this file only declares them.
//!
//! - `numeric`: integer/string/offset coercion of arguments and property-bag fields.
//! - `calendar_fields`: calendar identifiers and calendar-field resolution.
//! - `from_value`: constructors, `from`, and string/property-bag conversion.
//! - `getters`: the prototype accessors.
//! - `zoned_conversion`: `toZonedDateTime` and `ZonedDateTime` `toLocaleString`.
//! - `options`: shared option-bag readers.

mod calendar_fields;
mod from_value;
mod getters;
mod numeric;
mod options;
mod zoned_conversion;
