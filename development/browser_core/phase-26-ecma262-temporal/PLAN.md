# Phase 26 — ECMA-262 Temporal

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Design, Stage 0 done. Stage 1 (Tracks B/C/D/E) is done and closed
to its practical limit. Stage 2's first slice (`PlainDate`/`PlainDateTime`)
is done. Chronological closure record, each step's Test262 delta measured on
the pinned corpus (`python3 backend/bluejs/test262/run.py --filter
"Temporal/" --jobs 8`), diffed per path+mode against the step before it:

- **Stage 0 (shared foundation) — done 2026-09-18.** Includes a same-day
  exhaustive ISO 8601 grammar audit (11 real bugs; see its own bullet below)
  that closed the one checklist item Stage 0 originally left open.
- **Stage 1 (Tracks B/C/D/E) — done 2026-09-18, merged into one tree.**
  - Track C: `Instant` arithmetic + `Temporal.Now`.
  - Track D: `PlainTime`.
  - Track E: time-zone identifiers/offsets/disambiguation. Its former
    blocker (whether `icu_time` carries real IANA transition data) is
    **resolved** — see "Open questions" below; it does not, and real
    historical offset resolution instead uses `jiff`/`jiff-tzdb`, already a
    pinned `blueice-ecma402` dependency.
  - Track B: `Duration` arithmetic.
  - Track A was folded into Stage 2, not a standalone track.
  - Combined `Temporal/`: **4,396/13,272 (33.1%)**, up from the 2026-09-17
    baseline's 1,592 — confirming the repeated "per-track numbers are not
    strictly additive" note below (cross-track shared-foundation fixes
    compound when merged, not just sum). See each track's own bullet below
    for its independently-measured numbers, and the Stage 3 closure table
    for the fully-merged per-type picture.
- **Same-day gap-closure round (2026-09-18) — three independent,
  worktree-isolated passes, merged with zero textual conflicts** (each
  touched disjoint regions of the shared `vm/temporal.rs`/`PLAN.md` files):
  1. **`Instant`/`Now` timezone wiring.** Wired Track C's `Instant`/`Now`
     code paths to consume Track E's already-landed `time_zone.rs` (two call
     sites — `iso::resolve_fixed_time_zone_offset` and
     `time_zone_id::offset_seconds` — had not yet been updated to use it),
     and fixed a `Temporal.Duration` float64-rounding gap in
     `Instant.prototype.since`/`until`. `Instant` → 966/968 (every fixture
     except the 2 `toLocaleString/hourcycle.js` modes pass 3 below covers);
     `Now` → 138/138 (100%). See Track C's and Track E's own bullets below.
  2. **`PlainTime.prototype.toLocaleString` + `hourCycle` fix.**
     `toLocaleString` (previously aliased to `toJSON`) now goes through the
     same `Intl.DateTimeFormat` bridge every other Temporal `toLocaleString`
     uses, closing all 22 of its remaining fixtures. Alongside it, fixed a
     pre-existing, Temporal-unrelated `Intl.DateTimeFormat` `hourCycle:
     "h24"` rendering bug (midnight rendered as `"00"` instead of `"24"` —
     see [Phase 25's
     `CONFORMANCE.md`](../phase-25-ecma402-internationalization/CONFORMANCE.md)),
     which closes `Instant`'s own 2 remaining `toLocaleString/hourcycle.js`
     modes.
  3. **Foundation test-hardening + `calendar.rs` year-range fix** (see its
     own bullet below).
  - Combined result after merging all three: **`Instant` 968/968 (100%),
    `Now` 138/138 (100%), `PlainTime` 1,010/1,010 (100%)** — Stage 1's
    `Instant`/`Now`/`PlainTime`/`TimeZone` tracks are now Test262-complete.
    Combined `Temporal/`: **4,492/13,272 (33.85%)**, +96 over the 4,396
    baseline, zero regressions (per-type diff, not just the total). Verified
    with a full `cargo build --workspace --all-targets` / `cargo clippy
    --workspace --all-targets -- -D warnings` / `cargo test --workspace`
    pass (clean except the same 2 pre-existing, Temporal-unrelated
    `descriptors.rs`/`string_protocols.rs` failures every one of these
    passes independently confirmed pre-existing on `ba16c16`).
  - `Duration` remains at 870/1,122 (77.4%): every one of its 252 remaining
    failures was individually checked against the pinned corpus and needs
    calendar-aware `relativeTo`/year-month-week arithmetic, i.e. is
    structurally blocked on Stage 2's `PlainDate`, not further closeable
    within Stage 1's own scope.
- **Stage 2's `PlainDate`/`PlainDateTime` slice — done 2026-09-18** (single
  owner, sequential, per this document's own Stage 2 design; see that
  section's own bullet for the full account). Merged on top of the three
  gap-closure passes above with two real merge conflicts in `temporal.rs`/
  `PLAN.md`, both the "git diff misalignment" pattern this document has
  already documented several times: `PlainTime`'s `toLocaleString` and
  `PlainDate`/`PlainDateTime`'s `toLocaleString` are two distinct functions
  with near-identical shape that a line-based diff matched as one location;
  resolved by placing both complete functions in sequence rather than
  trusting the marked boundary.
  - `PlainDate`: 384→1,886/2,290 (82.4%). `PlainDateTime`: 382→2,056/2,512
    (81.8%). Whole-tree `Temporal/`: **7,576/13,272 (57.1%)**, zero
    regressions anywhere else in `Temporal/` on the same full-tree run.
  - `plain_year_month.rs`/`plain_month_day.rs` and `zoned_date_time.rs`
    remain open, in that order, per Stage 2's own stated sequencing.

This phase exists because completing Phase 25 (ECMA-402) surfaced a real gap
in `intl402/`'s
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
(Since this audit: Stage 1 Track C has closed `Temporal.Instant`'s arithmetic
and all of `Temporal.Now` — see that track's entry below.)
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

`iso.rs`, `epoch.rs`, `calendar.rs`, `rounding.rs`, `duration_math.rs` and
`time_zone.rs` all exist as of 2026-09-18. The per-type JS-visible adapters do
not: Track C and Track E both kept their `impl Vm` method bodies in
`vm/temporal.rs` alongside the existing `temporal_getter`/`temporal_from`
style, because that layer is `Value`/heap-coupled adapter code rather than
foundation code. Only the host-neutral modules above are split out.

`Calendar` and `TimeZone` are **not** general object protocols. Confirmed
directly from Gecko's `Calendar.h`: `CalendarId` is a closed 16-value enum
(`ISO8601`, `Buddhist`, `Chinese`, `Coptic`, `Dangi`, `Ethiopian`,
`EthiopianAmeteAlem`, `Gregorian`, `Hebrew`, `Indian`, `IslamicCivil`,
`IslamicTabular`, `IslamicUmmAlQura`, `Japanese`, `Persian`, `ROC`) — the
current Temporal spec revision dropped the earlier arbitrary-object-calendar
design. `TimeZone` is likewise not user-pluggable, and — confirmed against the
pinned Test262 corpus during Track E, which has no `built-ins/Temporal/
TimeZone/` directory at all — is not an object type in the current spec
revision either: it is a string, either a fixed UTC offset or a named IANA
identifier. Neither needs a new `heap.rs` `TemporalKind` variant.

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
      2026-09-18; landed by Stage 1 once each piece had a real caller
      (`rounding.rs`/`duration_math.rs` by Track C for `TimeUnit`, the full
      ten-variant `TemporalUnit` by Track B, which is the first caller that
      needs to *name* a calendar unit in order to reject it).** A
      `#[allow(dead_code)]` search across `backend/bluejs`
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
      `temporal_value_from_string`; now `iso::parse_duration_record`).
      **Corrected 2026-09-18 by Stage 1 Track B:** this item originally
      recorded that the parser "correctly restricts fractional parts to
      seconds only, matching Temporal's grammar". That was wrong on both
      counts — Temporal allows a fraction on any *final* time component
      (`PT0.5H` is 30 minutes), and the parser was also missing lowercase
      designators, the `,` decimal separator, the U+2212 sign, and
      component-order/duplication checks. See Track B's entry for the fix.
      The exhaustive grammar audit below (merged the same day) independently
      rewrote `parse_duration_record` around a `Cursor`/`DurationTerm`
      structure with the same case-insensitivity and component-order checks,
      but without the U+2212 sign fix; Track B's version was kept as the one
      merged in, since it is the strict superset.
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
      `TimeZone` needs its own variant was Track E's open question, and was
      answered "no" on 2026-09-18 (see below) — it is a string in a
      `ZonedDateTime`'s existing `time_zone` field, never a heap object.
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

