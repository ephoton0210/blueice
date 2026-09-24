# BlueIce extension package and guest ABI reference

This is the implemented Phase 9 interface for an extension author. It is a
versioned, intentionally narrow **WebAssembly guest ABI**, not a browser
content-script API. The [design plan](PLAN.md) tracks work that is still open;
this reference describes only operations the installed, core-spawned host can
currently execute. The Rust definitions in `backend/extension/src/manifest.rs`,
`backend/extension/src/runtime.rs`, and `backend/ipc/src/extension.rs` are the
authoritative implementation if this document and a future binary differ.

## Package and permission model

Place a strict JSON manifest beside a WebAssembly version-1 core module:

```json
{
  "name": "My extension",
  "version": "0.1.0",
  "blueice_api_version": 1,
  "entry_point": "extension.wasm",
  "capabilities": {
    "declared": ["dom:read", "ui:inject"],
    "optional": [],
    "runtime_ephemeral": []
  }
}
```

`name` and `version` are nonempty UTF-8 without control characters, at most
128 and 64 bytes respectively. Unknown manifest fields are errors. `entry_point` is a
package-relative `.wasm` path; absolute paths, `..`, and a symlink resolving
outside the package root are rejected. The module may be at most 1 MiB. Each
capability name may appear once in exactly one tier. The accepted names are
`dom:read`, `dom:write`, `network:observe`, `network:intercept`, `ui:inject`, and
`storage`.

Only `declared` grants a capability today. `optional` and
`runtime_ephemeral` are parsed, but **neither can be requested or exercised**:
there is no user-consent or authenticated gesture flow yet. Do not interpret
their presence as a grant. The shared launcher IPC accepts both human frontend
and AI clients, so an ordinary client message or toolbar activation cannot by
itself prove human approval; see the [Phase 9 plan](PLAN.md) before designing
an optional-grant flow. The host derives an ID from the exact manifest and
module bytes; the package cannot choose its identity. In production, core
starts the host and authenticates that child with a fresh environment-only
credential before accepting the derived ID. The manual development socket's
bearer-style `Hello` is not equivalent to that production boundary.

There is no frontend install button yet. To run one installed package against
`blueice-core`, pass `--extension-socket <private-path>`,
`--extension-manifest <extension.json>`, and
`--extension-host <blueice-extension-host-binary>` **together** when starting
core. Core validates the package, creates the private socket, and starts that
host itself; do not invoke its `--connect` mode or invent an authentication
token from extension code. Core still requires its normal frontend connection
and a live gatekeeper service for reviewed effects. Omitting
`--extension-host` while supplying the other two extension flags selects the
explicitly weaker manual protocol-development mode, not an installed guest
runtime.

`blueice_api_version` is the manifest schema version (currently only `1`). It
is separate from the independently negotiated **capability API versions**:

| Capability | Supported versions | Core-spawned guest version | Implemented effects |
| --- | --- | --- | --- |
| `dom:read` | 1–2 | 2 | A live tab's AI-facing representation, addressed by explicit tab ID in v2. |
| `dom:write` | 1–7 | 7 | Bounded native form-control operations; legacy generic v1 mutation has no core effect. |
| `network:observe` | 1–2 | 2 | Committed main-frame final response (v1) and initial request/redirect trace (v2). |
| `network:intercept` | 1–5 | 5 | Exact navigation URL block (v2), clearing own rules (v3), ASCII host/subdomain block (v4), and literal host/path-prefix block (v5); legacy v1 registration has no core effect. |
| `ui:inject` | 1–3 | 3 | Native toolbar button (v1), fixed-text native popup (v2), and one browser-owned popup action button (v3). |
| `storage` | 1–2 | 2 | Core-owned, process-lifetime v1 bucket and separate durable v2 bucket for the derived extension ID. |

An unsupported capability version is reported for that capability without
invalidating compatible declarations. All guest imports are linked, but a
call still needs both the installed manifest grant and a negotiated version;
an import's presence does not confer permission. The core-backed host currently
negotiates only its installed `declared` entries at the versions above.

