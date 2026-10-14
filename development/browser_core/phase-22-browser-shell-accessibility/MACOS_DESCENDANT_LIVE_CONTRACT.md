# macOS descendant live-region semantics

This increment follows retained announcement delivery (`eab165669`). It is implemented and accepted;
test-first failures and complete native/workspace evidence are recorded in
[the results](MACOS_DESCENDANT_LIVE_RESULTS.md).

WAI-ARIA 1.2 makes the nearest explicit `aria-atomic` setting decisive: false
limits the update to changed content, while true includes the containing
element's public contents and author label. Descendant `aria-relevant` replaces
the inherited set; absent values inherit the nearest defined ancestor or the
additions/text default. See [aria-atomic](https://www.w3.org/TR/wai-aria-1.2/#aria-atomic)
and [aria-relevant](https://www.w3.org/TR/wai-aria-1.2/#aria-relevant).

- Each observed contributor retains its effective relevant set and nearest atomic
  boundary. Removed contributors use their previously observed relevant scope.
- Changes activate an atomic group once. Explicit false stops outer expansion;
  nested live owners keep independent contents and delivery identity.
- Public `aria-labelledby` references precede `aria-label`. Names carry source
  provenance so subsequent private/hidden transitions purge queued text. External
  label changes update future expansions without generating a live event alone.
- Hidden, inert, protected and live-off content cannot contribute live text.
  Author references use the repository's public-content policy, including its
  exclusion of hidden references, rather than the complete AccName algorithm.
- Busy regions retain the last ready baseline, then coalesce current contents and
  labels. Suppressed changes advance the baseline without replay on re-enabling.
- Traversal, group count, public text and retained event count are bounded. Unicode
  clipping stays attached to the retained event until its prefix is acknowledged.

Regression coverage includes child true/false boundaries, multiple changes in one
group, descendant text/removal overrides, suppressed edits, labels/privacy,
external-reference mutations, busy coalescing and retained clipping. Acceptance
requires actual-core IPC, AppKit/XCUITest, the complete native suite, Rust gates
and the complete workspace on unchanged application/core/native-test inputs.
The results record the later MCP `cfg(test)` fixture-only source exception.

This increment does not establish physical VoiceOver speech, the complete ARIA
name algorithm or completion of the remaining browser delivery milestones.
