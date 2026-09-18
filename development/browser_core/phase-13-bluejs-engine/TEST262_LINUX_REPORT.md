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
