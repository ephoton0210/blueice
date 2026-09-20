// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.PlainDate` / `Temporal.PlainDateTime` (Phase 26 Stage 2): the
//! `impl Vm` adapters that bridge these types to JavaScript, grouped by
//! concern. Every method is an inherent `Vm` method, so nothing here is
//! re-exported: splitting the file changes where a method lives, not how it
//! is called.

mod arithmetic;
mod comparison;
mod construction;
mod conversions;
mod date_time;
mod formatting;
mod now_and_zone;
mod with;