## Guest execution contract

Compile a WebAssembly core module that exports `memory` and
`blueice_start: () -> ()`. Imports are from the `blueice` module below; there
is no WASI, filesystem, environment, clock, randomness, socket, or generic
host-call import. The host runs a **fresh instance** after core's runtime-start
barrier and again for each delivered event. Each invocation has at most 1 MiB
linear memory, one memory/table/instance, 1,024 table elements, and 1,000,000
fuel units. A trap or missing/unknown import fails that invocation; there is
no ambient fallback.

`runtime_event_kind() -> i32` returns `0` for startup, `1` for a committed
navigation, `2` for toolbar activation, or `3` for activation of a native
popup's action button. `runtime_event_tab_id() -> i64`
returns `-1` at startup and the event's opaque live tab ID otherwise. A
navigation event is advisory: core keeps at most 16 queued events and may drop
events if the extension cannot drain them. None of these events proves an
authenticated human gesture: the launcher also accepts client messages from
external AI/MCP clients. They cannot grant `optional` or `runtime_ephemeral`
capabilities. Re-read a live representation before acting on
a node ID.

All pointers and lengths below are `i32` offsets/counts into exported guest
memory. They must be nonnegative, in bounds, and contain valid UTF-8 when text
is expected. Output is **not NUL-terminated**. Reads return the copied byte
count (`0` is a valid empty value), `-2` for a too-small destination, `-3` for
an invalid argument, `-4` only for a documented missing-value case, and `-1`
for a denied/unavailable core operation (including an unknown tab). Mutations return `0` on success,
`-3` for malformed guest arguments, and `-1` for rejection or unavailability.
Both `storage_remove_utf8` and `durable_storage_remove_utf8` instead return `1` if a key was removed and `0` if it
was already absent. A negative result never authorizes an optimistic side
effect in the guest.

