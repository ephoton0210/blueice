# Phase 18 — BlueTS / BlueTSC completion worklist

[← Phase 18 plan](PLAN.md) · [integration contract](INTEGRATION_CONTRACT.md) · [test interface](TEST_INTERFACE.md) · [first DOM/event binding decision](../phase-13-bluejs-engine/DOM_EVENT_BINDINGS.md)

This file tracks work that is still needed to make BlueTS a usable BlueIce page language. [PLAN.md](PLAN.md) records scope, delivered foundations, and detailed evidence. Keep this worklist short: update an item when its acceptance condition changes or closes; put implementation history in the plan or commit message. A check mark needs a real acceptance test, not only an API or unit test.

## Current boundary

- The launcher-supervised child runs authorized JavaScript and supported BlueTS classic/module graphs in document order. The child has no source resolver or fetch fallback.
- The page host exposes copied document text and origin strings. It has no live DOM object, DOM mutation binding, or event listener surface.
- The debugger can pause, resume, and step a verified classic root; BlueTS source-span stepping is bounded to that root. Modules, nested frames, stack, scope, exception locations, and values are still outside this seam.
- The compiler/MCP route supports sealed startup projects and bounded read-only queries. It cannot accept client project registration, build output, or output writes.
- The script DOM dispatcher now checks the exact document generation on each request. Its socket still lacks child authentication and a session wait that can answer DOM calls while the child executes.

## Work order

Finish the P0 page runtime and its real-process tests before adding more static-metadata queries or TypeScript syntax. Add a debugger or compiler capability only when it directly closes an acceptance condition below. Keep page, compiler, debugger, and MCP authority separate.

**Next closure target:** one launcher/core/child HTTP fixture must perform a synchronous DOM lookup, change visible text, and reject an old-generation request after reload. Child authentication and the session-owned wait are the next implementation steps.

## P0 — make a supported page interactive

- [ ] **Connect the BlueJS child to the live core DOM (Phase 13).**
  Authenticate the child on the script socket, bind each call to its exact tab and document generation, and let the core session answer bounded synchronous DOM calls while it waits for the child. Keep the session thread as the sole owner of TabManager; a stale, cross-tab, unauthenticated, or old-core call must fail before mutation. Retain per-tab VM, source, program, bytecode, and child-wide limits.

  **Done when:** a real launcher/core/child HTTP page performs a DOM lookup and mutation without deadlock; navigation invalidates old calls and wrappers; denied calls leave the DOM unchanged. The existing generation check is one part of this item.

- [ ] **Install the first truthful DOM and event profile (Phases 2/13).**
  Implement the narrow document/node text and tree API, click listeners, and preventDefault behavior in the [binding decision](../phase-13-bluejs-engine/DOM_EVENT_BINDINGS.md). Keep JS node wrappers and listener roots in the child; core owns the DOM and click default action. Generate BlueTS declarations only for installed members and their exact capability policy. Do not add broad lib.dom declarations.

  **Done when:** a real page script changes rendered text and handles a click; a removed listener does not run; preventDefault suppresses link navigation; replacement clears listeners and wrappers. A supported BlueTS page uses the same host route. Unsupported members fail both static checking and runtime access.

- [ ] **Complete the native page debugger (Phase 17).**
  Preserve the delivered exact classic-root pause/resume, root-instruction step, and bounded BlueTS source-span step. Add module and nested-frame control, stack and scope inspection, exception locations, and bounded values through the native debugger channel, under explicit owner/client grants. Keep debugger control separate from DOM and network IPC.

  **Done when:** a real page test pauses at an original BlueTS source location, steps across nested/module code, inspects a bounded stack/scope/value, observes an exception location, and rejects stale, cross-tab, unauthorized, or over-budget requests without source or value leaks.

- [ ] **Bind BlueTS static debug information to every page lifecycle.**
  Retain compiler metadata only for its exact live program and source set. Complete original source locations and privacy policy for modules, nested code, cache eviction, and hibernation; invalidation must never attach old metadata to a successor. Static types must remain distinct from runtime values.

  **Done when:** real-process tests cover source breakpoints, stack locations, symbol navigation, and type display on a live page, then prove reload, tab close, eviction, and hibernation either invalidate or correctly restore the same checked generation.

