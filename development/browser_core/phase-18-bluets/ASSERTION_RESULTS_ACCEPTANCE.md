# Assertion result semantics acceptance

P0-J preserves the asserted semantic type while retaining readonly and opaque runtime provenance. Method calls, indexed values, async/generator results and logical joins keep those two concerns separate. JavaScript, declarations and complete source maps match the frozen TypeScript 5.9.3 expectations.

Source: `backend-tree:fd642c71683fbd992d1319e5598c51f5379dc766`. The complete frozen runtime and independently published-byte requirements pass. The broader22 feature rows remain open and full TypeScript parity remains OFF.

| Actual compiler profile | SHA256 | Producers | Consumers | Executions |
| --- | --- | ---: | ---: | ---: |
| native | `a15efd2650d97411931408b4037883bc157acf792b35883606760b1d8ef6b2af` | 1,296 | 6,144 | 768 |
| workspace | `a4085867a1e4a6c2ed7c6705ffaa9d31030f2bcc22a61145a11b1548ef5155fa` | 1,296 | 6,144 | 768 |
| standalone | `b2d09d2f09292be4f7640fdfdf9c39f2909f38dd4490cbb144712c34dad6d830` | 1,296 | 6,144 | 768 |

Each profile independently passes816 semantic and480 owner observations,1,008 exact declarations, 2,016 complete maps /31,116 segments and1,008 syntax checks. All384 owner negatives refuse at the authored mutation; all96 positive owner controls pass. All four declaration-consumer directions pass.

- Complete Native catalog:193 suites /1,113 functions; zero failed, ignored or filtered tests.
- Complete workspace: 8,081 passed /0 failed /402 existing or explicitly Native-only ignores /0 filters; all738 compiled executables ran.
- Both map profiles:55 original families /298 functions,8,407 configurations twice and572 original Native corrections per actual compiler.
- Both program-root profiles: all14 steps,512 primary cases,528 library/persistent states,760 maps each,108 publication states and24 tamper controls; all four original projects pass.
- Formatting, workspace all-target Clippy, all319 Node tests, Python, accounting and status gates pass; all586 production modules remain below1,500 lines.
- Windows x86_64:77 complete cohorts,82 original core oracles and25 path cases; actual PE, full source and raw logs independently verified.
- All43,276 old complete case objects and assertions remain intact;216 new primary/control cases bring the total to43,492. All12,224 old fixture paths remain; two inventory adapters only gain routes and the explicitly approved pure-return expectation/digest are updated. All621 original tool names remain;619 original tool files retain their bytes, and two exact virtual-host repairs are derived and recorded in ASSERTION_RESULTS_ORACLE_HOST_CORRECTIONS.json.

[Frozen contract](ASSERTION_RESULTS_CONTRACT.json) · [Approved amendment](ASSERTION_RESULTS_POLICY_AMENDMENT.json) · [Measurements](ASSERTION_RESULTS_MEASUREMENT.json) · [Independent acceptance audit](ASSERTION_RESULTS_ACCEPTANCE_AUDIT.json) · [All source and raw artifacts](ASSERTION_RESULTS_ARTIFACTS.json)

The Native host initially deadlocked during Node26 Maglev shutdown. Its failed report and stack sample are retained. The final catalog preserves90 successful cohorts unchanged and fully runs the remaining103 cohorts under the acceptance-scoped `--no-maglev` launcher, including all five original class-checker tests and523 pinned verdicts. A separate audit verifies the exact193-cohort /1,113-function union, every compiled test name and log digest; no failed, filtered or ignored result is credited. The optimizer flag changes no compiler source, fixture or expected verdict.

An initial complete workspace attempt ran in a restricted sandbox which denied local Unix socket binding: 7,841 passed,240 failed and402 existing ignores. That complete failed attempt is retained. The identical local IPC probe succeeds on the corrected test host and in its actual controller. The original complete workspace command then runs afresh with all738 compiled test executables and no failed result carried over. Source, fixture expectations and acceptance thresholds remain frozen throughout.

The artifact index includes complete file and fragment ledgers. Large files span bounded archives; independent verification reconstructs every original file digest. Recorded symlinks remain metadata and are never followed.

The index, audit and measurement retain the pre-commit state recorded at their timestamps. The [completed TODO item](TODO.md) records the final reviewed package status.