| Import (`blueice`) | Capability/version | Result and bounds |
| --- | --- | --- |
| `dom_read_utf8(tab_id:i64, dst:i32, cap:i32) -> i32` | `dom:read` v2 | JSON AI snapshot; at most 64 KiB. |
| `set_text_input_value(tab_id:i64, node_id:i64, ptr:i32, len:i32) -> i32` | `dom:write` v2 | At most 4 KiB UTF-8; only a supported live native text input. |
| `set_checkbox_checked(tab_id:i64, node_id:i64, checked:i32) -> i32` | `dom:write` v3 | `checked` is exactly `0` or `1`; only an enabled native checkbox. |
| `set_textarea_value(tab_id:i64, node_id:i64, ptr:i32, len:i32) -> i32` | `dom:write` v4 | At most 4 KiB UTF-8; only an enabled native textarea. |
| `set_radio_checked(tab_id:i64, node_id:i64) -> i32` | `dom:write` v5 | Select one enabled named native radio; core derives and clears its group. |
| `select_option(tab_id:i64, node_id:i64) -> i32` | `dom:write` v6 | Select one enabled option in an enabled native single-select; core clears its peers. |
| `set_range_input_value(tab_id:i64, node_id:i64, value:i64) -> i32` | `dom:write` v7 | Integer range input only; core validates live `min`/`max`/`step`. |
| `network_response_utf8(tab_id:i64, dst:i32, cap:i32) -> i32` | `network:observe` v1 | Final committed GET response JSON, at most 4 KiB; `-4` for a non-HTTP page. |
| `network_trace_utf8(tab_id:i64, dst:i32, cap:i32) -> i32` | `network:observe` v2 | Initial GET URL, redirect hops, and final response JSON, at most 32 KiB. |
| `register_network_block_url(ptr:i32, len:i32) -> i32` | `network:intercept` v2 | At most 2 KiB absolute credential-free HTTP(S) URL; exact canonical navigation match. |
| `clear_network_block_urls() -> i32` | `network:intercept` v3 | Clears this connection's exact-URL, host, and path-prefix rules. |
| `register_network_block_host(ptr:i32, len:i32) -> i32` | `network:intercept` v4 | At most 253 ASCII bytes, plain canonical DNS/IPv4 spelling without a URL, wildcard, or Unicode; matches the host and dot-boundary subdomains before connection. IPv4 literals match exactly. |
| `register_network_block_path_prefix(host_ptr:i32, host_len:i32, path_ptr:i32, path_len:i32) -> i32` | `network:intercept` v5 | Host uses v4's grammar; path is 1–512 literal ASCII bytes beginning with `/`, with no empty/dot segments, percent escapes, query, or fragment. Matches the canonical URL path at a `/` segment boundary on that host or a dot-boundary subdomain; IPv4 matches exactly. |
| `set_toolbar_button_utf8(ptr:i32, len:i32) -> i32` | `ui:inject` v1 | One 1–20-byte label: ASCII letters/digits, spaces, `-`, `_`, no outer spaces. |
| `clear_toolbar_button() -> i32` | `ui:inject` v1 | Removes only this connection's button and popup. |
| `show_popup_utf8(tab_id:i64, title_ptr:i32, title_len:i32, body_ptr:i32, body_len:i32) -> i32` | `ui:inject` v2 | Requires own live toolbar; title uses label grammar, body is 1–120 printable ASCII bytes without outer spaces. |
| `show_popup_action_utf8(tab_id:i64, title_ptr:i32, title_len:i32, body_ptr:i32, body_len:i32, action_ptr:i32, action_len:i32) -> i32` | `ui:inject` v3 | Same title/body bounds as v2; adds one action label using the toolbar-label grammar. The title, body, and label are gatekeeper-reviewed before native publication. |
| `clear_popup() -> i32` | `ui:inject` v2 | Clears only this connection's native popup. |
| `storage_get_utf8(key_ptr:i32, key_len:i32, dst:i32, cap:i32) -> i32` | `storage` v1 | Copies at most 16 KiB UTF-8, or `-4` for an absent key. |
| `storage_set_utf8(key_ptr:i32, key_len:i32, value_ptr:i32, value_len:i32) -> i32` | `storage` v1 | Sets one bounded UTF-8 value. |
| `storage_remove_utf8(key_ptr:i32, key_len:i32) -> i32` | `storage` v1 | Returns `1` removed / `0` absent. |
| `durable_storage_get_utf8(key_ptr:i32, key_len:i32, dst:i32, cap:i32) -> i32` | `storage` v2 | Reads the separate durable bucket; at most 16 KiB UTF-8, or `-4` for an absent key. |
| `durable_storage_set_utf8(key_ptr:i32, key_len:i32, value_ptr:i32, value_len:i32) -> i32` | `storage` v2 | Persists one bounded UTF-8 value in the separate durable bucket. |
| `durable_storage_remove_utf8(key_ptr:i32, key_len:i32) -> i32` | `storage` v2 | Returns `1` removed / `0` absent in the durable bucket. |

The v3 popup is still browser-owned chrome, not guest HTML. Core assigns a
fresh popup ID; a frontend activation includes that ID and is rejected if the
popup has been replaced, dismissed, or belongs to another tab. A successful
activation closes the popup and queues event kind `3` for the extension's next
fresh invocation. The event queue is bounded and an unavailable queue leaves
the popup visible with a structured error. The action does not open a URL or
grant a permission by itself. Client-originated activation is not trusted
human consent, even when it came from the reference frontend.
While a popup is visible in the selected human tab, keyboard input and wheel
events are consumed by browser chrome rather than the underlying page. Escape
dismisses it; Enter or Space activates its button only when the whole button
is visible in the current window. An offscreen button cannot be activated by
keyboard.