- [ ] **Close the direct-page regression matrix.**
  Use real core, child, debugger, and MCP boundaries where applicable. Cover classic and ESM BlueTS, type-only import elision, resolver identity, source mapping, contracts, tab resource attribution, policy isolation, multiple tabs, and reload.

  **Done when:** these fixtures exercise visible page behavior and the public boundaries; no required assertion is satisfied only by a host-neutral unit test.

## P1 — strict boundaries and native compiler

- [ ] **Finish the live contract inventory.**
  The current inventory names only the two copied host-to-script string results. For each newly implemented ingress or egress, record its owner, source location when one exists, contract ID, limits, failure category, and capability policy. Do not invent contracts for absent Fetch, storage, messaging, DOM, or extension APIs.

  **Done when:** strict-runtime rejects an implemented boundary with no reifiable or reviewed contract; checked and transpile-only policies remain observably distinct.

- [ ] **Enforce contracts on every declared live boundary.**
  Run the bounded pure validator before values cross into the VM; attribute validator and cache costs to the initiating tab. The two copied strings are already validated. Extend this to actual mutable and foreign-data boundaries as they arrive.

  **Done when:** malformed, cyclic, deep, oversized, and resource-heavy data fail within fixed budgets; valid data reaches BlueJS once; failures identify policy and source position without disclosing protected content.

- [ ] **Preserve strict contracts in BlueTSC output.**
  Standalone strict-runtime build currently rejects output because no versioned runtime helper is emitted. Add the helper, include its identity in the artifact manifest, and reject targets that erase a required check.

  **Done when:** direct-page and emitted ESM runs reject the same malformed boundary value; weaker artifacts cannot claim strict-runtime policy.

- [ ] **Complete the registered-project compiler service.**
  Keep owner registration sealed before listeners and keep check read-only. Add independently authorized project input, generation/fingerprint-bound build artifacts, atomic no-emit-on-error output, and explicit output-write elevation. Clients must not extend roots, source graphs, resolution, options, or plugins through query IPC.

  **Done when:** real-process check/build succeeds for an authorized project, emits nothing on error, and rejects guessed/private projects, stale generations, unauthorized inputs, escaped output paths, and oversized artifacts.

- [ ] **Complete the negotiated MCP compiler adapter (Phase 12).**
  Preserve the delivered same-stream receipts and bounded diagnostic/work-set/static-metadata pages. Add build only with explicit write authority, complete per-client project authorization, result pagination, untrusted-content handling, and capability/session negotiation.

  **Done when:** an MCP client can inspect an authorized page diagnostic and static type/contract failure, then check/build an authorized project; arbitrary paths, stale handles, private projects, untrusted page strings, and unauthorized writes fail safely.

## Release gate

- [x] Generated and verified host typings for the existing installed snapshot profiles.
- [x] Direct BlueTS-to-BlueJS bridge connected to page declaration loading.
- [x] Generation-bound BlueJS source identities and the verified BlueTS-to-safe-point map.
- [x] Pinned TypeScript 5.9.3 oracle runs as a required CI job.
- [ ] **Re-run all required quality gates after P0/P1 closure.** Run workspace tests, formatting, all-target Clippy with warnings as errors, real-process suites, the pinned TypeScript oracle, and the applicable coverage gate. Do not hide a failing required test or warning with an exclusion.

## P2 — compatibility after the page gate

- [ ] Add control flow, functions, narrowing, and overload behavior only with matching BlueJS execution and source-location semantics.
- [ ] Add expressions and runtime lowering one form at a time; document rejection rules and test parser/checker/emitter, direct execution, provenance, contracts, and the TypeScript oracle. Never reparse emitted JavaScript as a fallback.
- [ ] Decide separately whether classes, enums, decorators, namespaces, JSX/TSX, CommonJS, package resolution, remote declarations, transformers, or full web typings belong in the product. Each proposal needs runtime, authority, debugger, contract, and conformance evidence.

Phase 18 does not close the rest of Phase 13 ECMAScript conformance, Phase 17 automation/AJAX, or unrelated Phase 12 MCP families.
