# macOS Test262 Report

Engine run: `fccb017`. Platform: macOS 26.6.2 arm64. Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`; unfiltered `main` including proposals and staging; eight workers.

Test262 has no official “Core” classification. This report defines **ECMA-262 Core** as `language/` + `built-ins/`; **complete ECMA-262 Test262 scope** as Core + `annexB/` + `staging/`; and **ECMA-402** as `intl402/`. `harness/` tests runner support code, so it appears only in the all-inventory total. Every scope is derived from this one unfiltered complete run, not a separately filtered invocation. “Complete” is an inventory scope, not a claim that time-based proposal/staging content belongs to one published ECMA edition.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 75,683 | 16,097 | 40 | 82.425% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 78,612 | 17,328 | 40 | 81.905% |
| ECMA-402 (`intl402/`) | 6,714 | 2,922 | 3,792 | 0 | 43.521% |
| Test262 harness support (`harness/`) | 232 | 206 | 26 | 0 | 88.793% |
| All Test262 runner modes | 102,926 | 81,740 | 21,146 | 40 | 79.416% |

| Top-level Test262 group | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 41,884 | 2,613 | 0 | 94.128% |
| `built-ins/` | 47,323 | 33,799 | 13,484 | 40 | 71.422% |
| `annexB/` | 1,377 | 1,173 | 204 | 0 | 85.185% |
| `staging/` | 2,783 | 1,756 | 1,027 | 0 | 63.097% |
| `intl402/` | 6,714 | 2,922 | 3,792 | 0 | 43.521% |
| `harness/` | 232 | 206 | 26 | 0 | 88.793% |

The run completed all 53,582 test files in 260.041 seconds. Failures and timeouts are retained, not filtered.
