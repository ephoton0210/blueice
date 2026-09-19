# macOS Test262 Report

Timeout-remediation working-tree run. Adapter SHA-256: `3e7f32efcbe10c8f0d0d8bd2a3dc3b9aebce5f899e885d7d00d38ca2f6c17883`. Platform: macOS 26.6.2 arm64. Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`; unfiltered `main` including proposals and staging; eight jobs selected by the portable `min(8, logical CPUs)` default.

Test262 has no official “Core” classification. This report defines **ECMA-262 Core** as `language/` + `built-ins/`; **complete ECMA-262 Test262 scope** as Core + `annexB/` + `staging/`; and **ECMA-402** as `intl402/`. `harness/` tests runner support code, so it appears only in the all-inventory total. Every scope is derived from this one unfiltered complete run, not a separately filtered invocation. “Complete” is an inventory scope, not a claim that time-based proposal/staging content belongs to one published ECMA edition.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| ECMA-262 Core (`language/` + `built-ins/`) | 91,820 | 75,741 | 16,079 | 0 | 82.489% |
| Complete ECMA-262 Test262 scope (Core + `annexB/` + `staging/`) | 95,980 | 78,678 | 17,302 | 0 | 81.973% |
| ECMA-402 (`intl402/`) | 6,714 | 2,922 | 3,792 | 0 | 43.521% |
| Test262 harness support (`harness/`) | 232 | 206 | 26 | 0 | 88.793% |
| All Test262 runner modes | 102,926 | 81,806 | 21,120 | 0 | 79.480% |

| Top-level Test262 group | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `language/` | 44,497 | 41,884 | 2,613 | 0 | 94.128% |
| `built-ins/` | 47,323 | 33,857 | 13,466 | 0 | 71.544% |
| `annexB/` | 1,377 | 1,173 | 204 | 0 | 85.185% |
| `staging/` | 2,783 | 1,764 | 1,019 | 0 | 63.385% |
| `intl402/` | 6,714 | 2,922 | 3,792 | 0 | 43.521% |
| `harness/` | 232 | 206 | 26 | 0 | 88.793% |

The run completed all 53,582 test files in 281.516 seconds. Its JSONL contains zero `timeout` and zero `harness_error` records. The 21,120 retained failures are semantic outcomes, not filtered or recategorized timeouts.
