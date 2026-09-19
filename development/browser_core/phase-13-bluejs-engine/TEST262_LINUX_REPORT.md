# Ubuntu Test262 Report

## Current complete inventory (2026-09-19)

The test host is **Ubuntu 24.04.3 LTS under WSL2**,
`x86_64-unknown-linux-gnu`, with Rust/Cargo 1.95.0 and 12 logical CPUs. It is
not Ubuntu 24.04.4. The command
`python3 backend/bluejs/test262/run.py --jobs 8` completed the pinned,
unfiltered inventory in 685.257 seconds. Adapter SHA-256:
`0e903991578a48bcb2d8cfbb606fb0da6fa4e9e5b9c1665b6d9e85b901a8bd96`.
Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`; the scope includes
`main`, proposals and staging. The 24.04.4-labelled historical result is
superseded by this actual 24.04.3 run.

macOS and Windows are explicitly deferred until those environments are
available. No Ubuntu result is used as a substitute for their validation.

Test262 has no official “Core” classification. This report defines **ECMA-262 Core** as `language/` + `built-ins/`; **complete ECMA-262 Test262 scope** as Core + `annexB/` + `staging/`; and **ECMA-402** as `intl402/`. `harness/` tests runner support code, so it appears only in the all-inventory total. Every scope is derived from this one unfiltered complete run, not a separately filtered invocation. “Complete” is an inventory scope, not a claim that time-based proposal/staging content belongs to one published ECMA edition.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 86,304 | 5,509 | 7 | 93.993% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 89,631 | 6,340 | 9 | 93.385% |
| ECMA-402 (`intl402/`) | 6,714 | 6,364 | 350 | 0 | 94.787% |
| Test262 harness support (`harness/`) | 232 | 208 | 24 | 0 | 89.655% |
| All Test262 runner modes | 102,926 | 96,203 | 6,714 | 9 | 93.468% |

| Top-level Test262 group | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 42,604 | 1,893 | 0 | 95.746% |
| `built-ins/` | 47,323 | 43,700 | 3,616 | 7 | 92.344% |
| `annexB/` | 1,377 | 1,173 | 204 | 0 | 85.185% |
| `staging/` | 2,783 | 2,154 | 627 | 2 | 77.398% |
| `intl402/` | 6,714 | 6,364 | 350 | 0 | 94.787% |
| `harness/` | 232 | 208 | 24 | 0 | 89.655% |

The run completed all 53,582 test files. Its JSONL contains nine `timeout`
records and zero `harness_error` records. The 6,714 retained failures are
semantic outcomes, not filtered or recategorized timeouts.

## ECMA-402 and Temporal breakdown (same 2026-09-19 complete run)

The tables above report `intl402/` as one aggregate row. This section breaks
that 94.787% down by service and adds ECMA-262 Temporal's own test surface,
which `built-ins/` also includes above without a separate line. These figures
are derived from the same complete JSONL as the tables above, not a filtered
rerun. Reproduce with `python3 backend/bluejs/test262/run.py --jobs 8`, then
group `results.jsonl` by `path.split("/")[1]` (or `[2]` for the Temporal
sub-breakdown).

| `intl402/` group | Modes | Pass | Fail | Pass rate |
| --- | ---: | ---: | ---: | ---: |
| `Temporal/` | 4,058 | 3,708 | 350 | 91.375% |
| `NumberFormat/` | 498 | 498 | 0 | 100% |
| `DateTimeFormat/` | 488 | 488 | 0 | 100% |
| `Locale/` | 336 | 336 | 0 | 100% |
| `DurationFormat/` | 220 | 220 | 0 | 100% |
| `ListFormat/` | 162 | 162 | 0 | 100% |
| `RelativeTimeFormat/` | 160 | 160 | 0 | 100% |
| `Segmenter/` | 158 | 158 | 0 | 100% |
| `Intl/` | 132 | 132 | 0 | 100% |
| `Collator/` | 130 | 130 | 0 | 100% |
| `DisplayNames/` | 114 | 114 | 0 | 100% |
| `PluralRules/` | 106 | 106 | 0 | 100% |
| `intl402/*.js` (top-level) + `String/`/`Date/`/`BigInt/`/`Number/`/`Array/`/`FallbackSymbol/`/`TypedArray/` (`toLocale*`/`localeCompare`) | 152 | 152 | 0 | 100% |
| **Total** | **6,714** | **6,364** | **350** | **94.787%** |

Every non-`Temporal/` group is at 100%. `Temporal/` is ECMA-262, not an
ECMA-402 service — see [Phase 26's plan](../phase-26-ecma262-temporal/PLAN.md)
rather than [Phase 25's](../phase-25-ecma402-internationalization/PLAN.md).

`built-ins/Temporal/` (9,210 modes, part of this report's `built-ins/` row
above but not broken out there) is Temporal's actual primary test surface.
Combined with `intl402/Temporal/`, the true Temporal denominator is
**13,268 modes, 12,682 passing (95.583%)**, per type:

| Temporal type | Combined modes (`built-ins/` + `intl402/`) | Combined pass | Pass rate |
| --- | ---: | ---: | ---: |
| `ZonedDateTime` | 2,968 | 2,782 | 93.733% |
| `PlainDateTime` | 2,512 | 2,366 | 94.188% |
| `PlainDate` | 2,290 | 2,178 | 95.109% |
| `Duration` | 1,122 | 1,100 | 98.039% |
| `PlainYearMonth` | 1,672 | 1,582 | 94.617% |
| `PlainTime` | 1,010 | 1,010 | 100% |
| `Instant` | 968 | 968 | 100% |
| `PlainMonthDay` | 578 | 548 | 94.810% |
| `Now` | 138 | 138 | 100% |
| `Temporal/` root files | 10 | 10 | 100% |
| **Total** | **13,268** | **12,682** | **95.583%** |

This full run confirms that the current Temporal implementation has moved well
beyond the earlier read-only formatting slice. The remaining 586 combined
Temporal failures are concentrated in the documented Phase 26 follow-up work;
they are not evidence that Temporal is absent. This Ubuntu-only breakdown has
not yet been reproduced on macOS or Windows; do not assume platform parity
until it has.