The storage key is 1–256 ASCII bytes from `[A-Za-z0-9._-]`. Each derived
identity has at most 128 entries and 256 KiB total key/value bytes, including
every newly inserted key's bytes. An over-limit write is rejected without
changing the prior value. These limits apply independently to the v1 and v2
buckets. The v1 bucket is lost when that core process exits, including after
a v2 handshake. V2 persists under a core-selected private user-data directory,
using a hashed manifest-derived identity, private files, a per-bucket OS lock,
and atomic replace plus filesystem sync. A missing, busy, corrupt, or unsafe
durable store returns `-1`; it never falls back to v1. An updated extension
package has a different derived identity and cannot read the old package's
bucket without a future migration mechanism. No guest can supply a bucket ID or host
path. All implemented `dom:write` effects and declarative rule registration receive
mandatory, fail-closed gatekeeper review after ordinary capability checks;
publishing popup text or its v3 action label is also reviewed. A reviewer outage is **not** clearance.

`network_response_utf8` serializes
`{"method":"GET","final_url":"…","status":200,"content_type":"text/html"}`
(the content type can be `null`; when present it is validated printable ASCII
of at most 256 bytes). `network_trace_utf8` serializes
`{"request_url":"…","redirects":[{"request_url":"…","status":302,"target_url":"…"}],"response":{…}}`.
These describe only a **successfully committed main-frame navigation**, never
in-flight, rejected, or superseded traffic. They expose no arbitrary headers,
cookies, bodies, or subresource requests; URLs may still contain sensitive
paths/query strings, so grant `network:observe` deliberately. Exact navigation
URL rules ignore fragments; host rules cover that host and its dot-boundary
subdomains. Path-prefix rules additionally require a case-sensitive literal
canonical URL path match: `/private` covers `/private` and `/private/report`,
not `/privateer`; query and fragment do not matter. This is URL-path matching,
not server-side route normalization: a percent-encoded or rewritten path does
not match its decoded spelling. All three kinds cover redirect targets, share a 64-rule quota per
connection, and disappear on clear or disconnect. They cannot rewrite headers/bodies,
redirect traffic, or observe an interception callback.

## Verified minimal package

[`examples/toolbar/extension.json`](examples/toolbar/extension.json) and
[`extension.wat`](examples/toolbar/extension.wat) are a package source example:
compile the WAT to `extension.wasm` beside the manifest using a WAT compiler
(for example `wat2wasm extension.wat -o extension.wasm`). The guest shows a
native `Example` button on startup and a fixed `Hello` / `Ready` text popup on
toolbar activation; it asks for only `ui:inject`. The repository test below
compiles the **same WAT source**, loads the same manifest, executes both event
kinds through the restricted runtime, and asserts the exact core requests:

```sh
cargo test -p blueice-extension-host --test documented_example
```

The sample is an ABI/conformance fixture, not a claim that optional grants,
arbitrary interactive popup controls, arbitrary DOM mutation, subresource interception,
or a general extension marketplace are already available. Those remain open
in the [Phase 9 plan](PLAN.md).

## Private host/core transport (for implementers)

The host/core socket is a **private, long-lived implementation protocol**,
not a supported way for an unrelated process to claim an installed package.
`blueice_ipc::extension::{ExtensionRequest, ExtensionReply}` serialize as
Serde JSON enum variants, each preceded by a four-byte little-endian length
with an 8 MiB frame ceiling. A newly connected host sends one
`HelloAuthenticated { extension_id, capability_versions, authentication }`,
then waits for `HelloAck { unsupported_capabilities }`; core sends no
successful acknowledgement to an unauthenticated peer in host-spawned mode.
The host next sends `RuntimeReady` and awaits `RuntimeStart`, then executes the
startup invocation. Later `NextRuntimeEvent` requests pull one event or stream
closure at a time. Capability requests use one request/one reply on this same
connection; there is no concurrent socket reader. The documented guest ABI is
the extension-author interface; direct manual `Hello` sockets are for isolated
protocol development only and have weaker bearer-claim semantics.
