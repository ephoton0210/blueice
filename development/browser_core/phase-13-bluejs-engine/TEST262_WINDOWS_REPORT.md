# Windows Test262 Report

Engine run: `fccb017`; the following `7630360` runner-only change fixed Windows `.exe` regex-worker summary hashing and passed a real-adapter summary smoke test. Platform: Windows 11 24H2 x86_64, four-vCPU KVM VM. Test262 revision: `72faf8ec1445c55149615e8b35187830783aba1a`; unfiltered `main` including proposals and staging; eight workers.

| Scope | Scheduled | Pass | Fail | Timeout | Pass rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| All Test262 | 102,926 | 81,077 | 21,041 | 808 | 78.772% |
| ECMA-402 (`intl402/`) | 6,714 | 2,802 | 3,776 | 136 | 41.734% |
| Other Test262 groups | 96,212 | 78,275 | 17,265 | 672 | 81.357% |

The full JSONL contains 102,926 modes and zero `harness_error` records. Its originally completed run hit a report-only `bluejs-regexp-worker.exe` hash path error before `summary.json` creation; `7630360` resolves that Windows suffix handling and its real `.exe` smoke run successfully wrote a summary. The higher timeout count is expected for this CPU-bound inventory on the four-vCPU VM and remains part of the rate denominator.
