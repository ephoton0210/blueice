# Windows Test262 Report

Engine run: `fccb017`; the following `7630360` runner-only change fixed Windows `.exe` regex-worker summary hashing and passed a real-adapter summary smoke test. Platform: Windows 11 24H2 x86_64, four-vCPU KVM VM. Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`; unfiltered `main` including proposals and staging; eight workers.

Test262 has no official “Core” classification. This report defines **ECMA-262 Core** as `language/` + `built-ins/`; **complete ECMA-262 Test262 scope** as Core + `annexB/` + `staging/`; and **ECMA-402** as `intl402/`. `harness/` tests runner support code, so it appears only in the all-inventory total. Every scope is derived from this one unfiltered complete run, not a separately filtered invocation. “Complete” is an inventory scope, not a claim that time-based proposal/staging content belongs to one published ECMA edition.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 75,274 | 16,066 | 480 | 81.980% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 78,079 | 17,240 | 661 | 81.349% |
| ECMA-402 (`intl402/`) | 6,714 | 2,802 | 3,776 | 136 | 41.734% |
| Test262 harness support (`harness/`) | 232 | 196 | 25 | 11 | 84.483% |
| All Test262 runner modes | 102,926 | 81,077 | 21,041 | 808 | 78.772% |

| Top-level Test262 group | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 41,877 | 2,611 | 9 | 94.112% |
| `built-ins/` | 47,323 | 33,397 | 13,455 | 471 | 70.572% |
| `annexB/` | 1,377 | 1,149 | 204 | 24 | 83.442% |
| `staging/` | 2,783 | 1,656 | 970 | 157 | 59.504% |
| `intl402/` | 6,714 | 2,802 | 3,776 | 136 | 41.734% |
| `harness/` | 232 | 196 | 25 | 11 | 84.483% |

The full JSONL contains 102,926 modes and zero `harness_error` records. Its originally completed run hit a report-only `bluejs-regexp-worker.exe` hash path error before `summary.json` creation; `7630360` resolves that Windows suffix handling and its real `.exe` smoke run successfully wrote a summary. The higher timeout count is expected for this CPU-bound inventory on the four-vCPU VM and remains part of the rate denominator.