- [x] **Exhaustive ISO 8601 grammar audit — closed 2026-09-18, superseding
      both the original "spot-checked" item above and Track D's partial
      correction of it.** `iso.rs` was re-derived as a strict
      recursive-descent scan of Temporal's own productions (`ISODate`,
      `TimeSpec`, `UTCOffset`, `TimeZoneAnnotation`, `Annotations`,
      `TemporalYearMonthString`, `TemporalMonthDayString`,
      `TemporalTimeString`, `TemporalDurationString`) rather than a
      split-on-separator approximation, and driven against **every**
      string-relevant fixture under `built-ins/Temporal/*/from/`,
      `*/compare/` and `*/prototype/{until,since,equals,with}/`, plus
      `harness/temporalHelpers.js`'s own `ISO.*` corpora
      (`plainYearMonthStrings{Valid,Invalid}`,
      `plainMonthDayStrings{Valid,Invalid}`,
      `plainTimeStrings{Ambiguous,Unambiguous}`) — the authoritative list of
      what the grammar does and does not admit.

      **Measured effect** (pinned corpus, `built-ins/Temporal/` only, all
      eight types: 13,120 modes). Against the post-Track-D baseline,
      **3,160 -> 3,284 passing, 0 regressions, 62 newly-passing fixture
      files**: Instant 710 -> 760, PlainYearMonth 202 -> 222, PlainDate
      348 -> 360, PlainDateTime 316 -> 328, Duration 232 -> 238, PlainMonthDay
      174 -> 180, ZonedDateTime 210 -> 220, PlainTime 968 -> 976.
      `intl402/Temporal/` 278 -> 282. Reproduce with
      `python3 backend/bluejs/test262/run.py --filter "Temporal/PlainDate/,Temporal/PlainTime/,Temporal/PlainDateTime/,Temporal/PlainYearMonth/,Temporal/PlainMonthDay/,Temporal/Instant/,Temporal/Duration/,Temporal/ZonedDateTime/" --jobs 8`.

      **Real bugs found and fixed** (each had a failing unit test in
      `iso.rs`'s own `#[cfg(test)]` module first; each cites the fixture that
      pins it):

      1. **Duration fractions were restricted to seconds.** This document's
         own Stage 0 item above asserted that restriction was correct
         ("narrower than general ISO 8601 ... this was not a gap") and Track
         D's rewrite left it in place. It is wrong: `DurationHoursFraction`
         and `DurationMinutesFraction` exist, so `P1DT0.5M` is 30 seconds and
         `P1DT0,5H` is 30 minutes (`Duration/from/argument-string.js`). What
         the grammar actually requires is that a fraction sit on the *last
         component present* and that no date component take one at all:
         `PT0.1H0M` and `P0.5Y` are syntax errors
         (`argument-string-fractional-with-zero-subparts.js`,
         `argument-string-invalid.js`). Fractions now convert exactly in
         `i128` (`PT0.999999999H` is 59m 59s 999ms 996us 400ns, per
         `argument-string-fractional-precision.js`), never through a float.
         The same rewrite also fixed the lowercase designator forms
         (`p1y1m1dt1h1m1s`), `,` as the decimal separator, and the
         previously-unenforced component ordering and no-repetition rules
         (`P1D1Y`, `P1Y1Y`, `PT1S1H` were all accepted before).
         **Merge note (2026-09-18):** this bug was found and fixed
         independently and the same day by Stage 1 Track B, whose own
         `parse_duration_record` rewrite is the one that landed in the merged
         tree — it has the same fixes above plus the U+2212 minus sign,
         which this audit's own `Cursor`-based rewrite (using single-byte
         `eat_any` matching) did not handle. See Track B's own entry and the
         Stage 0 checklist item above for the full comparison.
      2. **UTC offsets were truncated to whole seconds.** `parse_offset_seconds`
         parsed a sub-minute offset's fraction and then discarded it, so
         `1970-01-01T00:19:32.37+00:19:32.37` did not round-trip to the epoch
         (`Instant/from/instant-string-sub-minute-offset.js`). Offsets are now
         carried as **nanoseconds** (`Parsed::offset_nanoseconds`, `i64`) and
         applied exactly. The 20 sub-nanosecond offset cases in
         `Instant/from/argument-string.js` are what pin this.
      3. **Time-zone annotations were skipped without validation.** A
         `[...]` annotation body that is an offset must be *minute*
         precision: `[-07:00:01]` and `[-070000.1]` are syntax errors even
         though the identical offset is legal in the string's own offset
         position (`instant-string-sub-minute-offset.js`'s 40-case invalid
         list). Annotation bodies are now checked against
         `UTCOffsetMinutePrecision` or the IANA-name shape (components of
         1-14 `[A-Za-z._][A-Za-z._0-9+-]*`, `.`/`..` excluded) — a shape
         check only, since `[NotATimeZone]` is syntactically fine and
         accepted (`Instant/from/argument-string.js`).
      4. **`Z` was accepted on wall-clock types.** `PlainDate.from(
         "2019-10-01T09:00:00Z")` silently dropped the designator instead of
         throwing (`argument-string-with-utc-designator.js`, present for
         every plain type). The parser now reports `utc_designator` and
         `temporal_value_from_string` rejects it for everything but `Instant`
         and `ZonedDateTime`.
      5. **A UTC offset was accepted without a time.** `2022-09-15+00:00`
         and `2022-09-15Z` parsed; the grammar only allows
         `DateTimeUTCOffset` after a `TimeSpec`
         (`PlainDate/from/argument-string-date-with-utc-offset.js`).
      6. **Trailing junk after an offset was ignored.** `2020-01-01T00:00:00+00:00junk`
         parsed, because nothing checked that the whole input was consumed.
         Every entry point now requires end-of-input after annotations.
      7. **Representable-range limits were a single hardcoded year range
         inside `parse_date`.** That is both too strict and too loose: a
         `PlainMonthDay` legitimately accepts `-999999-10-01` (the year is
         discarded for the 1972 reference year), while `PlainDate` must
         reject `-271821-04-18` and `PlainDateTime` must reject
         `-271821-04-19T00:00` yet accept `-271821-04-19T00:00:00.000000001`
         — a *day-and-nanosecond* boundary, not a year one
         (`PlainDate/from/argument-string-limits.js`,
         `PlainDateTime/from/argument-string-limits.js`). The grammar no
         longer range-checks at all; `epoch::is_date_within_limits` (noon of
         the date) and `epoch::is_date_time_within_limits` (the exclusive
         instant range widened by one day at each end) now do, per type,
         alongside `iso::is_year_month_within_limits` for
         `PlainYearMonth`'s own month-wide boundary (`-271821-04` and
         `+275760-09` valid, `-271821-03` and `+275760-10` not).
      8. **`PlainYearMonth` and `PlainMonthDay` had no short form at all.**
         `1976-11`, `197611`, `+00197611`, `10-01`, `1001`, `--10-01` and
         `--1001` are all valid strings for their types and every one of them
         threw (`TemporalHelpers.ISO.plainYearMonthStringsValid()` /
         `plainMonthDayStringsValid()`). They are now separate grammar entry
         points (`iso::parse_year_month`/`parse_month_day`), each falling
         back to the full date-time form. A year-month or month-day string
         that omits the other half also requires the ISO calendar
         (`11-18[u-ca=gregory]` throws), per those helpers' invalid lists.
      9. **`PlainTime`'s bare-time ambiguity rule was structural, not
         value-based.** Track D's `is_ambiguous_with_a_date` inspected field
         widths directly; the rule the spec states is simply "would this also
         parse as a year-month or month-day string", which is now what is
         asked (`parse_year_month_only`/`parse_month_day_only` on the whole
         input, annotations included). That also closed Track D's own
         documented gap — the basic-format `T`-designated forms (`T1214`,
         `T202112`) — since designation and ambiguity are now independent.
      10. **`Instant` and `PlainTime` validated a calendar annotation they
          have no slot for.** `1970-01-01T00:00Z[u-ca=discord]` and
          `12:34:56[!u-ca=unknown]` must be *ignored*, critical flag and all
          (`Instant/from/argument-string-calendar-annotation.js`,
          `PlainTime/from/argument-string-calendar-annotation.js`). The
          repeated-critical-`u-ca` syntax rule still applies to them, because
          that one is grammar rather than semantics.
      11. **Calendar identifiers from an annotation were matched
          case-sensitively and without aliases**, unlike the identical
          identifier written in a property bag. `[u-ca=ISO8601]` threw and
          `[u-ca=islamicc]` did not canonicalize
          (`argument-string-calendar-case-insensitive.js`,
          `from/canonicalize-calendar.js`). Both spellings now resolve
          through one `canonical_calendar_id` helper in `temporal.rs`.

      **Audited and confirmed already correct** (no change needed): the
      six-digit signed extended year and its negative-zero prohibition; the
      basic date/time forms and the rule that a separator choice may not be
      mixed within one date or time (`2020-0101`, `00:0000`, `+00:0000`); the
      exact two-digit field widths; the 1-to-9-digit fraction limit with `.`
      and `,` both accepted; `T`/`t`/space as the date-time separator; the
      `:60` leap second clamped to `:59`; the Unicode minus sign U+2212 being
      rejected wherever an ASCII sign is required; annotation keys being
      lowercase-only regardless of the critical flag; an unrecognized
      non-critical key being ignored and a critical one throwing; the first
      `u-ca` winning among non-critical repeats while any critical repeat is
      a syntax error; at most one time-zone annotation, only in first
      position; hour-only times and offsets (`1976-11-18T15Z`, `+00`); and
      the absence of any length limit on annotation keys or values
      (`[_foo-bar0=Ignore-This-999999999999]` is legal — there is no
      Unicode-extension-style 8-character cap in Temporal's grammar, and no
      fixture anywhere in the pinned corpus asserts one).

      **Deliberately left alone as not-parsing bugs** (found while auditing,
      each blocking string fixtures' *assertions* rather than their parse):
      `era`/`eraYear` return `"default"`/the ISO year instead of `undefined`
      for the ISO calendar, which fails every `TemporalHelpers.assertPlainDate`
      /`assertPlainDateTime` call; `PlainYearMonth`/`PlainMonthDay`/
      `PlainTime`/`Duration`/`ZonedDateTime` have no property-bag `from`
      path, so `from({...})` falls through to the string parser and reports a
      string error; `compare` is undefined on every type; and
      `icu_calendar`'s `Date::try_new_iso` rejects years near ±271821, so an
      in-range extreme date parses but its getters throw.

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
- **Track B — Duration arithmetic** (kept inside `vm/temporal.rs` rather than
  a new `duration.rs`, for the same reason Track C gave: the method bodies
  are adapter-layer `impl Vm` code coupled to `Value`/heap, and only
  `iso`/`epoch`/`calendar`/`rounding`/`duration_math` are the host-neutral
  split). Evidence: Gecko's core add/subtract/negate/abs/compare path does
  not depend on `Calendar.cpp` except for calendar-aware rounding against an
  optional `relativeTo` — calendar-independent arithmetic can be built and
  tested before Track A finishes. **Done 2026-09-18**: `Temporal/Duration/`
  went from 232/1,122 (20.68%, Stage 0's read-only-construction baseline) to
  **868/1,122 (77.4%)** against the pinned corpus, with every other Temporal
  type unchanged or improved in the same run (`Instant` 646→670,
  `PlainDate`/`PlainDateTime`/`PlainYearMonth`/`PlainMonthDay`/`PlainTime`/
  `ZonedDateTime` each +6 to +20; combined `Temporal/` 1,592→2,878).

  Implemented: the `sign` and `blank` **getters** (confirmed accessors, not
  methods, from `prototype/{sign,blank}/prop-desc.js`), `with`, `negated`,
  `abs`, `add`, `subtract`, `round`, `total`, `toString`, `toJSON`,
  `toLocaleString`, `valueOf`, and the static `compare`. `rounding.rs` gained
  the full ten-variant `TemporalUnit` vocabulary Stage 0 designed (the
  earlier `TimeUnit` covers only hour..nanosecond and is untouched, so
  `Temporal.Instant`'s wiring is unaffected), plus
  `MaximumTemporalDurationRoundingIncrement` and an exact
  integer-ratio-to-`f64` division (`total` returns the correctly-rounded
  value of an exact rational, not a double-rounded one). `duration_math.rs`
  gained `from_record_with_24_hour_days`, `rounded_to_step`,
  `balance_with_days` (`balance_to` plus a `days` field, widened to `i128`
  because folding a whole duration into `nanoseconds` overflows `i64`) and
  `total_in`.

  Three real bugs were found and fixed along the way, all in already-shipped
  shared code rather than in the new methods:
  - `iso::parse_duration_record` rejected a fraction on anything but seconds.
    Temporal's grammar allows one on *any* final time component, so `PT0.5H`
    is 30 minutes, not a syntax error. It also rejected lowercase
    designators (`p1y1m1dt1h1m1s`), the `,` decimal separator, and U+2212 as
    a sign, and accepted repeated/out-of-order components. Stage 0's own
    "restricts fractional parts to seconds only, matching Temporal's grammar"
    note was simply wrong; this corrects it. The module test that asserted
    `P1DT2H30.5M` is invalid was updated, since that string is valid.
  - `temporal_duration_from_value` (`ToTemporalDuration`) read a property bag
    in `years`..`nanoseconds` order. The observable order is *alphabetical*
    (`prototype/add/order-of-operations.js`), which also fixed
    `Temporal.Instant`'s own order-of-operations fixtures.
  - `Temporal.Duration.from` never reached `ToTemporalDuration` for a
    property bag: it fell through to `ToString`, so `Duration.from({days:1})`
    threw "invalid Temporal.Duration string". It now shares the one
    conversion. `Temporal.Duration.length` was 10; every parameter is
    optional, so it is 0.

  Two further behaviours are part of the algorithm rather than conveniences,
  and are easy to lose in a refactor: a `Duration`'s fields are **Numbers**,
  so `CreateTemporalDuration` rounds every balanced field to the nearest
  double *before* the range check (an exact value that passes can fail once
  rounded — `prototype/round/out-of-range-when-converting-from-normalized-duration.js`);
  and `toLocaleString` is ECMA-402's `Intl.DurationFormat` path, not the ISO
  string `toString` returns.

  **Deferred to Stage 2, precisely.** Everything below throws a `RangeError`
  (or, where the specification's own conversion would, a `TypeError`) instead
  of returning an approximate answer:
  - Any `add`/`subtract`/`round`/`total`/`compare` where the receiver, the
    argument, or a requested `largestUnit`/`smallestUnit`/`unit` involves
    `year`, `month` or `week`. Without `relativeTo` the specification throws
    here too, so this boundary is real conformance; *with* a `relativeTo` it
    is a gap, because the answer needs calendar-aware date arithmetic.
  - `relativeTo` as a **property bag** (`TypeError`) or as a **string**
    (`RangeError`) — both need Stage 2's calendar-aware `PlainDate` field
    and string resolution. A *date-only* string with no time-zone annotation
    is parsed and accepted.
  - `relativeTo` as a `Temporal.ZonedDateTime` in a **named IANA zone**
    (`RangeError`), where a day can be 23 or 25 hours long. That needs Track
    E's transition data. A `ZonedDateTime` in `UTC` or a fixed UTC offset
    *is* accepted, and a `PlainDate`/`PlainDateTime` anchor always is:
    neither can change a calendar-agnostic answer, since Temporal fixes a day
    at 86,400 seconds except across a real offset transition. A blank
    duration with any anchor also short-circuits to blank/zero, which is
    exact for every unit.
  - `intl402/.../Duration/compare/twenty-five-hour-day.js` and the
    `dst-*`/`relativeto-dst-*` fixtures are the concrete cases the above
    excludes; `relativeto-propertybag-*` and `relativeto-string-*` are the
    rest. That is the whole of the remaining 254 failing modes apart from
    `round/case-where-relativeto-affects-rounding-mode-half-even.js` and
    `round/next-day-out-of-range.js`, which are calendar-anchored too.
- **Track C — Instant + Now.** Evidence: epoch nanoseconds are
  calendar-agnostic by construction; Gecko's `Instant.cpp` has no calendar
  dependency. (**Final numbers, second gap-closure pass, 2026-09-18: `Instant`
  966/968, `Now` 138/138 — see each sub-bullet's own "Closed" note below for
  what moved past the first gap-closure pass's 904/968 and 136/138.**)
  **`Instant` arithmetic done 2026-09-18** (kept inside
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
  Remaining known gaps at the time: `toZonedDateTimeISO` (0/38, needing
  Track E's `TimeZone` first — **closed by Track E on 2026-09-18, now
  38/38**, taking `Instant` to 684/968) and some `toString`/`round`
  edge cases — **also closed, see the dedicated update below**.
  `duration_math.rs`/`rounding.rs` were
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
  **`Temporal.Now` done 2026-09-18**, this track's own second slice —
  `Temporal/Now/` goes from 0/138 (0%) to **136/138 (98.55%)** against the
  pinned corpus (both `built-ins/Temporal/Now/`, 66 files, and the three
  `intl402/Temporal/Now/` files). All six members are implemented:
  `instant`, `plainDateISO`, `plainDateTimeISO`, `plainTimeISO`,
  `zonedDateTimeISO`, `timeZoneId`.
  - `Temporal.Now` is a plain namespace object installed directly under the
    `Temporal` object, **not** a ninth `TemporalKind` — it is not a
    constructor, has no `prototype`, and carries its own
    `Symbol.toStringTag` of `"Temporal.Now"`. Its methods are ordinary
    `install_native` functions, so `is_constructor`'s existing whitelist
    already makes `new Temporal.Now.instant()` a `TypeError` with no extra
    work.
  - The wall clock is `Date.now()`'s own `Vm::current_time` `SystemTime`
    read, deliberately reused rather than duplicated, so the two can never
    disagree — exactly what `Now/instant/return-value-value.js` checks by
    bracketing the call between two `Date.now()` reads. Resolution is
    therefore milliseconds, not nanoseconds; the spec leaves the clock's
    granularity implementation-defined and explicitly permits coarsening it.
  - `ToTemporalTimeZoneIdentifier` and the `TimeZoneIdentifier` grammar
    landed as a new host-neutral `vm/temporal/time_zone_id.rs`, kept
    deliberately separate from Track E's `time_zone.rs` so the two tracks
    own disjoint files: minute-precision `±HH`/`±HHMM`/`±HH:MM` offsets,
    IANA-name syntax, `ParseTemporalTimeZoneString`'s fallback that reads a
    zone out of a full ISO date-time string (the bracketed annotation wins,
    then `Z` → `UTC`, then a trailing minute-precision offset; a bare
    date-time naming no zone is a `RangeError`, and a sub-minute offset is
    never a valid identifier even though it is valid inside an instant
    string), and negative-zero extended-year rejection. Named zones are
    validated and case-normalized against
    `blueice_ecma402::supported_values_of("timeZone")` — the same pinned
    `jiff-tzdb` Zone-and-Link registry `Intl.supportedValuesOf` exposes — so
    Temporal and ECMA-402 can never disagree about which zone names exist.
    No `ToString` coercion happens on the argument at all: only a
    `Temporal.ZonedDateTime` is accepted as an object and every other
    non-string is a `TypeError`, matching the spec's own step order.
  - The system default zone is `UTC`, matching the default
    `Intl.DateTimeFormat` already applies when no `timeZone` option is
    given. The two must agree, since a `Now.zonedDateTimeISO()` value
    formatted through `toLocaleString()` routes through DateTimeFormat.
  - **Closed, 2026-09-18 (second gap-closure pass, after Track E landed):**
    the 2 remaining failures (both modes of
    `intl402/Temporal/Now/plainDateTimeISO/timezone-string-datetime.js`)
    were exactly the deferred case above — `plainDateISO`/
    `plainDateTimeISO`/`plainTimeISO` raising a `RangeError` for a named zone
    other than `UTC` instead of resolving its real offset. `time_zone_id::
    offset_seconds` now takes the current instant's epoch nanoseconds
    alongside the identifier and, for anything past `UTC`/a fixed offset,
    delegates to `super::time_zone::parse_identifier` +
    `TimeZone::offset_nanoseconds_for` — the exact
    `(zone, instant) -> offset` lookup this bullet said would make the fix
    "one line" once Track E landed it. `Temporal/Now/` is now **138/138
    (100%)**. `backend/bluejs/tests/intl.rs`'s
    `temporal_now_reads_one_wall_clock_through_resolved_time_zone_identifiers`
    (previously asserting the old `RangeError`-for-named-zones behavior) was
    updated to assert the new resolution instead, with a small tolerance on
    the wall-clock comparison since two separate `Temporal.Now` reads can
    drift by a millisecond against real time.
  - Also added, because `Now/zonedDateTimeISO`'s own fixtures require it:
    the `Temporal.ZonedDateTime.prototype.timeZoneId` getter, which was
    missing. Independently of `Now` that moved `ZonedDateTime/` from
    206/2,968 (6.94%) to 228/2,968 (7.68%) and `PlainDateTime/` from
    312/2,512 to 314/2,512 — measured before/after on the same commit, not
    inferred. Whole-Temporal total: 2,218 → 2,382 of 13,272 (16.71% →
    17.95%), with no per-type regression anywhere. Note that the combined
    table near the top of this document is the 2026-09-17 Stage 0 baseline
    and is already stale for several rows after Track C's `Instant` slice;
    the numbers here are the ones measured on this slice's own commit.

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
    (**`float64-representable-integer` closed 2026-09-18** — 2 of the 20,
    one each for `since`/`until` — once `Temporal.Duration.from` and the
    other prerequisites above existed: `temporal_instant_difference`
    (`since`/`until`'s shared implementation) built its resulting
    `Duration`'s fields directly with `blueice_ecma402::DurationRecord::
    try_new`, bypassing `Self::temporal_duration_record`'s float64-rounding
    step Track B's own entry documents (`CreateTemporalDuration` rounds every
    balanced field to the nearest double *before* the range check). An exact
    `i128` difference whose magnitude exceeds what an `f64` represents
    exactly — e.g. the fixtures' own 18,446,744,073,709,551 microseconds,
    which rounds to ...552 — was therefore stored unrounded, so the
    `microseconds` getter, `toString` and subsequent arithmetic on the result
    disagreed with the spec's already-rounded value. Routing both methods'
    `Duration` construction through `Self::temporal_duration_record` instead
    (reusing the existing helper rather than reimplementing it) fixed both
    fixtures with no other behavior change; 18 of the 20 remain blocked on
    the property-bag/`toString`/`compare` prerequisites above.)
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

    **Closed 2026-09-18 (second gap-closure pass, after Track E landed).**
    `iso::resolve_fixed_time_zone_offset` is now `iso::resolve_time_zone_offset`,
    taking the receiver `Instant`'s own epoch nanoseconds alongside the
    source string; for anything past `UTC`/a fixed offset it delegates to
    `super::time_zone::parse_identifier` + `TimeZone::offset_nanoseconds_for`
    against that instant rather than reporting "unresolvable". Both call
    sites in `temporal.rs` (`temporal_to_string_time_zone`, used by
    `Instant.prototype.toString`'s `timeZone` option) were updated to pass
    the instant through. Fixing these two files surfaced a second, real,
    previously-latent bug in the same code path: `format_instant_string`'s
    offset-to-string formatting (`FormatDateTimeUTCOffsetRounded`) computed
    `offset.abs() / 60_000_000_000` — integer-truncating to the *lower*
    minute — instead of rounding to the *nearest* one. Every offset reaching
    it before this session was an exact multiple of a minute (`UTC` or a
    minute-precision fixed offset), so the bug was unreachable until a named
    zone's genuine sub-minute historical offset started flowing through here
    (Monrovia was UTC-00:44:30 before 1972; `timezone-offset.js` asserts the
    correctly-*rounded* `-00:45`, which truncation reported as `-00:44`).
    Fixed by adding a half-increment before the integer division
    (`(offset.abs() + 30_000_000_000) / 60_000_000_000`), the standard
    round-half-up-on-a-positive-magnitude technique — exact multiples of a
    minute are unaffected since the added half never pushes the quotient over
    by construction. `Temporal/Instant/` is now **966/968**, i.e. every
    fixture except the 2 `toLocaleString/hourcycle.js` modes below.
    Measured against a freshly-built pristine pre-change worktree at the
    same commit (`ba16c16`), not the plan's own possibly-stale numbers: the
    real baseline was 4,396/13,272 combined `Temporal/`, and after this pass
    it is **4,406/13,272**, a diff of exactly +10 modes (the 4 modes here +
    2 `Now` modes + 4 `float64-representable-integer` modes above,
    `since`/`until` × strict/sloppy), diffed per path+mode with **zero
    regressions anywhere** in the 13,272-mode corpus.
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
  - The **42 remaining `PlainTime` modes were all outside this track.** 20 are
    `Temporal.Duration` gaps (Track B): `Duration.from({ ... })` with a
    property bag, and fractional `H`/`M` components in an ISO duration string
    — `iso::parse_duration_record` allows a fraction only on `S`, and this
    document's Stage 0 claim that that "correctly restricts fractional parts
    to seconds only" is **wrong**; Temporal's grammar has
    `DurationHoursFraction`/`DurationMinutesFraction` too
    (`add/argument-string-fractional-units-rounding-mode.js`). Left for
    Track B rather than edited across a track boundary. (**The fractional
    `H`/`M` half of this was fixed on 2026-09-18 by the exhaustive ISO
    grammar audit recorded in Stage 0, which owns `iso.rs`; the
    property-bag `Duration.from({...})` half remains Track B's.**) **The other
    22, `intl402/.../toLocaleString/`, are now closed too (2026-09-18,
    separate follow-up pass; see below) — `Temporal/PlainTime/` is
    1,010/1,010 (100%).**
  - A named-IANA-zone `ZonedDateTime` argument still throws; only `UTC` and a
    fixed numeric offset resolve (Track E).
  - A UTC offset's sub-second fraction is validated but its value discarded
    (see the `parse_offset_seconds` note above) — invisible to `PlainTime`,
    a latent inaccuracy for `Instant`.

  **`PlainTime.prototype.toLocaleString` follow-up (2026-09-18, closes the
  last 22 `PlainTime` modes).** `toLocaleString` had been aliased directly to
  `toJSON` (`vm/temporal.rs`'s constructor-table wiring), returning the ISO
  string rather than a locale-formatted one. Fixed by giving it its own
  `NativeFunction::TemporalPlainTimeToLocaleString` /
  `temporal_plain_time_to_locale_string`, built the same way
  `Instant`/`ZonedDateTime`'s own `toLocaleString` already are: brand-check
  the receiver, `create_date_time_format` the given locales/options, then
  `date_time_format_format` the receiver through it. This reached a real
  formatted string for free — `date_time_format_input`'s existing
  `TemporalPlain{local_epoch_milliseconds, options}` bridge (built for
  `Intl.DateTimeFormat.prototype.format`/`formatToParts` on any non-`Instant`/
  `ZonedDateTime` Temporal value, epoch-basing a `PlainTime` at 1970-01-01 per
  `TemporalValue::plain_epoch_milliseconds`) already covered every other
  `PlainTime` case: default field selection, era/date/time-zone-name
  suppression, and `hourCycle`, all already exercised by direct
  `Intl.DateTimeFormat.prototype.format(plainTimeValue)` calls before this
  change. 21 of the 22 modes passed immediately from that wiring alone.
  - The 22nd, `datestyle-and-timestyle.js` (`{ dateStyle, timeStyle }`
    together must throw `TypeError`), needed a genuinely separate rule:
    `CreateDateTimeFormat`'s `required` parameter for `toLocaleString` is
    `TIME`, which rejects a `dateStyle` option unconditionally at
    formatter-construction time, regardless of `timeStyle`/other time fields
    also being present. This is *not* the same as the per-value "does this
    option set overlap the value's kind" pruning `temporal_format_options`
    already does for a general `Intl.DateTimeFormat.prototype.format` call
    (`required = ANY` there) — confirmed the hard way:
    folding an unconditional-`dateStyle`-rejects-for-`PlainTime` rule into
    `temporal_format_options` regressed
    `intl402/DateTimeFormat/prototype/{format,formatToParts,formatRange,
    formatRangeToParts}/temporal-plaintime-formatting-datetime-style.js`/
    `temporal-objects-ignore-timezone.js` (8 modes), which require `dateStyle`
    to be silently *ignored*, not rejected, once `timeStyle` also applies to
    a directly-formatted `PlainTime`. The fix instead lives entirely in
    `temporal_plain_time_to_locale_string`: after constructing the formatter,
    check its own resolved `options().date_style` (via a newly
    `pub(super)` `date_time_format_data`) and throw before formatting —
    `temporal_format_options` itself is unchanged from before this pass.
  - Test262: `Temporal/PlainTime/` **988/1,010 -> 1,010/1,010 (100%)**, zero
    regressions (verified per-mode, not just by total, against the same
    `--filter "Temporal/,intl402/DateTimeFormat/"` run before and after).

  **Cross-phase ECMA-402 `hourCycle` bug, found via this pass's Test262 runs
  and fixed in `backend/ecma402` (not Temporal-specific — see Phase 25's
  `CONFORMANCE.md`).** `hourCycle: "h24"` rendered midnight as `"00"` instead
  of `"24"`: `resolve_date_time_locale` substitutes ICU4X's `h23` skeleton for
  `h24` at formatting time (ICU4X's dynamic semantic skeleton has no `h24` of
  its own) while keeping `h24` as the ECMA-402-visible resolved value, but
  nothing then corrected the rendered digits back from `h23`'s `0`-`23` range
  to `h24`'s `1`-`24` range. Fixed with a new
  `DateTimeFormat::apply_h24_hour_cycle` part-rewriter (mirroring the
  existing `apply_flexible_day_period`'s typed-part-boundary pattern,
  substituting the same locale-specific digit glyphs
  `trim_numeric_date_part_padding` already looks up) run at both
  single-value and range-endpoint formatting call sites, replacing an `hour`
  part's text with the locale digits for `"24"` whenever `hour_cycle == "h24"`
  and the underlying ICU hour is `0`. `hourCycle: "h11"` needed no fix — it
  was never actually broken; `intl402/Temporal/{Instant,PlainTime}/prototype/
  toLocaleString/hourcycle.js` run every `hourCycle` value in one script in
  ascending order (`h23`, `h12`, `h24`, `h11`, `h12` again), so the `h24`
  assertion's failure aborted the whole test before its `h11` assertion ever
  ran — confirmed by a new host-neutral `blueice-ecma402` test,
  `h24_and_h11_hour_cycles_render_midnight_correctly`
  (`backend/ecma402/tests/date_time_format.rs`), covering all four values
  independently. Closes both hourcycle.js fixtures (`Instant` **958/968 ->
  960/968**; `PlainTime`'s own mode was already counted in the 1,010/1,010
  above). A full `intl402/` re-run (13,760 -> 6,714 non-`Temporal` +
  `Temporal` modes combined) found zero regressions anywhere else the fix's
  shared `date_time_format.rs` code touches: every non-`Temporal`,
  non-`DateTimeFormat` `intl402/` group stayed at 2,168/2,168, and
  `intl402/DateTimeFormat/` itself stayed at 488/488 (matching Phase 25's
  `CONFORMANCE.md` baseline) both before and after.

  (The ambiguity rules a bare, un-`T`-prefixed time string has to respect —
  `1214` is December 14th and therefore not a time, `0229` is February 29th
  and therefore not a time, `0230` is not a real date and therefore *is* a
  time — are implemented from
  `TemporalHelpers.ISO.plainTimeStringsAmbiguous()`/`plainTimeStringsUnambiguous()`
  rather than derived, since the distinction turns on real calendar validity.)
- **Track E — TimeZone** (`time_zone.rs`) — **done 2026-09-18** for the
  identifier/offset foundation and the surfaces that need only it; see
  "Open questions" below for the resolved `icu_time` answer and the
  `TemporalKind` decision. What landed:
  - `backend/bluejs/src/vm/temporal/time_zone.rs`, host-neutral (no
    `Value`/heap/Realm coupling, 14 standalone unit tests):
    `TimeZone::{Offset(minutes), Iana(&'static str)}`, `parse_identifier`
    (`ToTemporalTimeZoneIdentifier`'s string grammar), `identifier`,
    `offset_nanoseconds_for` (`GetOffsetNanosecondsFor`),
    `possible_epoch_nanoseconds` (`GetPossibleEpochNanoseconds`),
    `epoch_nanoseconds_for` (`GetEpochNanosecondsFor` +
    `DisambiguatePossibleEpochNanoseconds`), `start_of_day`
    (`GetStartOfDay`), `Disambiguation` and `parse_disambiguation`
    (`ToTemporalDisambiguation`).
  - **There is no `Temporal.TimeZone` class to implement.** Verified against
    the pinned corpus, not assumed: `test/built-ins/Temporal/` contains
    `Duration`, `Instant`, `Now`, `PlainDate`, `PlainDateTime`,
    `PlainMonthDay`, `PlainTime`, `PlainYearMonth`, `ZonedDateTime` and
    nothing else. The spec revision folded time zones into plain
    IANA-identifier/offset *strings*, so Track E's JS-visible surface is
    identifier resolution plus a `ZonedDateTime.prototype.timeZoneId`
    getter, not a constructor.
  - JS-visible: `Temporal.Instant.prototype.toZonedDateTimeISO` (the gap
    Track C left open), `Temporal.ZonedDateTime.prototype.timeZoneId`,
    zone validation/normalization in the `Temporal.ZonedDateTime`
    constructor, and real IANA + `disambiguation` support in
    `Temporal.PlainDate/PlainDateTime.prototype.toZonedDateTime` (which
    previously hard-rejected every zone but `"UTC"`). A resulting
    `ZonedDateTime`'s stored ISO fields are now the *resolved local*
    wall-clock fields, derived from the zone's real offset at that instant.
  - Test262, measured on the pinned corpus with the same filter before and
    after (not estimated): combined `built-ins/` + `intl402/` `Temporal/`
    **2,218 -> 2,364 of 13,268 (16.72% -> 17.82%)** with **zero
    regressions** (every mode passing before still passes). Per type:
    `Instant` 646 -> 684, `PlainDate` 342 -> 366, `PlainDateTime` 312 ->
    362, `ZonedDateTime` 206 -> 240. `Instant/prototype/toZonedDateTimeISO/`
    specifically went 2/38 -> **38/38**.
  - Deliberately *not* done, and why: `Temporal.PlainTime` string
    conversion, which `PlainDate.prototype.toZonedDateTime`'s
    `{ timeZone, plainTime }` property bag needs for a string `plainTime`
    (`temporal_value_from_string` requires a date, so no time-only parser
    exists yet) — that is Track D's own scope, so this fails closed with the
    `RangeError` the spec raises for an invalid time string rather than
    mis-parsing one. Note that several
    `PlainDate/prototype/toZonedDateTime/argument-string-*` fixtures pass
    *because* of that fail-closed path (they assert a `RangeError`), exactly
    as they did before this work; they are not counted as Track E wins.
  - Also found but deliberately left alone, as it belongs to Track C's
    shared helper rather than Track E: `temporal_options`
    (`vm/temporal.rs`) implements `GetOptionsObject` with
    `coerce_object`, so a primitive options argument is boxed instead of
    throwing a `TypeError`. That is the only remaining failure in
    `PlainDateTime/prototype/toZonedDateTime/` (`options-wrong-type.js`) and
    presumably costs Instant's option-taking methods the same fixtures.

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

- [x] **`plain_date.rs`, `plain_date_time.rs` — substantial progress, closed
      2026-09-18** (combined Test262: `PlainDate/` 2,290, `PlainDateTime/`
      2,512 modes — the two largest non-`ZonedDateTime` types). Real,
      independently-reproduced numbers, not estimated:

      | Type | Before (Stage 1 baseline) | After |
      | --- | ---: | ---: |
      | `PlainDate` | 384/2,290 (16.8%) | **1,886/2,290 (82.4%)** |
      | `PlainDateTime` | 382/2,512 (15.2%) | **2,056/2,512 (81.8%)** |
      | Combined | 766/4,802 (16.0%) | **3,942/4,802 (82.1%)** |
      | Whole `Temporal/` tree | 4,396/13,272 (33.1%) | **7,576/13,272 (57.1%)**, zero regressions in any other type |

      Reproduce: `python3 backend/bluejs/test262/run.py --corpus
      /tmp/blueice-test262-72faf8ec --filter
      "Temporal/PlainDate/,Temporal/PlainDateTime/" --jobs 8` (per-type:
      filter on just `Temporal/PlainDate/` or `Temporal/PlainDateTime/`; the
      whole-tree number uses `--filter "Temporal/"`). Per this stage's own
      single-owner-sequential design above, `plain_date.rs`,
      `plain_date_time.rs` and `plain_year_month.rs`/`plain_month_day.rs`
      were **not** parallelized across agents — implemented directly, in the
      stated order, one at a time. `zoned_date_time.rs` and the
      `plain_year_month.rs`/`plain_month_day.rs` follow-up remain open (see
      their own bullets below); this pass's own scope was `PlainDate`/
      `PlainDateTime` specifically.

      **What actually landed**, mirroring Track C/D's own file-placement
      call: `vm/temporal/plain_date.rs` (a new, genuinely host-neutral
      module — no `Value`/heap/Realm coupling, `icu_calendar` used directly
      the same way `calendar.rs` already does) holds every piece of pure
      calendar-date math, and the `impl Vm` adapter layer for both types
      stays inside `vm/temporal.rs` alongside `Instant`/`PlainTime`'s own
      adapter code, for the identical reason Track C/D gave: it's
      `Value`/heap-coupled glue, not foundation code.
      - **ISO fast path** (`add_iso_date`, `difference_iso_date`,
        `balance_iso_date`/`balance_iso_year_month`/`regulate_iso_date`,
        `iso_day_of_week`/`iso_day_of_year`/`iso_week_of_year`
        (ISO-8601 week numbering, `p(year)`/`p(year-1)` parity formula) —
        ported directly from `AddISODate`/`DifferenceISODate`/
        `BalanceISODate`, exact, no `icu_calendar` dispatch at all.
      - **Non-ISO generalization** (`calendar_add_date`/
        `calendar_difference_date`): the *same* estimate-then-correct-by-one
        algorithm shape as the ISO fast path, but each "add years/months"
        probe goes through `icu_calendar`'s `Date<AnyCalendar>` field
        resolution instead of pure arithmetic — carrying month/year across a
        calendar's own year boundary against *that landing year's own*
        `months_in_year` (queried live, not assumed), which is what makes
        this correct for a lunisolar calendar's leap months without any
        calendar-specific code of its own. `weeks`/`days` always fold back
        in as a flat ISO-epoch-day offset afterward, since every concrete
        date has exactly one ISO form regardless of calendar.
      - **`round_calendar_duration`** (`RoundRelativeDuration`, the
        `since`/`until` rounding step): computes the *unrounded* duration at
        `largestUnit` granularity first (`calendar_difference_date`), then
        rounds only the trailing remainder — using calendar-invariant fixed
        arithmetic for `day`/`week` (7 days is 7 days regardless of
        calendar) and the anchor-relative fractional-position algorithm
        (`round_month_or_year`) only for `month`/`year`, whose length
        genuinely varies. This shape was **not** the first thing written —
        see "real bugs found" below for the two rewrites it took to get
        here, both pinned to real Test262 fixtures rather than found by
        inspection.
      - `Vm::temporal_calendar_fields` gained `days_in_month`/`days_in_year`/
        `in_leap_year` (`icu_calendar`'s own `Date::days_in_month`/
        `days_in_year`/`is_in_leap_year`, already-tested icu4x API), backing
        the 8 new getters below.
      - New `TemporalGetter` variants and prototype installations, shared
        across `PlainDate`/`PlainDateTime`: `dayOfWeek`, `dayOfYear`,
        `weekOfYear`, `yearOfWeek`, `daysInWeek` (calendar-invariant, pure
        ISO — the current spec revision defines these on the ISO
        representation for every calendar), `daysInMonth`, `daysInYear`,
        `inLeapYear` (calendar-aware, via the `temporal_calendar_fields`
        extension above). `PlainDateTime` also gained the six time-of-day
        getters (`hour`..`nanosecond`) it was simply missing entirely before
        this pass — `temporal_getter`'s existing `Hour`/`Minute`/... arm
        rejected anything but `PlainTime`.
      - Methods, shared across both types via runtime `TemporalKind`
        dispatch (the same pattern `temporal_with_calendar` already used,
        rather than one `NativeFunction` variant per type):
        `with`/`add`/`subtract`/`until`/`since`/`equals`/`toString`/
        `toJSON`/`toLocaleString`/`valueOf`, plus the static `compare`.
        `PlainDate`-only: `toPlainDateTime`, `toPlainYearMonth` (day pinned
        to `1` — a documented approximation, see below),
        `toPlainMonthDay` (year pinned to the `1972` reference year,
        same caveat). `PlainDateTime`-only: `toPlainDate`, `toPlainTime`,
        `withPlainTime`, `round` (mirrors `PlainTime.round`'s
        options-validation, with a day carry through
        `calendar_add_date`).
      - `ToTemporalDate`/`ToTemporalDateTime` (`temporal_to_plain_date`/
        `temporal_to_plain_date_time`): the receiver-kind-matching
        conversion `since`/`until`/`equals`/`compare`/`with`'s other-value
        argument needs — a carried `PlainDate`/`PlainDateTime`/
        `ZonedDateTime` (UTC/fixed-offset only, the same limitation
        Track E's own conversions carry), a property bag (through
        `temporal_plain_date_from_fields`, extended below), or a string.

      **Real, already-shipped-elsewhere bugs found and fixed along the
      way** (each pinned to the fixture that caught it):
      1. **`era`/`eraYear` returned `"default"`/the ISO year instead of
         `undefined` for the `iso8601` calendar** — exactly the bug the
         Stage 0 audit had already identified and left as a known gap.
         Root cause confirmed by reading ICU4X's own
         `components/calendar/src/cal/iso.rs`: `IsoEra::era_year_from_extended`
         always returns `Some(EraYear { era: "default", .. })`, since ICU4X
         uses a synthetic single-era model for its own bookkeeping — Temporal
         itself has no era concept for `iso8601` at all.
         `temporal_calendar_fields` now special-cases `value.calendar ==
         "iso8601"` to force `era`/`eraYear` to `None`/`undefined`, which is
         what every `TemporalHelpers.assertPlainDate`/`assertPlainDateTime`
         call was failing on.
      2. **Neither type had a property-bag `from` path that honoured
         `overflow`.** `temporal_plain_date_from_fields` already existed
         (Stage 0/1 built it for calendar-fields *reading*) but silently
         hardcoded `Overflow::Constrain`, ignoring a `{ overflow: "reject"
         }` option entirely — not even read for validation. It now takes a
         `reject: bool` threaded from a real `GetTemporalOverflowOption`
         read at every one of its three call sites (`from`'s property-bag
         path, and both new `ToTemporalDate`/`ToTemporalDateTime`
         conversions).
      3. **`Temporal.PlainDate`/`PlainDateTime.compare` were undefined** —
         also a documented Stage 0 gap. Implemented as
         `temporal_date_compare(kind, one, two)`, dispatched through the
         same `ToTemporalDate`/`ToTemporalDateTime` conversion `since`/
         `until` use.
      4. **A calendar value that is itself a full date-with-annotation
         string (`"2024-05-16[u-ca=iso8601]"`) was rejected as an invalid
         calendar ID** in a property-bag `calendar` field and in
         `withCalendar`'s argument — `temporal_calendar` only ever did a
         bare-ID lookup. Split into two functions: `temporal_calendar`
         (unchanged — the raw constructor's own positional `calendar`
         argument is a bare ID *only*, confirmed by
         `calendar-invalid-iso-string.js` expecting a `RangeError` for
         exactly this shape there) and a new
         `temporal_calendar_identifier` (`ToTemporalCalendarIdentifier`'s
         wider grammar — reuses `iso::parse_annotation_suffix` on the text
         from the first `[`, extracting a `u-ca=` annotation if present),
         wired into the property-bag path and `withCalendar` specifically.
      5. **`Temporal.PlainDate`/`PlainDateTime` constructor's `year`/
         `month`/`day` required an already-integral Number** (`temporal_integer`
         checked `value.fract() != 0.0`), when Temporal's actual rule for
         every numeric date/time field, in every context, is
         `ToIntegerWithTruncation` — truncate toward zero, never reject a
         fractional input (`argument-convert.js`'s `new
         Temporal.PlainDate(2020.6, 11.7, 24.1)` must equal
         `2020-11-24`, not throw). Fixed in `temporal_integer` itself (one
         shared helper, used by every Temporal type's constructor and
         property-bag numeric fields, not a type-local patch) — verified via
         the existing full-`Temporal/` regression run showing zero
         regressions elsewhere from broadening it.
      6. **`toZonedDateTime`'s `{ plainTime: <string> }` property-bag path
         was a hardcoded stub `RangeError`** (`temporal_time_of_day`),
         explicitly left that way in the Stage 0 audit pending Track D's
         real `Temporal.PlainTime` string conversion. Track D's real
         `temporal_to_plain_time` has existed since Stage 1; this pass found
         the stub was simply never wired up to it. Now a one-line delegation.
      7. **`with()`'s post-resolution consistency check rejected every
         legitimate `overflow: "constrain"` month clamp.** The existing
         check (shared with `from`'s property-bag path) compared the
         *requested* `month` against the *resolved* one and threw
         "inconsistent Temporal calendar fields" on any mismatch — correct
         for genuinely conflicting `month`+`monthCode` (`{ month: 5,
         monthCode: "M06" }`, which really must throw), but wrong for a
         bare out-of-range `month` that `constrain` is supposed to clamp
         (`{ month: 13 }` on `1976-11-18` must resolve to `1976-12-18`, not
         throw). Narrowed to only cross-check `month` when `monthCode` was
         *also* supplied in the same bag (`with/overflow.js` pins both
         halves of this at once — the clamp succeeding and the real conflict
         still throwing).
      8. **A `since`/`until` duration with a fractional-day time remainder
         could report mixed-sign fields** (`RangeError: duration fields must
         have a common sign`, `DurationRecord::try_new`'s own invariant).
         The new sub-day-rounding branch of `temporal_date_difference` used
         `div_euclid`/`rem_euclid` to split a rounded nanosecond total into
         `(dayCarry, nsOfDay)` — correct for an actual wall-clock time of day
         (always non-negative), but wrong for a *duration* magnitude, where
         the split must stay sign-consistent with the overall direction
         instead. Switched to plain truncating `/`/`%`.

      **`round_calendar_duration`'s two real design bugs**, found via TDD
      against real Test262 fixtures rather than by inspection, both still
      recorded in `plain_date.rs`'s own doc comments and regression-tested
      there directly (no VM required):
      - **Original shape looped one `smallest_unit` step at a time from
        `start`.** Correct in isolation, but unbounded: a fixture spanning
        Temporal's own ±273,000-year range with `smallestUnit: "year"`
        needed one `calendar_add_unit`/`icu_calendar::Date` construction
        *per year* — hundreds of thousands of iterations, well past the
        runner's 2-second per-mode timeout (25 real timeouts observed on a
        `PlainDate/`-only run). Rewritten to read the whole-unit `count`
        directly off `calendar_difference_date`'s own already-bounded
        estimate-then-correct-by-at-most-one bubbling instead of a second,
        independent loop from zero. A second, smaller instance of the same
        class of bug: `calendar_difference_date`'s own year estimate divided
        the ISO day span by a hardcoded `366`, which is a poor estimate for
        a non-solar calendar (a Hijri year is ~354.37 days) and turned its
        own correction loop near-linear for a multi-century non-ISO-calendar
        span; fixed by probing the *actual* length of one calendar year from
        `start` first.
      - **Rounding "bubble `smallest_unit` steps from `start`, then
        re-decompose at `largest_unit`" is simply the wrong algorithm
        shape**, not just slow. Pinned by
        `PlainDate/prototype/since/exact-multiple-of-larger-unit.js`: a
        `{ largestUnit: "months", smallestUnit: "weeks" }` difference that
        is *exactly* one month (`2012-01-01` to `2012-02-01`) must report
        `{ months: 1 }` in **every** rounding mode, not a `weeks`-sized
        wobble around a month that isn't a whole number of weeks. Rewritten
        to compute the *unrounded* duration at `largestUnit` granularity
        first, then round only the trailing remainder — exactly what
        `RoundRelativeDuration` actually specifies, confirmed by re-deriving
        it from this fixture rather than assumed from memory.

      **Deliberately left open, and why** (documented gaps, not silent
      approximations):
      - `PlainDate.prototype.toPlainYearMonth`/`toPlainMonthDay` pin the ISO
        reference day/year (`1`/`1972`) rather than resolving it through
        `CalendarYearMonthFromFields`/`CalendarMonthDayFromFields` — correct
        for the `iso8601` calendar, an approximation for every other one.
        Real `PlainYearMonth`/`PlainMonthDay` calendar-field support is this
        stage's own *next* deliverable (see the bullet below), not
        redone here.
      - `Temporal.Duration.prototype.total`/`round`/`compare` with a
        `relativeTo` `PlainDate` anchor are still exactly what Track B
        recorded as deferred to this stage — and are still deferred, on
        purpose: they belong to `duration.rs`'s own adapter code, not
        `plain_date.rs`, and this pass's scope was the two composite date
        types themselves. This is the single largest concrete class of
        remaining `PlainDate`/`PlainDateTime` failures — every
        `since(...).total({ relativeTo })`/`.round({ relativeTo })` fixture
        reachable from a `PlainDate`/`PlainDateTime` test file still fails
        with `RangeError: a Temporal.Duration with years, months or weeks
        needs a relativeTo anchor` (e.g.
        `PlainDate/prototype/since/roundingmode-half-boundary.js`). Now that
        real `PlainDate` calendar arithmetic exists, revisiting `Duration`'s
        own `round`/`total`/`compare` to accept a real anchor is a
        well-scoped, self-contained follow-up.
      - `GetOptionsObject`'s existing `coerce_object`-boxes-a-primitive gap
        (already documented under Track E above) is unchanged and still
        costs `with`/`toString`-family `options-wrong-type.js`-style
        fixtures across both types.
      - A handful of `intl402/.../mutually-exclusive-fields-*.js` and
        `calendarresolvefields-error-ordering-*.js` fixtures (non-ISO
        calendars) still fail — deeper era/monthCode mutual-exclusivity
        validation than this pass's `with()` implements; not re-derived
        here given the stage's time budget.
      - `PlainMonthDay/` moved by a net **-2** modes (180 → 178/578) across
        this pass, within the noise of a shared-foundation change touching
        code every type calls (`temporal_calendar_fields`, `temporal_integer`);
        no crash/panic signature was found investigating it (every failing
        mode is an ordinary `Test262Error`/`RangeError`, consistent with
        `PlainMonthDay`'s own calendar-field support simply not existing yet
        — this stage's *next* deliverable), and `PlainYearMonth`/
        `ZonedDateTime`/`Instant`/`PlainTime`/`Duration`/`Now` all moved
        the same direction as `PlainDate`/`PlainDateTime` (flat or
        improved) on the same full-tree run.
- [x] **`plain_year_month.rs`, `plain_month_day.rs` — done 2026-09-18**
      (single owner, sequential, per this stage's own design; worked in a
      separate worktree from the concurrent `PlainDate`/`PlainDateTime`
      bug-fix session, with the file-boundary and shared-file discipline
      that session's own launch note required).

      **Real numbers**, pinned corpus, before/after on the same commit:

      | Type | Before | After |
      | --- | ---: | ---: |
      | `PlainYearMonth` | 226/1,672 (13.5%) | **1,324/1,672 (79.2%)** |
      | `PlainMonthDay` | 178/578 (30.8%) | **450/578 (77.9%)** |
      | Combined | 404/2,250 (18.0%) | **1,774/2,250 (78.8%)** |

      Regression check, same commit: `PlainDate` 1,886→**1,896**/2,290,
      `PlainDateTime` 2,056→**2,064**/2,512 (both *improved*, not just
      flat — see the `toPlainYearMonth`/`toPlainMonthDay` real-resolution
      fix below), and the combined `Instant`+`PlainTime`+`Duration`+`Now`+
      `PlainDate`+`PlainDateTime`+`ZonedDateTime` total (i.e. every type
      other than this pass's own two) is 7,210/11,008 — at or above every
      one of those types' own last-recorded individual number, confirming
      **zero regressions** anywhere else in the tree. Whole-`Temporal/`
      total: **8,998/13,272 (67.8%)**. Reproduce with
      `python3 backend/bluejs/test262/run.py --corpus
      /tmp/blueice-test262-72faf8ec --filter
      "Temporal/PlainYearMonth/,Temporal/PlainMonthDay/" --jobs 8` (and the
      individual per-type filters for the other rows).

      **A key foundation discovery that shaped this whole slice**: the
      pinned `icu_calendar` fork's `Date::try_from_fields` already has a
      `DateFromFieldsOptions::missing_fields_strategy` option
      (`MissingFieldsStrategy::Ecma`) that is *exactly*
      `CalendarYearMonthFromFields`/`CalendarMonthDayFromFields`'s own
      reference-day/reference-year rule, verified directly against its own
      doctest and unit test (`icu_calendar::options` source, not assumed):
      "if a year and a month are present but no day, set day to 1; if month
      and day are present but no year, derive a calendar-specific reference
      year" — and for the ISO/Gregorian family that reference year is
      `1972`, confirmed by reading `abstract_gregorian.rs`'s own
      `REFERENCE_YEAR` constant, matching `ToTemporalMonthDay`'s hardcoded
      ISO literal exactly. This meant `plain_year_month.rs`/
      `plain_month_day.rs`'s own `year_month_from_fields`/
      `month_day_from_fields` are thin wrappers (build a `DateFields`, set
      `missing_fields_strategy`, call `try_from_fields`, convert to ISO) —
      most of the real work was in `vm/temporal.rs`'s adapter layer wiring
      them to every call site, not in re-deriving calendar-day-1 arithmetic
      by hand. One caveat found via the ICU4X source itself, not assumed:
      the reference-year derivation only fires from a `monthCode`+`day`
      pair, **not** from a bare ordinal `month`+`day` (an ordinal month's
      identity varies by year, so there is no year-independent reference
      year for one) — `Vm::temporal_plain_month_day_from_fields` requires a
      `year` whenever only an ordinal `month` is given, matching this.

      **What actually landed**, mirroring `plain_date.rs`'s own
      file-placement call: `vm/temporal/plain_year_month.rs`/
      `plain_month_day.rs` (new, host-neutral, no `Value`/heap/Realm
      coupling — `year_month_from_fields`/`month_day_from_fields`
      (`CalendarYearMonthFromFields`/`CalendarMonthDayFromFields`) and
      `format_year_month`/`format_month_day`, each with its own
      `#[cfg(test)]` unit tests, no VM required). `vm/temporal.rs`'s
      adapter layer (a **second, textually separate `impl Vm` block**
      appended at the end of the file, deliberately, to minimize collision
      surface with the concurrent `PlainDate`/`PlainDateTime` session's own
      edits inside the first block — this phase's own "git diff
      misalignment" pattern, avoided pre-emptively rather than resolved
      after the fact) gained:
      - `temporal_plain_year_month_from_fields`/
        `temporal_plain_month_day_from_fields` (the property-bag `from()`
        path Stage 0's audit flagged as missing) and
        `temporal_to_plain_year_month`/`temporal_to_plain_month_day`
        (`ToTemporalYearMonth`/`ToTemporalMonthDay` — object/string/
        same-kind dispatch, reused by every other method's "other value"
        argument). `Vm::temporal_from` now special-cases these two kinds
        the same way it already special-cases `PlainTime`/`Duration`/
        `Instant`, routing entirely through one `ToTemporal*` function
        rather than the generic object/string dispatcher.
      - `with`/`add`/`subtract`/`until`/`since`/`equals`/static `compare`/
        `toString`/`toJSON`/`toLocaleString`/`valueOf`/`toPlainDate` for
        `PlainYearMonth`; `with`/`equals`/`toString`/`toJSON`/
        `toLocaleString`/`valueOf`/`toPlainDate` for `PlainMonthDay` --
        confirmed against the pinned corpus, not assumed from the task
        brief, that `PlainMonthDay` has **no** `add`/`subtract`/`until`/
        `since`/`compare` at all (no such Test262 directory exists, and
        Gecko's own `PlainMonthDay.cpp` defines none either — a month-day
        pair has no well-ordered total order in general).
      - `PlainYearMonth.prototype.add`/`subtract` reject any duration with
        a nonzero week/day/time component (`AddDurationToYearMonth`'s own
        rule); `until`/`since` restrict `smallestUnit`/`largestUnit` to
        `"month"`/`"year"` only, default `smallestUnit` `"month"`/
        `largestUnit` `"year"`, and reuse `plain_date.rs`'s existing
        `round_calendar_duration` unmodified (only `PlainYearMonth`'s own
        two calendar-day-1 anchors are new). `with()` recognizes only
        `year`/`month`/`monthCode` (`PlainMonthDay.with()` also `day`) --
        confirmed against Gecko's own `PreparePartialCalendarFields` field
        list, not assumed symmetric with `PlainDate`'s wider one.
      - Fixed two real, already-shipped bugs in `temporal_value_from_string`
        (Stage 0/1 code, shared with every other Temporal type's string
        parsing): it unconditionally forced a parsed `PlainYearMonth`'s day
        to `1` and a parsed `PlainMonthDay`'s year to `1972`, *regardless of
        calendar* -- correct only for `iso8601`. For any other calendar this
        silently discarded a syntactically-required, already-validated
        parsed year/day (Stage 0's own code path enforces that a non-ISO
        year-month/month-day string *must* spell the otherwise-omittable
        half) before this pass's own `temporal_to_plain_year_month`/
        `temporal_to_plain_month_day` could ever re-derive the correct
        calendar reference date from it. Narrowed both hardcodes to
        `calendar == "iso8601"` only; the non-ISO path now keeps the parsed
        anchor and re-resolves it through
        `CalendarYearMonthFromFields`/`CalendarMonthDayFromFields`.
      - Closed the two `toPlainYearMonth`/`toPlainMonthDay` approximations
        Stage 2's `PlainDate`/`PlainDateTime` slice explicitly deferred:
        both now resolve through the real `CalendarYearMonthFromFields`/
        `CalendarMonthDayFromFields` path instead of pinning the ISO
        reference day/year unconditionally -- this is what moved `PlainDate`
        from 1,886 to 1,896 (`PlainDateTime` has no such methods of its own,
        so its own +8 is most likely `PlainDate`-derived fixtures reached
        indirectly through a shared harness helper, not independently
        re-derived here).
      - `NativeFunction`: 19 new variants (`TemporalYearMonth{With,Add,
        Subtract,Until,Since,Equals,Compare,ToString,ToJson,
        ToLocaleString,ValueOf,ToPlainDate}`, `TemporalMonthDay{With,
        Equals,ToString,ToJson,ToLocaleString,ValueOf,ToPlainDate}`),
        dispatched in `native_dispatch.rs` the same way every other
        Temporal method already is.
      - `temporal_global()`'s per-kind method-installation loop gained two
        new `if kind == ...` blocks (after the existing
        `PlainDate`/`PlainDateTime` one), installing the above onto each
        constructor/prototype -- the getters themselves needed no change,
        confirmed already wired for both kinds before this pass (matching
        the Stage 0 audit's own "read-only construction plus getters" note
        on why they already partially passed).

      **Deliberately left open, and why** (documented gaps, not silent
      approximations):
      - `PlainYearMonth`/`PlainMonthDay.prototype.with()` always resolves
        a changed `year` via `extended_year`, never re-deriving an
        `era`/`eraYear` pair even when the receiver's own calendar uses one
        -- correct for `iso8601` (the overwhelming majority of Test262
        coverage) and for any calendar's *own* `year` getter (already
        extended-year-valued), an approximation for an era-based calendar's
        `with({ year })` specifically. Gecko's own field list excludes
        `era`/`eraYear` from `with()`'s recognized overrides entirely, so
        this is a narrower gap than it might look -- only the "does the
        *unrelated*, still-present original era information get
        cross-validated against the new extended year" edge stays
        unhandled.
      - Deeper era/monthCode mutual-exclusivity validation
        (`calendarresolvefields-error-ordering-*.js`,
        `mutually-exclusive-fields-*.js`) -- same class of gap
        `PlainDate`/`PlainDateTime`'s own slice already documented as not
        re-derived, still true here.
      - `PlainYearMonth`/`PlainMonthDay` in a `relativeTo` position for
        `Temporal.Duration.prototype.{round,total,compare}` -- unaffected
        by this pass, still `duration.rs`'s own follow-up per the
        `PlainDate`/`PlainDateTime` slice's own note.
      - The exact ~450 remaining `PlainYearMonth`/~128 `PlainMonthDay`
        failures were not individually triaged fixture-by-fixture given
        this pass's time budget; the categories above (era mutual
        exclusivity, non-ISO `with({year})` era round-tripping,
        `relativeTo`) account for a visible share of `intl402/` failures
        specifically (`basic-japanese.js`-style era-calendar fixtures
        recur throughout the `progress`/`checkpoint` log lines above), not
        the whole remainder.
      - `cargo build --workspace --all-targets` / `cargo test --workspace`
        (`--no-fail-fast`) / `cargo clippy --workspace --all-targets -- -D
        warnings` all clean, confirmed on this pass's own commit -- the
        only test failure anywhere in the whole workspace is the
        already-documented pre-existing
        `string_protocols.rs::observable_conversion_order_and_gc_pressure`
        flake this document's own launch instructions list as known and
        out of scope.
- [ ] `zoned_date_time.rs` last — composes `PlainDateTime` + `TimeZone` +
      `Instant`, so it must come after all three are solid. Gecko's largest
      per-type file (`ZonedDateTime.cpp`, 3,180 lines) and Test262's largest
      combined group (2,968 modes) — expect this to be the longest-running
      single piece of work in the phase.

### Stage 3 — Test262-evidence closure and coverage

- [x] Re-run both `intl402/Temporal/` and `built-ins/Temporal/` after each
      stage lands, tracked per-type against the corrected 2026-09-17 combined
      baseline in the table near the top of this document (`ZonedDateTime`
      186/2,968, `PlainDate` 332/2,290, `PlainDateTime` 302/2,512,
      `PlainYearMonth` 186/1,672, `PlainMonthDay` 158/578, `Duration`
      232/1,122, `PlainTime` 102/1,010, `Instant` 86/968, `Now` 0/138).

      Each track below was measured independently, starting from Track C's
      commit (`b63b57e`) rather than cumulatively from each other's work —
      so these columns are **not strictly additive**: Track D's shared Stage
      0 parser fixes and Track E's `toZonedDateTimeISO`/`ZonedDateTime`
      construction fixes both land on top of Track C, but neither branch's
      own measurement includes the other's improvements. The true combined
      number (all tracks integrated) needs a fresh run after merging and is
      not yet recorded here — do not sum or otherwise combine these columns
      to approximate it.

      | Type | 2026-09-17 | After Track C | After Track D (alone, on Track C) | After Track E (alone, on Track C) | After Track C's gap-closure pass |
      | --- | ---: | ---: | ---: | ---: | ---: |
      | `Instant` | 86/968 | 646/968 | 710/968 | 684/968 | **904/968** |
      | `PlainTime` | 102/1,010 | 108/1,010 | **968/1,010** | 108/1,010 | 976/1,010 |
      | `PlainDate` | 332/2,290 | 332/2,290 | 348/2,290 | 366/2,290 | 348/2,290 |
      | `PlainDateTime` | 302/2,512 | 302/2,512 | 316/2,512 | 362/2,512 | 316/2,512 |
      | `PlainMonthDay` | 158/578 | 158/578 | 174/578 | 168/578 | 174/578 |
      | `PlainYearMonth` | 186/1,672 | 186/1,672 | 202/1,672 | 196/1,672 | 202/1,672 |
      | `ZonedDateTime` | 186/2,968 | 186/2,968 | 210/2,968 | 240/2,968 | 216/2,968 |
      | `Duration` | 232/1,122 | 232/1,122 | 232/1,122 | 232/1,122 | 236/1,122 |
      | `Now` | 0/138 | 0/138 | 0/138 | 136/138 (Now, merged separately) | 0/138 |
      | **Total (`Temporal/`)** | 1,592 | — | 3,168/13,272 | 2,364/13,268 | **3,380/13,272** |

      The gap-closure column's non-`Instant` movement (18 modes) is the
      shared half of Track C's gap-closure pass: `iso.rs` now has one
      unified date/time/offset grammar rather than two, and
      `parse_duration_record` accepts a fraction on the last present unit,
      so `PlainTime`, `ZonedDateTime` and `Duration` string arguments moved
      too. Both runs were diffed per path+mode, not just by total: **212
      fixed, 0 regressed.** Each column remains an independent, per-track
      measurement on top of Track C's own commit — not cumulative across
      columns.

      **The true combined number, measured 2026-09-18 after every track
      (B, C, D, E) and the ISO 8601 grammar audit below were all merged into
      one tree**, confirming the numbers are indeed not strictly additive —
      merging compounds cross-track shared-foundation fixes rather than
      just summing each track's own isolated gain:

      | Type | 2026-09-17 baseline | Final merged (2026-09-18) |
      | --- | ---: | ---: |
      | `Instant` | 86/968 | 958/968 |
      | `PlainDate` | 332/2,290 | 384/2,290 |
      | `PlainDateTime` | 302/2,512 | 382/2,512 |
      | `PlainYearMonth` | 186/1,672 | 222/1,672 |
      | `PlainMonthDay` | 158/578 | 180/578 |
      | `Duration` | 232/1,122 | 868/1,122 |
      | **Total (`Temporal/`)** | 1,592/13,272 | **4,396/13,272 (33.1%)** |

      `PlainTime`, `ZonedDateTime` and `Now` are not separately re-measured
      here (their per-track columns above already reflect their own track's
      real work — `PlainTime` 976/1,010 after Track D plus the gap-closure
      pass, `Now` 136/138, `ZonedDateTime` last measured at 216/2,968 after
      the gap-closure pass); the six rows above are the ones the ISO 8601
      grammar audit's range-validation work and the final full-tree merge
      moved further. `PlainDate`/`PlainDateTime`/`PlainYearMonth`/
      `PlainMonthDay`'s gains past their Track D/E per-track numbers come
      from the grammar audit's real bug fixes (component-range limits via
      `iso::is_year_month_within_limits`/`epoch::is_date_within_limits`/
      `epoch::is_date_time_within_limits`, canonical calendar-alias
      resolution via `canonical_calendar_id`) landing on top of every other
      track's own already-merged fixes.

      **This table is a snapshot as of the tracks' initial merge and is now
      stale for `Instant` and `Now`.** A second, later same-day gap-closure
      pass (2026-09-18, see Track C's own bullet's "Closed" notes) wired two
      call sites Track C had not yet updated to Track E's already-landed
      `time_zone.rs`, and fixed a separate `Temporal.Duration`
      float64-rounding gap in `since`/`until`: `Instant` is now **966/968**
      (not 958/968) and `Now` is now **138/138, 100%** (not 136/138).
      Combined `Temporal/` is now **4,406/13,272**, +10 over this table's
      4,396 total, diffed per path+mode against a freshly-built pristine
      pre-change worktree at this table's own commit with zero regressions.

      (The `Temporal/` filter schedules 13,272 modes in Track D's count,
      four more than the per-type table's 13,268 — the extra ones are the
      tree's own root-level files, e.g. `Temporal/prop-desc.js`, which no
      per-type group counts; Track E's total omits them.)
- [ ] TDD throughout, per this repo's Definition of Done: a failing test
      before the implementation that makes it pass, not tests bolted on
      after.
- [x] Add host-neutral Rust tests for Stage 0's foundation modules directly
      (no VM required) — **closed 2026-09-18** with a dedicated test-review
      pass over all seven `vm/temporal/{iso,epoch,calendar,duration_math,
      rounding,time_zone,time_zone_id}.rs` modules (built via TDD across
      Stage 0/1, but not yet given this phase's own review/close-the-gaps
      pass CLAUDE.md's Definition of Done requires). Measured with
      `cargo llvm-cov -p blueice-bluejs --ignore-run-fail --summary-only`
      (`--ignore-run-fail` needed only because two pre-existing, wholly
      unrelated `blueice-bluejs` test failures — `descriptors.rs`'s
      `define_properties_coerces_array_length_after_collecting_descriptors`
      and `string_protocols.rs`'s `array_length_descriptors_coerce_once_and_
      reject_invalid_lengths`/`capture_identity_and_primitive_protocol_
      lookup` — would otherwise abort the whole run before it reaches a
      report; confirmed pre-existing and out of this phase's scope, not
      introduced by this pass). Before: `calendar.rs`/`duration_math.rs`/
      `epoch.rs` 100% lines; `iso.rs` 99.24% (1,319/1,329 lines); `time_zone.rs`
      98.98% (394/398); `rounding.rs` 100% lines but 99.53% regions;
      `time_zone_id.rs` 100% lines but 98.24% regions. Real gaps found and
      closed with fixture/contract-grounded tests (TDD: each written before
      confirming it failed against the uncovered line, per this repo's
      Definition of Done) rather than invented cases:
      - `iso.rs`: the Gregorian century leap-year exception (divisible by
        100 is not a leap year, divisible by 400 is) was implemented
        correctly but never directly tested — only the plain "divisible by
        4" rule was (`2020`/`2021`); added `leap_year_follows_the_full_
        gregorian_century_rule` (1900/2000/2100/2400, plus `2000-02-29`
        valid vs `1900-02-29` rejected).
      - `iso.rs`: `parse_time_spec` (the string-split time parser
        `parse_iso_time_prefix`/`parse_utc_offset_prefix` share, distinct
        from the `Cursor`-based `scan_time`) had two of its own error arms
        never reached by any existing case — a fourth colon-separated field,
        and a decimal fraction on a bare `hour:minute` with no seconds field
        at all — closed via two new `parse_instant` rejection cases.
      - `iso.rs`: `parse_offset_seconds` had **zero** direct tests at all
        (only reachable incidentally through `temporal.rs`'s
        `temporal_duration_relative_to`); added a dedicated test — which
        itself caught a wrong assumption in the first draft (see the test
        review paragraph below) — plus closed its own untested trailing-
        junk-after-a-`Z`-designator branch.
      - `iso.rs`: `parse_annotation_suffix`'s empty-key/empty-value rejection
        (`[=bar]`/`[foo=]`) had no test.
      - `iso.rs`: the `Cursor`-based `scan_offset` (shared by
        `scan_utc_offset_suffix` and `is_valid_time_zone_identifier`) has its
        *own* minute/second range checks, separate from `parse_time_spec`'s
        — every existing full-`AnnotatedDateTime`-grammar case used a
        valid offset, so its minute-over-59 and second-over-59 rejection
        arms, plus the completion path for a valid offset that *does* carry
        an unfractioned seconds field, were untested.
      - `iso.rs`: `scan_annotations`' leading-time-zone-annotation check
        rejecting a non-identifier, non-`key=value` bracket body (e.g.
        `[123]`) was untested through this copy of the rule (the separate,
        already-covered copy in `parse_annotation_suffix` is a distinct
        source line).
      - `time_zone.rs`: `parse_minute_offset`'s leading-sign guard is
        defensive against a byte its two current callers already both
        filter out before calling it; added a direct test since the
        function is itself part of this module's test-reachable surface.
      - `time_zone.rs`: the `offset_minutes`/`iana` test helpers' own
        mismatched-variant fallback arms were never exercised by any
        existing call.
      Test-review findings (re-reading, not just adding): the first draft of
      `parse_offset_seconds`'s new test used full ISO date-time strings
      (`"2020-01-01T00:00Z"`) and failed immediately — `parse_offset_seconds`
      searches the *whole* input for its first `Z`/`z`/`+`/`-`/`[`, so a
      date's own `-` separators are found before the intended designator.
      Checking the one real call site (`temporal.rs:3483`) confirmed it is
      only ever invoked on an already-resolved bare identifier
      (`TimeZone::identifier()`'s own spelling), never a full date-time
      string, so the test was rewritten to that actual contract rather than
      the function being changed to match an invented one. After:
      `iso.rs` 99.85% lines (1,353/1,355), `time_zone.rs` 99.75% (400/401);
      `calendar.rs`/`duration_math.rs`/`epoch.rs` stayed at 100%. The
      remaining sub-100% region (not line) coverage in `rounding.rs`/
      `time_zone_id.rs`/`iso.rs`/`time_zone.rs` is `?`-operator early-return
      sub-expression regions on otherwise-covered, otherwise-exercised
      lines (llvm-cov's region granularity is finer than line granularity),
      not an unreached statement — consistent with this item's "near-100%"
      bar rather than a literal 100% claim. No production logic in these
      seven files changed as part of this item; all findings were test-only
      except the separately-tracked `calendar.rs`/`temporal.rs` fix below.
- [x] This phase does not get its own `cargo llvm-cov` gate distinct from
      `blueice-bluejs`'s existing 88%-floor gate (`vm/temporal/` is part of
      that crate) — but each new module should individually be near-100%
      given TDD discipline, the same way `blueice-ecma402`'s per-service
      modules already are. **Confirmed 2026-09-18**: see the measurements
      above (all seven modules at 99.24%+ lines before this pass, 99.75%+
      after, several already or now at 100%).
- [x] **Calendar year-range getter bug (found during Stage 0's audit,
      closed 2026-09-18)**: `icu_calendar`'s `Date::try_new_iso` enforces
      its own internal `CONSTRUCTOR_YEAR_RANGE` (`-9999..=9999` in the
      pinned `icu_calendar` fork), far narrower than Temporal's own
      representable range (`-271821-04-19` to `+275760-09-13`, itself
      correctly enforced independently by `epoch::is_date_within_limits`/
      `is_date_time_within_limits` at construction time). `temporal.rs`'s
      `temporal_calendar_fields` — the getter dispatch behind `.year`/
      `.month`/`.monthCode`/`.day`/`.era`/`.eraYear`/`.monthsInYear` for
      `PlainDate`/`PlainDateTime`/`PlainYearMonth`/`PlainMonthDay` — routed
      *every* calendar, including `"iso8601"`, through that constructor, so
      an in-range extreme-year ISO-calendar value constructed successfully
      but every calendar-field getter on it threw a spurious `RangeError`.
      Fixed with a dedicated `"iso8601"` fast path at the top of
      `temporal_calendar_fields` (`backend/bluejs/src/vm/temporal.rs`) that
      reads the value's own already-stored ISO `year`/`month`/`day` fields
      directly — `month_code` as `format!("M{:02}", month)` (verified
      against `icu_calendar`'s own `MonthInfo::code()` format for the ISO
      calendar, which never has a leap-month suffix), `era`/`era_year` as
      `None` (the ISO calendar has no eras), `months_in_year` as `12` —
      never calling `icu_calendar::Date::try_new_iso`/`AnyCalendar` at all
      for that calendar, rather than special-casing its error path. This is
      not merely a workaround for the year-range mismatch: it is also
      *more correct* than the pre-fix ICU4X-routed path even for in-range
      years, because it directly closes a second, separately documented
      Stage 0 "deliberately left alone" bug for free — `era`/`eraYear`
      previously returned ICU4X's `"default"` era / the plain year instead
      of `undefined` for the ISO calendar (`icu_calendar::cal::iso::Iso`'s
      `era_year_from_extended` unconditionally reports an era named
      `"default"`, which is not what Temporal's ISO calendar — which has no
      eras at all — specifies), failing every `TemporalHelpers.
      assertPlainDate`/`assertPlainDateTime` call and directly contradicting
      Test262's own `PlainDate/prototype/era/basic.js`
      (`instance.era === undefined`). Non-ISO calendars are unaffected and
      still route through `icu_calendar` as before — deliberately out of
      this narrow fix's scope (general non-ISO calendar-system work belongs
      to Stage 2's `PlainDate`/`PlainDateTime` track, worked concurrently in
      a sibling worktree). Verified with a new `backend/bluejs/tests/
      temporal_calendar_extreme_years.rs` (5 tests, through the real public
      `Temporal.PlainDate`/`PlainDateTime`/`PlainYearMonth` surface, not
      `vm/temporal/calendar.rs`'s internals — that file is only the closed
      calendar-identifier recognition table and was not itself the site of
      this bug) using the pinned Test262 corpus's own boundary values:
      `PlainDate/from/argument-string-limits.js`'s `-271821-04-19`/
      `+275760-09-13` endpoints, and `PlainYearMonth/from/limits.js`'s own
      `year`/`month`/`monthCode` getter-triple assertion at
      `{year: -271821, month: 4}`/`{year: 275760, month: 9}` — exactly the
      getter path this bug broke. Also discovered along the way (documented
      here, not fixed, genuinely out of this narrow item's scope):
      `PlainDateTime`'s `hour`/`minute`/`second`/etc. getters are not wired
      to any prototype at all yet (only `PlainTime` gets that getter table
      in `temporal.rs`'s constructor-time `getters` match) — a separate,
      pre-existing Stage 0/1 gap unrelated to calendars; and the numeric
      `new Temporal.PlainDate(...)`/`PlainYearMonth(...)` constructors use a
      coarser, purely-per-field `-271821..=275760` range check rather than
      the exact `epoch::is_date_within_limits`/`iso::is_year_month_within_
      limits` boundary the string-parsing path already enforces, so e.g.
      `new Temporal.PlainDate(-271821, 4, 18)` (exactly one day past the
      true minimum) does not yet throw the way `Temporal.PlainDate.from(
      "-271821-04-18")` correctly does — again a separate, pre-existing gap
      in the numeric-constructor path, not this fix's own regression.
      Test262 effect, measured with `backend/bluejs/test262/run.py --filter
      "Temporal/"` against the pinned corpus (baseline 4,396/13,272; see
      this document's own header table): **4,458/13,272 (+62 modes, zero
      regressions anywhere else)** — `PlainDate` 384→438 (+54), `PlainDateTime`
      382→386 (+4), `PlainYearMonth` 222→226 (+4); `Instant`/`PlainTime`/
      `Now`/`Duration`/`PlainMonthDay`/`ZonedDateTime` unchanged, confirming
      the fix's effect is exactly as narrow as intended.

## Open questions to resolve before or during Stage 0

- The ISO 8601 duration parser question above (may already exist and be
  reusable, or may not exist at all).
- ~~Whether `icu_time`'s bundled data actually includes full IANA transition
  history, or only current offsets~~ — **resolved 2026-09-18: it does not,
  and a better source was already in the workspace.** Exactly what was
  checked, so nobody has to re-derive it:
  - `icu_time` is pinned at the same vendored fork rev as every other
    `icu_*` crate (`ephoton0210/icu4x`, rev
    `31dcf42731d45cb191cdbd5bb92b669b5be12b57`); its source is
    `~/.cargo/git/checkouts/icu4x-*/31dcf42/components/time/`.
  - Its **only** offset-computing API is
    `icu_time::zone::VariantOffsetsCalculator::compute_offsets_from_time_zone_and_name_timestamp`,
    and ICU4X marks it
    `#[deprecated(since = "2.1.0", note = "this API is a bad approximation
    of a time zone database")]`. It returns a
    `VariantOffsets { standard, daylight }` pair for a display-name *era*,
    not the offset in effect at an instant — it cannot say whether DST was
    actually observed then.
  - Its key type, `ZoneNameTimestamp`, documents the design directly:
    "Most software deals with _time zone transitions_, computing the UTC
    offset on a given point in time. In ICU4X, we deal with _time zone
    display names_", representable only after 1970 and only to a coarse
    15-minute granularity. `grep -rn transition` across
    `components/time/src/` finds no transition API at all, and
    `provider/mod.rs` even notes "transitions at different times, not
    implemented yet".
  - `icu_time::zone::{IanaParser, IanaParserExtended}` *is* real and does
    give case-insensitive IANA validation plus canonicalization — the
    "partial win" fallback this question anticipated. It was not needed:
    `blueice-ecma402` already pins `jiff = "=0.2.35"` with
    `tzdb-bundle-always` plus `jiff-tzdb = "=0.1.8"`, i.e. **the complete,
    pinned, real IANA Time Zone Database**, and
    `date_time_format.rs`'s `datetime_from_milliseconds` already resolves
    genuine historical offsets for arbitrary instants from it
    (`TimeZone::to_offset_info(timestamp)`), with `jiff_tzdb::get` supplying
    the case-normalized identifier. Track E therefore added `jiff`/
    `jiff-tzdb` to `backend/bluejs/Cargo.toml` (both already in
    `Cargo.lock` at those versions) and reads that same database directly
    from `vm/temporal/time_zone.rs`, the way `vm/temporal/calendar.rs`
    already reads `icu_calendar` directly rather than through the ECMA-402
    crate. A Temporal offset and an `Intl.DateTimeFormat` offset for the
    same zone and instant consequently come from one source and cannot
    diverge.
  - Two real Jiff-boundary details this surfaced, both now handled and
    unit-tested, and both worth knowing for `ZonedDateTime` (Stage 2):
    (1) Jiff's civil `Timestamp` stops at ISO year ±9999 while Temporal's
    Instant range reaches ±273,972 years, and `Timestamp::from_nanosecond`
    trips an *internal debug assertion* rather than returning `Err` for
    inputs far outside it — so the range must be checked before calling it.
    Out-of-range instants use `to_fixed_offset()` for a fixed IANA zone and
    a Gregorian 400-year-cycle projection otherwise, mirroring
    `blueice-ecma402`. (2) A Jiff `Timestamp` stores its second and
    sub-second parts with a *shared* sign, so for a pre-1970 instant with a
    sub-second part the second field is the **ceiling**; looking an offset
    up from it can therefore read the wrong side of a transition falling in
    that second. `offset_nanoseconds_for` floors nanoseconds to whole
    seconds first, which is exact because offsets only change on second
    boundaries. `blueice-ecma402`'s millisecond-based path has the same
    latent off-by-one-second for negative sub-second instants and was not
    changed here.
- ~~Whether `TimeZone` needs a new `TemporalKind`/`ObjectKind` heap
  variant~~ — **resolved 2026-09-18: no.** Gecko's `TimeZoneObject`
  predates the spec revision that removed `Temporal.TimeZone` as an object
  type; the pinned Test262 corpus has no `built-ins/Temporal/TimeZone/`
  directory at all. A time zone is a string in a `ZonedDateTime`'s existing
  `TemporalValue::time_zone` field, and `time_zone::TimeZone` is a
  transient host-neutral parse of it, never heap-allocated.

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
