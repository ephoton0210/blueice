# Defu multi-source state cross-use evaluation

The current state policy passes the frozen two-profile, two-direction matrix:
92 required checks and three owner-path rejection checks. Accepted compiler
source: backend-tree:afe0fa1eb02894b7d618fc90edaad8af4bee46b4. This evaluation required no incremental compiler
extension or broader state authority.

[Matrix](../../../differential-testing/bluets-projects/defu-interchange.json) ·
[measurement](DEFU_INTERCHANGE_MEASUREMENT.json) ·
[raw evidence](DEFU_INTERCHANGE_ARTIFACTS.json) ·
[compiler and complete gates](DEFU_REPAIR_ACCEPTANCE.md)

| Configured source discovery | Direction | Observed handoff |
| --- | --- | --- |
| All three production sources are roots | Native → BlueTSC | Up to date; preserves every verified donor artifact byte |
| All three production sources are roots | BlueTSC → Native | Rebuilds; complete inventory equals Native clean output |
| One root with two transitive imports | Native → BlueTSC | Rebuilds from current owner inputs; complete inventory equals BlueTSC clean output |
| One root with two transitive imports | BlueTSC → Native | Rebuilds; complete inventory equals Native clean output |

Each workflow first verifies a separate fresh Native build against the earlier
frozen full-project artifact inventory. Donor declarations and runtime are
checked before the receiver runs. Every direction separately checks exact
declarations, twelve runtime scenarios, positive/negative consumers, warm
stability, imported implementation edits, exported-type edits with changed
dependent-consumer verdicts, dependency addition/missing/restoration, exact
edited-clean inventories and recovery from corrupt/missing public state.

External configured state paths and input/output collisions are rejected while
all project inputs and the outside sentinel remain unchanged. Public state
presence, bytes and SHA-256 are retained separately; other public artifacts are
not filtered out of the comparisons.

The earlier single-source measurements did not characterize all admitted-root
compositions. This matrix establishes the measured three-root case while
retaining the existing rebuild for a transitive source outside the selected
root set. Successful consumption alone does not establish a Native warm no-op,
portable state bytes or arbitrary-project compatibility. Other unmeasured
profiles and the broad G-M1/G-M2 rows remain open.
