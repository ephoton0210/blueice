# Phase 26 — ECMA-262 Temporal

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Design — this is the first version of this phase's plan, written
2026-09-17 before any of its own implementation work started. It exists
because completing Phase 25 (ECMA-402) surfaced a real gap: `intl402/`'s
`Temporal/` subtree is 4,058 of the full 6,714 modes (60%+ of the corpus) and
sits at 6.55% pass — see
[Phase 25's `CONFORMANCE.md`](../phase-25-ecma402-internationalization/CONFORMANCE.md#reproducible-current-inventory)
for the exact per-type breakdown. Temporal is **ECMA-262** (a core language
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

- [ ] `iso.rs`: `ISODate`/`Time`/`ISODateTime` records; full ISO 8601 grammar
      parser (dates, times, datetimes, durations, instant/zoned-datetime
      strings with calendar/time-zone annotations) — TDD against Test262's
      own ISO-string-parsing fixtures before any per-type work starts.
- [ ] `epoch.rs`: epoch-nanosecond representation (`i128`, not a bespoke
      bigint — this is materially simpler than Gecko's own `Int96`, since
      Rust has a native 128-bit integer type Gecko's C++ baseline did not).
- [ ] `rounding.rs`: `TemporalUnit`, `TemporalRoundingMode` enums and their
      tables.
- [ ] `duration_math.rs`: `InternalDuration`/`TimeDuration`/`DateDuration`
      internal combinators and the balancing/rounding algorithms every
      arithmetic operation in every later stage calls into. Per Gecko, this
      is not one monolithic `Balance()` — it is many focused per-operation
      functions (`Duration.cpp` is Gecko's single largest file at 4,229
      lines) sharing the same internal record shapes.
- [ ] Locate (or build, if genuinely absent) the ISO 8601 duration parser
      `Intl.DurationFormat` already appears to depend on — resolve the open
      item above before assuming this needs to be built from scratch here.
- [ ] `heap.rs`: confirm/extend `TemporalKind` if any Stage 0 record needs
      direct heap representation (most of Stage 0 is plain Rust values held
      inside the existing per-type `TemporalKind` payloads, not new heap
      object kinds).

### Stage 1 — parallel tracks (worktree-isolated agents, after Stage 0 lands)

Each track's Gecko evidence for independence is stated explicitly so this
isn't an assumption:

- **Track A — Calendar systems** (`calendar.rs`, split further by calendar
  cluster if useful, e.g. one agent for the three `Islamic*` variants
  together since they share era logic, one for `Chinese`/`Dangi`, one for
  the rest). Evidence: `CalendarId` is a closed enum dispatching to
  independent per-calendar conversion logic (Gecko's `Calendar.cpp`, 4,016
  lines, is a dispatch service, not per-type logic); this codebase already
  proved the same calendars work through `icu_calendar` for DateTimeFormat,
  so most of this track is wiring existing ICU4X calendar math into the
  `CalendarFields` foundation and validating against Test262's per-calendar
  fixtures, not new algorithm design.
- **Track B — Duration arithmetic** (`duration.rs`, the JS-visible wrapper;
  building on Stage 0's `duration_math.rs`). Evidence: Gecko's core
  add/subtract/negate/abs/compare path does not depend on `Calendar.cpp`
  except for calendar-aware rounding against an optional `relativeTo` —
  calendar-independent arithmetic can be built and tested (Test262's
  `intl402/Temporal/Duration/`, 42 modes) before Track A finishes.
- **Track C — Instant + Now** (`instant.rs`, `now.rs`). Evidence: epoch
  nanoseconds are calendar-agnostic by construction; Gecko's `Instant.cpp`
  has no calendar dependency. Smallest track (Test262: `Instant/` 34 modes,
  `Now/` 6 modes) — a reasonable first track to land as a template for the
  others' Test262-driven TDD rhythm.
- **Track D — PlainTime** (`plain_time.rs`). Evidence: time-of-day has no
  calendar-field dependency; Gecko's `PlainTime.cpp` (1,644 lines) is the
  smallest of the calendar-adjacent per-type files. Test262: `PlainTime/` 24
  modes.
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

- [ ] `plain_date.rs`, `plain_date_time.rs` first (Test262: `PlainDate/` 986,
      `PlainDateTime/` 966 modes — the two largest non-`ZonedDateTime` types).
- [ ] `plain_year_month.rs`, `plain_month_day.rs` next, reusing the
      calendar-field pattern `plain_date.rs` establishes (Test262:
      `PlainYearMonth/` 654, `PlainMonthDay/` 180 modes).
- [ ] `zoned_date_time.rs` last — composes `PlainDateTime` + `TimeZone` +
      `Instant`, so it must come after all three are solid. Gecko's largest
      per-type file (`ZonedDateTime.cpp`, 3,180 lines) and Test262's largest
      single group (1,166 modes) — expect this to be the longest-running
      single piece of work in the phase.

### Stage 3 — Test262-evidence closure and coverage

- [ ] Re-run `intl402/Temporal/` after each stage lands, tracked per-type
      against the 2026-09-17 baseline in Phase 25's `CONFORMANCE.md`
      (`ZonedDateTime` 32/1,166, `PlainDate` 100/986, `PlainDateTime`
      66/966, `PlainYearMonth` 18/654, `PlainMonthDay` 48/180, `Duration`
      2/42, `Instant`/`PlainTime`/`Now` 0/34, 0/24, 0/6).
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
