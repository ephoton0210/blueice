# Linux Test262 Report

Engine run: `fccb017`. Platform: Ubuntu 24.04.4 LTS x86_64. Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`; unfiltered `main` including proposals and staging; eight workers.

Test262 has no official “Core” classification. This report defines **ECMA-262 Core** as `language/` + `built-ins/`; **complete ECMA-262 Test262 scope** as Core + `annexB/` + `staging/`; and **ECMA-402** as `intl402/`. `harness/` tests runner support code, so it appears only in the all-inventory total. Every scope is derived from this one unfiltered complete run, not a separately filtered invocation. “Complete” is an inventory scope, not a claim that time-based proposal/staging content belongs to one published ECMA edition.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 75,680 | 16,095 | 45 | 82.422% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 78,600 | 17,318 | 62 | 81.892% |
| ECMA-402 (`intl402/`) | 6,714 | 2,912 | 3,792 | 10 | 43.372% |
| Test262 harness support (`harness/`) | 232 | 206 | 26 | 0 | 88.793% |
| All Test262 runner modes | 102,926 | 81,718 | 21,136 | 72 | 79.395% |

| Top-level Test262 group | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 41,884 | 2,613 | 0 | 94.128% |
| `built-ins/` | 47,323 | 33,796 | 13,482 | 45 | 71.416% |
| `annexB/` | 1,377 | 1,173 | 204 | 0 | 85.185% |
| `staging/` | 2,783 | 1,747 | 1,019 | 17 | 62.774% |
| `intl402/` | 6,714 | 2,912 | 3,792 | 10 | 43.372% |
| `harness/` | 232 | 206 | 26 | 0 | 88.793% |

The run completed all 53,582 test files in 805.726 seconds. The additional timeouts compared with macOS are recorded as results, not discarded.

## ECMA-402 and Temporal breakdown (this Linux/WSL environment, 2026-09-17)

The tables above report `intl402/` as one aggregate row. This section breaks
that 43.372% down by service and adds ECMA-262 Temporal's own test surface,
which `built-ins/` also silently includes above without a separate line.
**This section is a supplementary, filtered rerun on this development
environment's own working tree** (engine build after `fccb017`, including
this session's `Intl.PluralRules.prototype.selectRange` fix and in-progress
`Temporal` string-parsing work) — it is not the pinned unfiltered `fccb017`
run the tables above report, and its `intl402/` totals may differ from that
run by a few modes due to ordinary run-to-run timeout-classification
variance. Reproduce with `python backend/bluejs/test262/run.py --filter
"intl402/" --jobs 8` and `--filter "built-ins/Temporal/" --jobs 8`, grouping
`results.jsonl` by `path.split("/")[1]` (or `[2]` for the `Temporal/`
sub-breakdown).

| `intl402/` group | Modes | Pass | Fail | Pass rate |
| --- | ---: | ---: | ---: | ---: |
| `Temporal/` | 4,058 | 266 | 3,792 | 6.55% |
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
| **Total** | **6,714** | **2,922** | **3,792** | **43.52%** |

Every group other than `Temporal/` is at 100%. `Temporal/` is ECMA-262, not
an ECMA-402 service — see
[Phase 26's plan](../phase-26-ecma262-temporal/PLAN.md) rather than
[Phase 25's](../phase-25-ecma402-internationalization/PLAN.md).

`built-ins/Temporal/` (9,210 modes, part of this report's `built-ins/` row
above but not broken out there) is Temporal's actual primary test surface.
Combined with `intl402/Temporal/`, the true Temporal denominator is
**13,268 modes, 1,592 passing (12.00%)**, per type:

| Temporal type | Combined modes (`built-ins/` + `intl402/`) | Combined pass | Pass rate |
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

Today's Temporal slice (`backend/bluejs/src/vm/temporal.rs`) is read-only
construction plus one-way `Intl.DateTimeFormat` formatting; no arithmetic
(`add`/`subtract`/`until`/`since`/`compare`/`round`/`equals`/`toString`/etc.)
exists yet, which is what these pass rates reflect. This Linux-only
breakdown has not yet been reproduced on macOS or Windows; do not assume
platform parity until it has.
