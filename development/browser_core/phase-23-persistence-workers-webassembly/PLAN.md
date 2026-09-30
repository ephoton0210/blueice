# Phase 23 — Persistence, Workers, Offline Platform, and WebAssembly

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Proposed — BlueIce has no web storage, worker realm, offline interception, PWA lifecycle or WebAssembly host today.

## Objective

Add persistent and concurrent web-platform capabilities without weakening origin isolation, process containment or the one-core observation model. This includes `localStorage`, `sessionStorage`, IndexedDB, Cache API, Web/Shared/Service Workers, offline/PWA lifecycle, WebAssembly and the corresponding debugging/MCP surface.

## Decisions

### Storage, cache and offline ownership

`core` owns an encrypted-at-rest, partitioned `StorageKey` model derived from Phase 20's origin/site/top-level context/private profile. Cookies remain request-service state; Web Storage, IndexedDB, Cache API, File System Access handles and service-worker registrations use distinct quotas, transaction/locking rules, clear-site-data eviction and private-profile lifetime. An extension, worker, page, API workspace or MCP client never receives a raw database/filesystem path.

Service Workers are isolated worker realms registered only through the standard secure-context/lifecycle path. Their fetch interception is a typed `blueice-net` decision before network I/O, preserves CSP/CORS/origin/cache policy and is auditable by the gatekeeper; it is not an alternate unrestricted socket. Offline manifests, installability, push/background sync/periodic work and notification delivery are explicit feature profiles with user permission, storage/network/power budgets and deterministic termination/restart semantics.

### Workers and structured concurrency

Dedicated, shared and service workers run in isolated `blueice-worker` processes or sandboxes, each with its origin, agent cluster, realm generation, event loop, import/module policy and resource account. `postMessage`, `MessagePort`, `BroadcastChannel`, transferable objects, SharedArrayBuffer/Atomics and structured clone follow a single versioned serializer. A worker cannot access a DOM; DOM-affecting work returns through standard messages to its owning page. Worker startup, messaging, shutdown, crash and blocked network/permission events are visible to the page, DevTools and MCP without exposing another process's heap.

### Web Crypto and protected key material

Implement the Web Crypto API as a separate `blueice-crypto` service with standards-profiled `SubtleCrypto`, `CryptoKey`, secure random generation, digest, HMAC, AES, RSA/ECDSA/ECDH and HKDF/PBKDF2 operations only where a published algorithm/curve/key-size matrix and platform cryptography review permit them. Keys are origin/profile-bound, have immutable extractable/usages metadata and use an OS-backed or encrypted key store when persistence is allowed. Non-extractable keys, raw entropy, private key bytes, TLS keys and credentials never cross to BlueJS, DevTools or MCP. Crypto operations are asynchronous tasks with cancellation/fuel/byte limits and do not grant filesystem, network, signing-oracle or cross-origin authority.

### WebAssembly

Add `blueice-wasm` as an isolated, standards-conformant WebAssembly runtime host; reuse a maintained, audited execution engine rather than writing a second bytecode VM merely to claim Wasm support. The compatibility target is the pinned [WebAssembly Core specification](https://webassembly.github.io/spec/core/) plus the WebAssembly JavaScript embedding API: `validate`, `compile`, `instantiate`, streaming variants, `Module`, `Instance`, `Memory`, `Table`, `Global`, imports/exports, traps, multi-value/reference/bulk-memory/SIMD/threads features only where the published feature matrix permits them.

A Wasm module has no authority beyond explicit imports supplied by its page/worker realm. Importing JavaScript, network, storage, GPU or host APIs preserves the original capability/origin/permission checks; exported functions do not become native MCP tools. Module bytes come through Phase 20's loader, respect MIME/CSP/SRI/CORS and compilation cache keys, and execute with fuel, stack, memory/table, compile-time and wall-time budgets. A trap, fuel exhaustion, validation failure or sandbox crash is reported as a bounded runtime failure and cannot corrupt the owning BlueJS realm or `core`.

DWARF/source-map metadata, Wasm instruction ranges, module/import/export identities and safe memory snapshots are retained under the same generation/privacy limits as BlueTS metadata. The direct language boundary is explicit: WebAssembly is not TypeScript type reification, and a TS contract does not validate arbitrary Wasm memory without a declared safe boundary copy/contract.

### AI MCP integration

The Web Platform MCP environment provides `storage_*`, `worker_*`, `crypto_*` and `wasm_*` targets. It can inspect quota/partition/capability metadata, bounded/redacted key and cache manifests, worker lifecycle/message traces, service-worker routes, crypto algorithm/key-handle/usages/error metadata, Wasm module validation/import/export/disassembly/source mapping, trap stacks and bounded memory ranges. `wasm_set_breakpoint`, pause/step and expression evaluation delegate to the canonical debugger and require controller lease/fuel/privacy limits. MCP cannot browse a storage directory, register a worker/service worker from an arbitrary URL, read protected storage values or cryptographic key material, use a signing oracle, inject a host import, attach to another origin's worker or mutate Wasm memory outside a paused authorized target.

## Delivery and acceptance

1. Define StorageKey/worker/Wasm identities, quotas, lifecycle and privacy model with deterministic fake clock/storage tests.
2. Implement Web Storage/IndexedDB/Cache API, then dedicated/shared workers and structured clone; introduce service workers only after request interception is provably policy-gated.
3. Integrate the isolated Wasm runtime and JS embedding, then threads/SIMD/reference extensions according to the feature matrix and conformance suite.
4. Add offline/PWA/push profiles and the negotiated MCP inspection/debug adapters.

Acceptance requires same-origin/third-party/private-profile storage partition tests; transaction/crash/eviction cases; worker message/transfer/lifecycle isolation; offline service-worker fetch behavior; Web Crypto algorithm/key-usage/non-extractability tests; Wasm validation/import/trap/fuel/memory-boundary fixtures; and MCP stale-handle/privacy/authorization failures. The human UI, DevTools and MCP must report the same worker/Wasm/storage/crypto generation and policy result.

## Explicit non-goals

- An AI-accessible raw database, filesystem or native plugin mechanism.
- A Service Worker that bypasses network policy, gatekeeper or user permission.
- Treating Wasm native code as trusted, or allowing a module to acquire ambient host authority.
- A raw key export or arbitrary signing/decryption oracle for an AI client.
