// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.ZonedDateTime`'s JavaScript-visible adapters.
//!
//! `ZonedDateTime` composes `PlainDateTime` + `TimeZone` + `Instant`: its
//! stored ISO fields are always the *local* wall-clock fields its
//! `epoch_nanoseconds` resolves to in its own `time_zone`
//! (`resolution::temporal_set_local_fields`), so every calendar-field and
//! time-of-day getter, and every `plain_date`/`calendar` helper already there
//! for `PlainDate`/`PlainDateTime`, applies to a `ZonedDateTime` receiver once
//! its local fields are correct. Each child holds one cohesive group of
//! `impl Vm` methods; the host-neutral arithmetic they drive is in
//! `zoned_date_time.rs` and `zoned_difference.rs`.

mod arithmetic;
mod conversions;
mod formatting;
mod from_value;
mod resolution;
mod since_until;
mod with;

// The helpers other `vm/temporal` modules call. The facade keeps the
// pre-split `zoned::*` paths stable for all of them.
pub(super) use self::formatting::format_offset_nanoseconds_exact;
pub(super) use self::resolution::{
    temporal_interpret_offset, temporal_resolution_error, temporal_set_local_fields,
    temporal_zoned_date_time_zone,
};
