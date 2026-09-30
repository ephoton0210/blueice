# BlueTS test interface

[← Phase 18 plan](PLAN.md)

`bluets-test-interface` uses the same long-lived JSON Lines transport contract as BlueJS's `bluejs-test262` adapter. This lets a supervising test runner use the same process lifecycle and bounded request/response exchange without linking BlueTS to BlueJS.

```text
stdin/stdout: one UTF-8 JSON value per line
startup reply: {"ready":1}
one request line -> exactly one reply line
```

The adapter is intentionally **compile-only**. It parses, resolves, checks and optionally emits through the ordinary BlueTS public compiler API. It does not execute JavaScript, install Test262 globals, load files/URLs, or claim that the Test262 corpus is a TypeScript conformance suite. Once the public BlueJS AST/IR bridge exists, a separate runtime adapter may compose the two; this transport does not create that dependency early.

## Request

The shared fields are compatible with the BlueJS adapter:

```json
{
  "source": "const value: number = 1;",
  "mode": "raw | sloppy | strict | module",
  "module_path": "src/main.ts",
  "module_sources": {
    "types/model.d.ts": "export interface Model { id: string }"
  },
  "parse_only": false,
  "source_map": false,
  "declaration": false,
  "runtime_policy": "checked | transpile-only | strict-runtime",
  "limits": {
    "max_modules": 4096,
    "max_module_edges": 16384,
    "max_module_depth": 128,
    "max_total_source_bytes": 16777216,
    "max_source_bytes": 1048576,
    "max_tokens": 1000000,
    "max_type_depth": 128,
    "max_type_expansions": 256,
    "max_source_map_segments": 100000
  }
}
```

`source` is required. `mode` defaults to `raw`; `sloppy` is accepted as the BlueJS/Test262-compatible alias for `raw`, while `strict` and `module` retain their shared transport names even though BlueTS only compiles them. `module_path` and `module_sources` are canonicalized into the in-memory `memory:///` module namespace. The entry source always wins if it is also present in `module_sources`. All module data is caller-supplied; the adapter performs no filesystem or network I/O.

Omitted limits use `CompilerLimits::default()`. Limits are passed directly to the compiler and therefore affect its fingerprint. Invalid mode/policy or malformed JSON receives `harness_error` rather than terminating the persistent adapter.

## Reply

Replies retain BlueJS's `kind` and `phase` conventions while preserving the stable BlueTS diagnostic code and byte span:

| BlueTS outcome | `kind` | `phase` |
| --- | --- | --- |
| successful compile | `ok` | `compile` (`parse` with `parse_only`) |
| parser diagnostic | `SyntaxError` | `parse` |
| unsupported syntax | `unsupported` | `parse` |
| module resolution/cycle diagnostic | `SyntaxError` | `resolution` |
| binding/checker/declaration diagnostic | `TypeError` | `type` |
| bounded-resource diagnostic | `resource_error` | `compile` |
| invalid request/transport JSON | `harness_error` | omitted |

A successful reply contains `language_version`, the deterministic compiler `fingerprint`, and an `artifacts` summary. The summary names only canonical module IDs and whether a JavaScript artifact has Source Map or declaration output; it never returns source text. A failure also includes `code` (for example `BTS3003`), message, and a half-open source span in UTF-8 bytes.

Example:

```json
{"kind":"TypeError","phase":"type","code":"BTS3003","span":{"module":"memory:///entry.ts","start_byte":0,"end_byte":42}}
```

## Running it

```sh
cargo build -p blueice-bluets --bin bluets-test-interface
target/debug/bluets-test-interface
```

The process emits its ready line immediately. Test supervisors must impose their own wall deadline and bounded I/O exactly as BlueJS's Test262 runner does. BlueTS compiler limits provide the per-request parser/module/checker/map work bounds; a blocked external process is still a supervisor concern.
