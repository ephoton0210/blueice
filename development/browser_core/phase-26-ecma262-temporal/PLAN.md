# Phase 26 — ECMA-262 Temporal

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Design, Stage 0 done — first version written 2026-09-17; Stage 0
(shared foundation) completed 2026-09-18, see its own checklist below for
exactly what that means and what remains open within it (a full spec-grammar
audit beyond Test262's coverage, and Track E's open TimeZone/`icu_time`
question). **Stage 1 is in progress**: Track C (`Instant`, 904/968 after its
same-day gap-closure pass) and Track D (`PlainTime`, 976/1,010) are done as
of 2026-09-18 — see each track's own bullet for its measured Test262 numbers
and its remaining gaps, and the closure table in Stage 3 for the per-type
picture. Tracks A (calendars), B (`Duration`) and E
(`TimeZone`) are still open, as is `Temporal.Now` (grouped under Track C but
a separate slice that track did not reach). It exists because
completing Phase 25 (ECMA-402) surfaced a real gap in `intl402/`'s
`Temporal/` subtree. **Correction (2026-09-17, same day):** the plan's first
version only measured `intl402/Temporal/` (4,058 modes, 6.55% pass) — see
[Phase 25's `CONFORMANCE.md`](../phase-25-ecma402-internationalization/CONFORMANCE.md#reproducible-current-inventory).
Test262 also has a **separate, larger `built-ins/Temporal/` tree** (4,605
files, 9,210 modes) not counted there, since it is correctly out of scope for
an *ECMA-402* denominator — but it is very much in scope for *this* phase,
since it is ECMA-262 Temporal's actual primary test surface. The true
combined Temporal denominator this phase is accountable to is
**13,268 modes, currently 1,592 passing (12.00%)**, not the 4,058/6.55%
figure the first version of this document cited. Per-type combined totals
(`built-ins/` + `intl402/`, 2026-09-17):

| Type | Combined modes | Combined pass | Rate |
| --- | ---: | ---: | ---: |
| `ZonedDateTime` | 2,968 | 186 | 6.27% |
| `PlainDateTime` | 2,512 | 302 | 12.02% |
| `PlainDate` | 2,290 | 332 | 14.50% |
| `Duration` | 1,122 | 232 | 20.68% |
| `PlainYearMonth` | 1,672 | 186 | 11.12% |
| `PlainTime` | 1,010 | 102 | 10.10% |
| `Instant` | 968 | 86 | 8.88% |
| `PlainMonthDay` | 578 | 158 | 27.34% |
| `Now` | 138 | 0 | 0% |
| **Total** | **13,268** | **1,592** | **12.00%** |

(Reproduce with `python backend/bluejs/test262/run.py --filter "built-ins/Temporal/"`
and the existing `--filter "intl402/Temporal/"` run, grouped by
`path.split("/")[2]` each.) Temporal is **ECMA-262** (a core language
built-in, like `Date`), not an ECMA-402 service, so it does not belong inside
`blueice-ecma402`; it is scoped here as its own phase, owned by BlueJS
(Phase 13), the same way Gecko implements it inside SpiderMonkey rather than
inside its `Intl` internationalization layer.

This document's structure is organized around its own central design
decision: **which parts of Temporal can be built in parallel, and which
cannot.** That split is not a guess — it is read directly off SpiderMonkey's
real, shipping Temporal implementation (`development/browser_core/reference/gecko/js/src/builtin/temporal/`,
33,679 lines across 32 files), per this project's standing convention of
reading Gecko/Blink source as technical reference and porting basis, not
clean-rooming.

## Starting point: this is not a from-zero build

A 2026-09-17 repo audit found a real, partial implementation already exists.
Do not re-derive or duplicate any of this:

- `backend/bluejs/src/vm/temporal.rs` (1,286 lines) — `TemporalCalendarFields`
  and 8 `Vm` methods (`temporal_global`, `temporal_value_from_string`,
  `temporal_constructor`, `temporal_from`, `temporal_with_calendar`,
  `temporal_getter`, `temporal_plain_to_zoned_date_time`,
  `temporal_zoned_date_time_to_locale_string`), dispatched through
  `backend/bluejs/src/native.rs:150-155`
  (`NativeFunction::Temporal{Constructor,From,WithCalendar,PlainToZonedDateTime,Getter,ZonedDateTimeToLocaleString}`)
  and `backend/bluejs/src/vm/builtins/native_dispatch.rs:985-996`.
- `backend/bluejs/src/heap.rs:541-550` already has a `TemporalKind` enum with
  8 variants (`Duration`, `Instant`, `PlainDate`, `PlainDateTime`,
  `PlainMonthDay`, `PlainTime`, `PlainYearMonth`, `ZonedDateTime`) using the
  same `ObjectKind`-variant-with-`Rc<data>`-payload pattern already
  established for the Intl services (`Collator{...}`, `NumberFormat{...}`,
  etc.) and for `TypedArrayKind` (heap.rs:1070). **No new GC mechanism is
  needed** — this is a template to extend, not a design question.
- Non-ISO calendar math (lunisolar/Chinese/Coptic/etc.) already goes through
  ICU4X's `icu_calendar` crate directly on the BlueJS side — it is not a
  bespoke bridge type, and `icu_calendar` is already a proven, tested
  dependency in this codebase.
- `backend/ecma402/src/date_time_format.rs:628-647`'s `DateTimeFormatInput`
  (`TemporalInstant`/`TemporalPlain` variants) is a one-way, read-only bridge
  that collapses a Temporal-like value to a local epoch-millisecond integer
  purely so `Intl.DateTimeFormat` can format it. It is **not** a
  calendar-field record and is not a foundation for real Temporal — Temporal
  values keep their own calendar-field state; this bridge only ever sees the
  already-resolved output.
- Existing regression surface that must not break:
  `backend/bluejs/tests/intl.rs:309`
  (`temporal_calendar_fields_round_trip_through_iso_and_lunisolar_months`),
  `:445` (`temporal_datetime_format_uses_one_typed_bridge_for_values_and_ranges`),
  plus the inline-assertion-string arrays at lines 265-276, 294-295, 436, 631.
  Zero existing Temporal tests exist at any host-neutral crate boundary —
  today's entire Temporal surface is BlueJS-internal.

**What is missing** (and is the actual scope of this phase): every
arithmetic/comparison/serialization operation — `add`, `subtract`, `until`,
`since`, `compare`, `round`, `equals`, `toString`, `toJSON`, `negated`, `abs`,
`total` — on every type; `Temporal.Now`; `Temporal.TimeZone` as a real object.
Today's slice is read-only construction plus one-way `Intl.DateTimeFormat`
formatting. This matches the 6.55% Test262 pass rate exactly: the passes that
already occur are essentially all construction/getter/formatting cases.

An unresolved item from the same audit: `Intl.DurationFormat().format()`
already accepts ISO 8601 duration strings (`intl.rs:631`), but no ISO 8601
duration parser was found in `backend/ecma402/src/duration.rs` or anywhere
else searched. **Locate this before Stage 0 below** — it may already be
reusable, or it may reveal the existing DurationFormat duration handling
takes a different, non-parseable input path than assumed.

## Architecture

Temporal lives inside `backend/bluejs`, not as a new top-level crate.
Rationale (departs from Phase 25's `blueice-ecma402` pattern deliberately):
Phase 25 pulled ECMA-402 into its own crate because Intl algorithms have a
real second consumer (a future non-JavaScript host) and because keeping them
out of BlueJS decouples Intl completeness from VM work. Neither reason holds
for Temporal — it is a language built-in with no non-JS-host use case, and
Gecko itself keeps it inside the JS engine, not beside `Intl`. The existing
`vm/temporal.rs` already made this call; this phase continues it rather than
re-litigating it.

Within `backend/bluejs`, split `vm/temporal.rs` into a `vm/temporal/` module
directory mirroring Gecko's own foundation/per-type split (and this
project's own precedent: Phase 25's `source modularity audit`, and
`vm/builtins.rs`'s existing promise/typed-array submodule extractions):

```text
vm/temporal/
  iso.rs           — ISODate{year,month,day}, Time{hour..nanosecond}, ISODateTime;
                      ISO 8601 grammar parser (dates, times, datetimes, durations,
                      instant/zoned strings with calendar annotations)
  epoch.rs          — epoch-nanosecond representation (Rust i128 covers Gecko's
                      bespoke 96-bit Int96 natively — no custom bigint type needed)
  duration_math.rs  — InternalDuration/TimeDuration/DateDuration (internal,
                      non-JS-visible combinators) + balancing/rounding algorithms
  rounding.rs        — TemporalRoundingMode, TemporalUnit enums and tables
  calendar.rs        — CalendarId closed enum (16 IDs, matching Gecko/CLDR) +
                      calendar-field<->ISO dispatch per ID, wired to icu_calendar
  time_zone.rs       — fixed-offset and IANA-identifier resolution/transitions
  plain_date.rs, plain_time.rs, plain_date_time.rs, plain_year_month.rs,
  plain_month_day.rs, zoned_date_time.rs, duration.rs (JS-visible wrapper,
  distinct from duration_math.rs), instant.rs, now.rs
                      — JS-visible object adapters: Value/heap/Realm coupling,
                      constructor/prototype/method wiring, GC tracing
```

`iso.rs`, `epoch.rs`, `duration_math.rs`, `rounding.rs` and `calendar.rs`'s
per-calendar conversion functions are deliberately written with **no
`Value`/heap/Realm coupling** — plain Rust structs and functions, directly
unit-testable without a VM, mirroring the host-neutral/adapter split Phase 25
established, just as internal modules of one crate rather than a second
crate (see rationale above for why not a second crate). This is what makes
Stage 1 below actually parallelizable: each track owns files with a clean,
narrow dependency edge onto this foundation and no dependency on the other
Stage 1 tracks' files.

`Calendar` and `TimeZone` are **not** general object protocols. Confirmed
directly from Gecko's `Calendar.h`: `CalendarId` is a closed 16-value enum
(`ISO8601`, `Buddhist`, `Chinese`, `Coptic`, `Dangi`, `Ethiopian`,
`EthiopianAmeteAlem`, `Gregorian`, `Hebrew`, `Indian`, `IslamicCivil`,
`IslamicTabular`, `IslamicUmmAlQura`, `Japanese`, `Persian`, `ROC`) — the
current Temporal spec revision dropped the earlier arbitrary-object-calendar
design. `TimeZone` still needs more machinery (a fixed UTC-offset string vs.
a named IANA identifier with real transition-rule lookups), but is likewise
not user-pluggable. Neither needs a new `heap.rs` `TemporalKind` variant.

## Parallel development plan

This is the organizing structure of this phase's delivery order, not an
afterthought bolted onto it. Every stage below states which files it owns, so
a real worktree-isolated parallel agent per track has an unambiguous,
non-overlapping file boundary — this is a hard requirement, not a suggestion:
this session already saw first-hand what happens when two agents share one
working directory and edit concurrently (a background fork's edits and the
primary session's edits interleaved in the same files with no isolation).
Every parallel agent in Stage 1 and Stage 3 below **must** use
`isolation: "worktree"`.

### Stage 0 — shared foundation (single owner, sequential, blocking)

Gecko's own source is the evidence this cannot be parallelized: every
per-type `.cpp` file (`PlainDate.cpp`, `PlainDateTime.cpp`,
`PlainYearMonth.cpp`, `PlainMonthDay.cpp`, `ZonedDateTime.cpp`) directly
includes nearly every other type's header plus `Calendar.h`,
`CalendarFields.h`, `Duration.h`, `TemporalParser.h`,
`TemporalRoundingMode.h`, `TemporalTypes.h`, `TimeZone.h` and `ToString.h`.
Nothing downstream is stable until this lands. Scope:

- [x] **`iso.rs`/`epoch.rs`/`calendar.rs` module split — closed 2026-09-18.**
      `backend/bluejs/src/vm/temporal.rs`'s pure ISO 8601/epoch/calendar-id
      parsing functions are extracted into `backend/bluejs/src/vm/temporal/`
      as this document's Architecture section describes: `iso.rs`
      (`parse_date`, `parse_time`, `parse_annotations`,
      `parse_duration_record`, `parse_offset_seconds`), `epoch.rs`
      (`nanoseconds_since_epoch`, `is_in_instant_range`) and `calendar.rs`
      (`calendar_kind`). Each now has its own focused Rust unit tests
      (no VM required), on top of the existing `vm/temporal.rs`-level
      regression tests. Pure, behavior-preserving refactor: `cargo test -p
      blueice-bluejs` (all binaries, `--no-fail-fast`) is unchanged apart
      from two pre-existing, unrelated failures (`descriptors.rs`'s
      `define_properties_coerces_array_length_after_collecting_descriptors`
      and `string_protocols.rs`'s
      `array_length_descriptors_coerce_once_and_reject_invalid_lengths`/
      `capture_identity_and_primitive_protocol_lookup`) — confirmed
      pre-existing by reproducing them in a worktree at this session's
      original starting commit, before any Phase 25/26 work began.
- [x] **`epoch.rs`'s representation — resolved 2026-09-18, corrected from
      this document's own earlier assumption.** This document originally
      proposed `i128` over `BigInt` on the theory that Rust's native
      128-bit integer is simpler than Gecko's bespoke `Int96`. Checking the
      actual code first: `crate::heap::TemporalValue::epoch_nanoseconds`
      (the JS-visible `Temporal.Instant`/`ZonedDateTime` epoch field) is
      already `BigInt` throughout this engine, read with ordinary `BigInt`
      arithmetic (e.g. `epochMilliseconds`'s division) and backing BlueJS's
      native JS `BigInt` support. Switching to `i128` would add conversion
      friction at every read, not remove any — `epoch.rs` keeps `BigInt`.
- [x] **`TemporalUnit`/`TemporalRoundingMode` vocabulary and the
      calendar-agnostic `TimeDuration` combinator — design settled
      2026-09-18, deliberately not landed as `rounding.rs`/`duration_math.rs`
      files yet.** A `#[allow(dead_code)]` search across `backend/bluejs`
      and `backend/ecma402` finds zero precedent anywhere in this codebase
      for landing code with no real caller; `iso`/`epoch`/`calendar` above
      are justified as Stage 0 deliverables specifically because they
      extract already-called, already-tested code, which this is not.
      Recorded design, for Stage 1/2's first real arithmetic method to
      implement via TDD from that call site:
      - Temporal reuses `Intl.NumberFormat`'s exact nine-mode
        `roundingMode` vocabulary (a deliberate shared TC39 design) —
        already implemented as `blueice_ecma402::NumberRoundingMode`
        (`HalfExpand` (default), `Floor`, `Ceil`, `Expand`, `Trunc`,
        `HalfCeil`, `HalfFloor`, `HalfTrunc`, `HalfEven`). Reuse it
        directly; do not redefine it.
      - `TemporalUnit`: `Year`/`Month`/`Week`/`Day`/`Hour`/`Minute`/
        `Second`/`Millisecond`/`Microsecond`/`Nanosecond`. Both singular
        and plural option spellings are accepted and equivalent — verified
        against Test262's
        `built-ins/Temporal/Duration/prototype/round/singular-units.js`,
        not assumed.
      - `TimeDuration` (calendar-agnostic; mirrors Gecko's own
        `TimeDuration`/`DateDuration` split in `TemporalTypes.h` — only the
        latter needs calendar-aware balancing against a `relativeTo`, which
        stays out of scope until Stage 1 Track A's `calendar.rs` exists):
        hold the exact total as `i128` nanoseconds (safely covers even the
        largest bounded Duration Record field converted to nanoseconds);
        `balance_days()` extracts `(days, hours, minutes, seconds,
        milliseconds, microseconds, nanoseconds)` via sign-consistent
        truncating division at each step, matching `BalanceTimeDuration`.
- [x] Locate the ISO 8601 duration parser `Intl.DurationFormat` depends on —
      **resolved 2026-09-17**: it already exists, as
      `temporal_duration_record` in `backend/bluejs/src/vm/temporal.rs`
      (called from `intl.rs`'s `duration_record` via
      `temporal_value_from_string`). **Correction (2026-09-18, Track D):**
      this item originally recorded that restricting the fraction to the
      seconds component "correctly" matches Temporal's grammar. It does not —
      Temporal has `DurationHoursFraction` and `DurationMinutesFraction` as
      well, so `"PT1.03125H"` is valid and today throws
      (`built-ins/Temporal/PlainTime/prototype/add/argument-string-fractional-units-rounding-mode.js`).
      It *is* narrower than general ISO 8601 in that a fraction may only sit
      on the last present component, but seconds are not the only component
      that may carry one. Left for Track B, which owns `Duration`.
- [x] **Calendar-annotation parsing in ISO strings — closed 2026-09-17.**
      `temporal_value_from_string` previously hardcoded every parsed value's
      calendar to `"iso8601"` regardless of any `[u-ca=...]` annotation in
      the source string — a real, silent bug (confirmed no existing test
      covered this; `temporal_calendar_fields_round_trip_...` only exercises
      calendar via constructor argument/property-bag form, never a string
      annotation). Added `temporal_annotations` (TDD, `backend/bluejs/tests/intl.rs`'s
      `temporal_string_calendar_and_unknown_annotations_follow_the_grammar`,
      cases taken directly from Test262's
      `built-ins/Temporal/PlainDate/from/argument-string-calendar-annotation*.js`
      fixtures): first `u-ca=` annotation wins (later ones ignored,
      unvalidated); an uppercase annotation key is always a syntax error
      regardless of the critical flag; any other unrecognized key is ignored
      unless critical (`!`), in which case it throws; an unrecognized
      calendar ID throws. A leading non-`key=value` bracket (a time-zone
      annotation, e.g. `[UTC]`) is skipped without validation — that
      remains Track E's scope, not this item's.
- [x] **Found and fixed a real, unrelated latent bug while adding the above
      — closed 2026-09-17.** Extracting the time-of-day portion used
      `source.split_once(['T', 't'])` globally across the *entire* input
      string. `"UTC"` itself contains a `'T'`, so a date-only string with a
      leading time-zone annotation and no actual time component (e.g.
      `"2000-05-02[UTC][u-ca=hebrew]"`) mis-split inside the annotation
      bracket and failed with a spurious "invalid Temporal time string".
      Fixed by bounding the search to the character immediately following
      the date portion (mirroring `temporal_date`'s own boundary
      computation) instead of a global search. This was reachable before
      this session's new annotation test exercised the combination; no
      previously-existing test caught it.
- [x] `heap.rs`: confirmed 2026-09-18 — `TemporalKind` already has the
      8 variants Stage 0 and Stage 2's calendar-aware types need
      (`Duration`, `Instant`, `PlainDate`, `PlainDateTime`,
      `PlainMonthDay`, `PlainTime`, `PlainYearMonth`, `ZonedDateTime`); no
      Stage 0 record needs direct heap representation of its own. Whether
      `TimeZone` needs its own variant remains Track E's open question
      (see below), not a Stage 0 blocker.
- [x] Full ISO 8601 grammar coverage: spot-checked rather than exhaustively
      audited, given the size of the full grammar. Confirmed working: the
      six-digit signed extended-year form (`+002020-06-01`, exercised by an
      existing DateTimeFormat test).

      **Correction (2026-09-18, Track D): this item's conclusion that the
      basic (separator-less) format is "not required" was wrong**, and the
      method that produced it — searching for a fixture that requires it —
      is what failed: `built-ins/Temporal/PlainTime/from/argument-string.js`
      requires `152330`, `19761118T15:23:30.1+00:00`,
      `+0019761118T152330.1+0000` and `T0030`, and
      `PlainTime/from/argument-string-with-time-designator.js` requires
      `T003000.000000000`. Basic-format dates and times are now both
      supported (`iso::parse_date`/`parse_time_spec`), along with `,` as the
      decimal separator. Track D also found, by rewriting the parser against
      `from/argument-string-invalid.js` rather than by audit, that this
      parser was accepting a *superset* of the grammar in several places —
      inconsistent separators, arbitrary field widths, over-long fractions, a
      negative-zero extended year — each of which is listed under Track D
      below. The full line-by-line audit against the spec text (as opposed to
      against Test262) still remains open, and this item is evidence it is
      worth doing: every one of those defects was reachable and none was
      caught by "spot-checking".

### Stage 1 — parallel tracks (worktree-isolated agents, after Stage 0 lands)

Each track's Gecko evidence for independence is stated explicitly so this
isn't an assumption:

- **Track A — Calendar systems: corrected 2026-09-18, folded into Stage 2
  rather than a standalone Stage 1 track.** The pinned Test262 revision has
  **no `built-ins/Temporal/Calendar/` directory at all** (confirmed by
  direct search) — consistent with the Gecko finding above that the current
  spec dropped the object-protocol calendar design for a closed identifier
  set. `calendar.rs`'s recognition table and `temporal_calendar_fields`'s
  ISO↔any-calendar conversion (via `icu_calendar`) already exist from Stage
  0 and already pass real Test262 evidence
  (`temporal_calendar_fields_round_trip_through_iso_and_lunisolar_months`).
  There is no freestanding Stage 1 Test262 surface left to drive a separate
  track against — Calendar's remaining work (deeper per-calendar edge
  cases, era/monthCode handling for less-common calendars) only has real
  test coverage through `PlainDate`/`PlainDateTime`/etc., which are Stage
  2's calendar-aware composite types. Do not dispatch a standalone "Track
  A" agent; calendar correctness gets exercised as part of Stage 2 instead.
- **Track B — Duration arithmetic** (`duration.rs`, the JS-visible wrapper;
  building on Stage 0's `duration_math.rs`). Evidence: Gecko's core
  add/subtract/negate/abs/compare path does not depend on `Calendar.cpp`
  except for calendar-aware rounding against an optional `relativeTo` —
  calendar-independent arithmetic can be built and tested (Test262's
  combined `Duration/`, 1,122 modes across both trees) before Track A
  finishes.
- **Track C — Instant + Now.** Evidence: epoch nanoseconds are
  calendar-agnostic by construction; Gecko's `Instant.cpp` has no calendar
  dependency. **`Instant` arithmetic done 2026-09-18** (kept inside
  `vm/temporal.rs` rather than a new `instant.rs` file — the method bodies
  are adapter-layer `impl Vm` code coupled to `Value`/heap, matching the
  existing `temporal_getter`/`temporal_with_calendar` style, not
  foundation code; only `iso`/`epoch`/`calendar`/`rounding`/`duration_math`
  are the host-neutral split). `add`/`subtract`/`round`/`until`/`since`/
  `equals`/`compare`/`toString`/`toJSON`/`valueOf`/`fromEpochMilliseconds`/
  `fromEpochNanoseconds` plus the previously-missing `epochMilliseconds`/
  `epochNanoseconds` getters are implemented and TDD-verified against the
  real pinned Test262 corpus: `Temporal/Instant/` went from 86/968 (8.88%,
  Stage 0's read-only-construction baseline) to **646/968 (66.7%)**.
  Remaining known gaps, not yet closed: `toZonedDateTimeISO` (0/38, not
  implemented — needs Track E's `TimeZone` first), some `toString`/`round`
  edge cases, and `Temporal.Now` itself (still entirely unimplemented,
  0/138 — a real, separate slice from `Instant`'s own arithmetic, despite
  being grouped in the same track). `duration_math.rs`/`rounding.rs` were
  finally landed as real files here (not speculative — Instant's arithmetic
  is their first real caller): `TimeDuration` (exact-nanosecond combinator,
  `round`/`balance_to`), `TimeUnit` + `parse_time_unit`, `round_to_increment`
  (verified against Test262's exact expected values in
  `rounding-increments.js`/`round-to-days.js`, not just self-consistency),
  and reuse of `blueice_ecma402::NumberRoundingMode` rather than a
  redefinition. `since(a,b)`'s rounding-mode semantics were confirmed via
  Test262 (`since/roundingmode-ceil.js`) to be a literal signed-difference
  computation with the given mode applied as-is — no `NegateRoundingMode`
  step needed, contrary to an initial assumption.

  **Gap-closure pass, 2026-09-18 (same day, after Track D merged).**
  `Temporal/Instant/` went from **710/968 (73.3%)** — the post-Track-D
  baseline, Track C's original 646 plus the 64 modes Track D's shared Stage 0
  parser fixes carried — to **904/968 (93.4%)**, with **zero regressions
  anywhere in `Temporal/`** (the whole-tree filter went 3,168 → 3,380 of
  13,272, +212 fixed / 0 regressed, diffed per path+mode). Per bucket:
  `toString` 52→110/114, `toLocaleString` 12→40/42, `round` 64→82/82,
  `equals` 48→60/60, `since` 116→136/142, `until` 114→134/140, `add`
  48→52/56, `subtract` 46→50/54, `epochMilliseconds` 4→6/6, and every
  `from`/`compare` string-argument file (24 modes) from 0.

  What was actually wrong, each item pinned to the fixture that proves it:

  - **Instant strings had no grammar of their own.** They went through the
    generic date/time path, so a mandatory time and offset were not enforced,
    `Z`-less and space-separated forms, leap seconds, sub-minute (indeed
    nanosecond-precision) offsets, basic-format dates and ignorable `u-ca`
    annotations were all mishandled, and trailing junk was accepted.
    `iso::parse_instant` is now a complete `TemporalInstantString` parser
    built from prefix parsers (`parse_iso_date_prefix`/`parse_iso_time_prefix`/
    `parse_utc_offset_prefix`) that Track D's `parse_date`/`parse_time`/
    `parse_offset_seconds` now delegate to, so there is one grammar
    implementation rather than two.
  - **Rounding used the wrong algorithm for negative instants.**
    `RoundTemporalInstant` is defined over
    `RoundNumberToIncrementAsIfPositive`, not `RoundNumberToIncrement`: for a
    pre-epoch instant, `trunc` must move *earlier* and `ceil` *later*,
    independent of sign. Added `rounding::round_to_increment_as_if_positive`
    (kept beside, not replacing, the ordinary magnitude-based one that
    `PlainTime`/`Duration` need) — `round/negative-instant.js`,
    `toString/rounding-direction.js`, `toString/negative-instant-rounding.js`.
  - **`toString` ignored three of its own options.** `smallestUnit: "minute"`
    must drop the seconds field entirely; `fractionalSecondDigits: n` implies
    a rounding *increment* of `10^(3-n)`/`10^(6-n)`/`10^(9-n)`, not 1
    (`rounding-cross-midnight.js`); and the `timeZone` option was not
    implemented at all. It now prints local fields plus a `±HH:MM` offset.
  - **`toLocaleString` was an alias for `toString`.** It is
    `CreateDateTimeFormat(locales, options, ANY, ALL)` + `FormatDateTime`, so
    it now goes through the same `Intl.DateTimeFormat` bridge
    `temporal_zoned_date_time_to_locale_string` uses — minus that method's
    forced `timeZone`, which an `Instant` does not carry.
  - **Option reading was neither strict nor ordered.** `GetOptionsObject` now
    rejects primitives instead of `ToObject`-boxing them; `round` accepts a
    String `roundTo` shorthand on a null-prototype object; and every method
    reads *all* its options, in alphabetical order, before validating any of
    them — `GetTemporalUnitValuedOption` accepts any unit *name* (including
    calendar units) and the unit-group check happens afterwards, which is
    exactly what the `order-of-operations.js` and
    `options-read-before-algorithmic-validation.js` fixtures observe.
  - **`until`/`since` had two rule bugs.** `largestUnit` defaults to
    `LargerOfTwoTemporalUnits("second", smallestUnit)`, not to `"second"`
    flatly (`largestunit-default.js`), and their rounding increment must
    divide the *next larger unit* and stay strictly below it
    (`invalid-increments.js`) — a different rule from
    `Instant.prototype.round`'s divide-a-whole-day one.
  - **Smaller fixes:** the constructor takes `ToBigInt` (so a numeric string
    and a Boolean work, and a Number is a `TypeError`) rather than requiring
    a literal `BigInt` (`basic.js`); `epochMilliseconds` floors rather than
    truncating toward zero, so a pre-epoch instant rounds down
    (`epochMilliseconds/basic.js`); `ToTemporalInstant` has a `ZonedDateTime`
    fast path and throws `TypeError` for a non-String primitive rather than
    stringifying it (`argument-zoneddatetime.js`, `argument-wrong-type.js`);
    and `iso::parse_duration_record` now supports a fraction on the last
    present time unit, cascading exactly into the units below it, which is
    what `Instant.prototype.add("PT1.03125H")` needs.

  **The 64 still-failing modes, and why** (none of them are Instant
  arithmetic itself):

  - **38 — `toZonedDateTimeISO` (2/40).** Not implemented; needs Track E's
    `TimeZone`. Deliberately untouched.
  - **20 — blocked on Track B (`Temporal.Duration`).** `add-large-subseconds`,
    `subtract-large-subseconds` and `minimum-maximum-instant` need
    `Temporal.Duration.from` with a property bag; `until`/`since`'s
    `add-subtract`, `argument-zoneddatetime` and
    `float64-representable-integer` need `Duration.prototype.negated`/`total`/
    `add`, `Duration.prototype.toString` and `Duration.compare`. The
    `Instant` side of each of these already works.
  - **4 — `intl402` `toString/timezone-offset.js` and
    `timezone-string-datetime.js`.** The only genuinely timeZone-dependent
    deferral: they format against `Europe/Berlin`, `America/New_York` and
    `Africa/Monrovia`, which needs real IANA transition data at an arbitrary
    instant. `iso::resolve_fixed_time_zone_offset` therefore resolves `UTC`
    and fixed offsets and returns "unresolvable" for every named zone, which
    the caller turns into a `RangeError`. That is also, coincidentally, what
    makes `timezone-string-unknown.js` pass, so those two files are the exact
    measure of what Track E's data would add here. Note `backend/ecma402`
    already depends on `jiff`/`jiff_tzdb` with real transition data
    (`to_offset_info`), so Track E's open question has a ready answer — it was
    simply out of scope to wire a new dependency edge from here.
  - **2 — `intl402` `toLocaleString/hourcycle.js`.** Pre-existing
    `Intl.DateTimeFormat` gap (`hourCycle: "h24"`/`"h11"`), not an `Instant`
    one: the fixture's own `Intl.DateTimeFormat` equivalent fails the same
    way, and this implementation is verified against
    `new Intl.DateTimeFormat(locales, options).format(instant)` directly.

  One pre-existing foundation test's expectation was corrected, not weakened:
  `iso.rs`'s `parses_duration_strings_with_the_seconds_only_fraction_rule`
  asserted `parse_duration_record("P1DT2H30.5M").is_none()`. That is wrong —
  Temporal's `DurationTime` grammar allows a fraction on the *last present*
  unit, and Test262's
  `Instant/prototype/add/argument-string-negative-fractional-units.js` adds
  `"-PT1440.567890123M"` to an `Instant`. It is now
  `parses_duration_strings_with_a_fraction_on_the_last_unit_only`, still
  pinning the "not on a non-final unit" half of the rule.
- **Track D — PlainTime.** Evidence: time-of-day has no calendar-field
  dependency; Gecko's `PlainTime.cpp` (1,644 lines) is the smallest of the
  calendar-adjacent per-type files. **Done 2026-09-18** — kept inside
  `vm/temporal.rs` for the same reason Track C's `Instant` was (the method
  bodies are `Value`/heap-coupled adapter code, not host-neutral foundation),
  so no `plain_time.rs` file exists. `Temporal/PlainTime/` went from
  **108/1,010 (10.7%)** — Stage 0's read-only-construction baseline, itself
  slightly above this document's 2026-09-17 figure of 102 — to
  **968/1,010 (95.8%)**. Implemented: the six field getters
  (`hour`…`nanosecond`), `add`/`subtract`, `round`, `until`/`since`,
  `equals`, static `compare`, `with`, `toString`/`toJSON`/`toLocaleString`/
  `valueOf`, plus a real `ToTemporalTime` behind `from`/`until`/`since`/
  `equals`/`compare` (PlainTime, PlainDateTime, fixed-offset/UTC
  ZonedDateTime, property bag with `overflow`, and string).
  Semantics Test262 settled, each against a named fixture rather than from
  memory:
  - **Wrapping, not overflow.** `PlainTime` arithmetic wraps at the 24-hour
    boundary in both directions (`rem_euclid` over `NANOSECONDS_PER_DAY`, new
    in `duration_math.rs` as `time_fields_to_nanoseconds`/
    `time_fields_from_nanoseconds`) — `add/balance-negative-time-units.js`
    and `round/rounding-cross-midnight.js`, where rounding
    `23:59:59.999999999` up lands on `00:00:00`, not an out-of-range `24:00`.
  - **Calendar units are ignored, including `days` — not rejected.** The
    plan's own working assumption (and `Instant.prototype.add`'s behavior)
    was wrong here: `add/argument-higher-units.js` requires
    `plainTime.add({ days: 1 })` to be the *same* time and not to throw,
    because the spec's `ToInternalDurationRecord` leaves years/months/weeks/
    days in the date part `AddTime` never reads. A `days` field does **not**
    contribute 24 hours.
  - **`round`'s increment rule differs from `Instant`'s.** `Instant` needs
    the increment to divide a whole *day* (inclusive); `PlainTime` needs it to
    divide the *unit's own* place value (24/60/60/1000/1000/1000) and stay
    strictly below it, so `{ smallestUnit: "hours", roundingIncrement: 24 }`
    and `{ smallestUnit: "nanoseconds", roundingIncrement: 1000 }` both throw
    (`round/roundingincrement-invalid.js`). `until`/`since` use the same rule,
    which `Instant`'s own difference methods do not.
  - `round`'s argument is **required**, and a bare string is shorthand for
    `{ smallestUnit }` via a *null-prototype* options object
    (`round/string-shorthand-no-object-prototype-pollution.js`).
  - `until`/`since` default `largestUnit` to `hour` (not `second` as
    `Instant` does) and accept `"auto"`. `since` again needs **no**
    rounding-mode negation, for the same algebraic reason Track C recorded.
  - **All options are read and coerced before any is validated**, in
    alphabetical order — `round/options-read-before-algorithmic-validation.js`
    reads `smallestUnit` and only then throws on the increment, so the
    existing combined read-and-validate `temporal_string_option` could not be
    reused where options participate in a joint check. Duration property bags
    are likewise read alphabetically (`add/order-of-operations.js`).

  Shared Stage 0 foundation bugs this track found and fixed (all
  spec-correct for every Temporal type, and each lifted the other types'
  Test262 numbers — see the closure table below):
  - `parse_time` accepted inconsistent separators (`00:0000`), fields of any
    width (`001Z`), fractions longer than nine digits, and fractions on the
    minute/hour field (`05:07.123`); it rejected the leap-second `:60` the
    grammar accepts and constrains to `:59`; and it supported neither the
    basic separator-less format (`152330`, `T0030`) nor `,` as the decimal
    separator. Rewritten onto one strict `parse_time_spec` shared with the
    offset parser.
  - `parse_date` accepted `-000000` as an extended year (a negative zero,
    which the grammar rejects), let a four-digit year borrow the six-digit
    form's width, and did not support the basic format (`19761118`,
    `+0019761118`). **Stage 0's own note above that basic date format is
    "not required" is wrong** — `PlainTime/from/argument-string.js` requires
    it.
  - `parse_offset_seconds` rejected a sub-minute fraction
    (`+00:00:00.000000000`) and accepted trailing junk (`+00:00junk`); it now
    shares `parse_time_spec`. The fraction's *value* is still discarded,
    which is invisible to `PlainTime` (it ignores the offset entirely) but
    would matter to a sub-second `Instant` offset — recorded as a known gap.
  - `parse_annotations` did not reject a repeated `u-ca` annotation when any
    copy carries the critical flag.
  - `GetOptionsObject` boxed a primitive into a wrapper object instead of
    throwing `TypeError`.
  - `temporal_duration_from_value` read its ten fields in declaration rather
    than alphabetical order, which is observable.

  **Not done**, and why:
  - The **42 remaining `PlainTime` modes are all outside this track.** 20 are
    `Temporal.Duration` gaps (Track B): `Duration.from({ ... })` with a
    property bag, and fractional `H`/`M` components in an ISO duration string
    — `iso::parse_duration_record` allows a fraction only on `S`, and this
    document's Stage 0 claim that that "correctly restricts fractional parts
    to seconds only" is **wrong**; Temporal's grammar has
    `DurationHoursFraction`/`DurationMinutesFraction` too
    (`add/argument-string-fractional-units-rounding-mode.js`). Left for
    Track B rather than edited across a track boundary. The other 22 are
    `intl402/.../toLocaleString/`, which needs real `Intl.DateTimeFormat`
    integration for a plain time (default field set, `dateStyle`/`timeStyle`
    conflict rejection) — an ECMA-402 boundary, not PlainTime arithmetic;
    `toLocaleString` currently returns the same ISO string as `toJSON`.
  - A named-IANA-zone `ZonedDateTime` argument still throws; only `UTC` and a
    fixed numeric offset resolve (Track E).
  - A UTC offset's sub-second fraction is validated but its value discarded
    (see the `parse_offset_seconds` note above) — invisible to `PlainTime`,
    a latent inaccuracy for `Instant`.

  (The ambiguity rules a bare, un-`T`-prefixed time string has to respect —
  `1214` is December 14th and therefore not a time, `0229` is February 29th
  and therefore not a time, `0230` is not a real date and therefore *is* a
  time — are implemented from
  `TemporalHelpers.ISO.plainTimeStringsAmbiguous()`/`plainTimeStringsUnambiguous()`
  rather than derived, since the distinction turns on real calendar validity.)
- **Track E — TimeZone** (`time_zone.rs`). Fixed-offset resolution is
  self-contained; named-IANA-identifier transition-rule lookup needs its own
  investigation first — **check whether `icu_time` (already a pinned
  dependency in `backend/ecma402/Cargo.toml`) provides real historical
  transition data before assuming it does**, since `Intl.DateTimeFormat`'s
  existing IANA zone handling may only need current-offset/display-name
  data, not the full transition history `getOffsetNanosecondsFor` requires
  at arbitrary points in time.

That is 5 tracks as directly evidenced; Track A may reasonably split into 2
(ISO-adjacent solar calendars vs. lunisolar/Islamic-era calendars) to reach
6 if that matches available parallel capacity — do not force a 6-way split
where Gecko's own boundaries suggest 5.

### Stage 2 — calendar-aware composite types (single owner / one coordinated agent, sequential, after Stage 1)

`plain_date.rs`, `plain_date_time.rs`, `plain_year_month.rs`,
`plain_month_day.rs`, `zoned_date_time.rs`. Evidence this must not be
parallelized: this is exactly the dense cross-inclusion Gecko's source
shows directly (`PlainDateTime.cpp` alone includes `PlainDate.h`,
`PlainMonthDay.h`, `PlainTime.h`, `PlainYearMonth.h`, `ZonedDateTime.h`, all
of `Calendar.h`/`CalendarFields.h`/`Duration.h`/`TemporalParser.h`/
`TemporalRoundingMode.h`/`TemporalTypes.h`/`TimeZone.h`/`ToString.h` at
once). One owner:

- [ ] `plain_date.rs`, `plain_date_time.rs` first (combined Test262:
      `PlainDate/` 2,290, `PlainDateTime/` 2,512 modes — the two largest
      non-`ZonedDateTime` types).
- [ ] `plain_year_month.rs`, `plain_month_day.rs` next, reusing the
      calendar-field pattern `plain_date.rs` establishes (combined Test262:
      `PlainYearMonth/` 1,672, `PlainMonthDay/` 578 modes).
- [ ] `zoned_date_time.rs` last — composes `PlainDateTime` + `TimeZone` +
      `Instant`, so it must come after all three are solid. Gecko's largest
      per-type file (`ZonedDateTime.cpp`, 3,180 lines) and Test262's largest
      combined group (2,968 modes) — expect this to be the longest-running
      single piece of work in the phase.

### Stage 3 — Test262-evidence closure and coverage

- [ ] Re-run both `intl402/Temporal/` and `built-ins/Temporal/` after each
      stage lands, tracked per-type against the corrected 2026-09-17 combined
      baseline in the table near the top of this document (`ZonedDateTime`
      186/2,968, `PlainDate` 332/2,290, `PlainDateTime` 302/2,512,
      `PlainYearMonth` 186/1,672, `PlainMonthDay` 158/578, `Duration`
      232/1,122, `PlainTime` 102/1,010, `Instant` 86/968, `Now` 0/138).

      Measured after Stage 1 Tracks C and D (`python3
      backend/bluejs/test262/run.py --filter "Temporal/" --jobs 8`,
      2026-09-18). Track D's shared Stage 0 parser fixes are why every type
      moved, not only the two with a track:

      | Type | 2026-09-17 | After Track C | After Track D | After Track C's gap-closure pass |
      | --- | ---: | ---: | ---: | ---: |
      | `Instant` | 86/968 | 646/968 | 710/968 | **904/968** |
      | `PlainTime` | 102/1,010 | 108/1,010 | 968/1,010 | **976/1,010** |
      | `PlainDate` | 332/2,290 | 332/2,290 | **348/2,290** | 348/2,290 |
      | `PlainDateTime` | 302/2,512 | 302/2,512 | **316/2,512** | 316/2,512 |
      | `PlainMonthDay` | 158/578 | 158/578 | **174/578** | 174/578 |
      | `PlainYearMonth` | 186/1,672 | 186/1,672 | **202/1,672** | 202/1,672 |
      | `ZonedDateTime` | 186/2,968 | 186/2,968 | 210/2,968 | **216/2,968** |
      | `Duration` | 232/1,122 | 232/1,122 | 232/1,122 | **236/1,122** |
      | `Now` | 0/138 | 0/138 | 0/138 | 0/138 |
      | **Total (`Temporal/`)** | 1,592 | — | 3,168/13,272 | **3,380/13,272** |

      The last column's non-`Instant` movement (18 modes) is the shared half
      of Track C's gap-closure pass: `iso.rs` now has one unified
      date/time/offset grammar rather than two, and
      `parse_duration_record` accepts a fraction on the last present unit,
      so `PlainTime`, `ZonedDateTime` and `Duration` string arguments moved
      too. Both runs were diffed per path+mode, not just by total: **212
      fixed, 0 regressed.**

      (The `Temporal/` filter schedules 13,272 modes, four more than the
      per-type table's 13,268 — the extra ones are the tree's own root-level
      files, e.g. `Temporal/prop-desc.js`, which no per-type group counts.)
- [ ] TDD throughout, per this repo's Definition of Done: a failing test
      before the implementation that makes it pass, not tests bolted on
      after.
- [ ] Add host-neutral Rust tests for Stage 0's foundation modules directly
      (no VM required) — today's zero host-neutral Temporal test coverage
      (everything lives only in `backend/bluejs/tests/intl.rs`) should not
      continue once `iso.rs`/`duration_math.rs`/`calendar.rs` exist as
      pure-Rust modules.
- [ ] This phase does not get its own `cargo llvm-cov` gate distinct from
      `blueice-bluejs`'s existing 88%-floor gate (`vm/temporal/` is part of
      that crate) — but each new module should individually be near-100%
      given TDD discipline, the same way `blueice-ecma402`'s per-service
      modules already are.

## Open questions to resolve before or during Stage 0

- The ISO 8601 duration parser question above (may already exist and be
  reusable, or may not exist at all).
- Whether `icu_time`'s bundled data actually includes full IANA transition
  history, or only current offsets — determines Track E's real scope.
- Whether `TimeZone` needs a new `TemporalKind`/`ObjectKind` heap variant
  (Gecko's `TimeZoneObject : NativeObject` suggests yes, unlike `Calendar`)
  — confirm during Track E rather than assuming either way here.

## Relationship to other phases

- **Phase 13 (BlueJS)**: this phase's actual home; `vm/temporal/` is a
  BlueJS module, and this phase's coverage rolls into Phase 13/BlueJS's
  existing `blueice-bluejs` gate, not a new one.
- **Phase 25 (ECMA-402)**: motivated this phase (the `intl402/Temporal/`
  gap), and the existing `DateTimeFormatInput::Temporal{Instant,Plain}`
  bridge in `blueice-ecma402` is a consumer of this phase's output, not a
  dependency this phase needs — that boundary does not change here.
- Shares the pinned Test262 corpus and `backend/bluejs/test262/run.py`
  runner with Phase 13/25; no separate Temporal-specific test harness is
  needed.

This document is the first version of this phase's plan, meant to be
refined as Stage 0's actual implementation surfaces design questions this
research pass could not — per this repository's design-first convention,
update it as the design evolves rather than treating it as a historical
record.
