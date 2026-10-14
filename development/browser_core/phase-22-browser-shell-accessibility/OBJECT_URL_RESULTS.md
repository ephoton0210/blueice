# Blob URL software increment

This increment retains actual Blob bytes in Core and adds an owner-installed
JavaScript URL constructor plus createObjectURL/revokeObjectURL. It does not
complete Phase 20 or full File API/URL conformance.

The Core store uses the browser context and document origin as its current
storage key. Tuple origins are canonical; opaque documents retain distinct
identities even though their URLs serialize as `blob:null`. Fetch admission
requires the same key; top-level navigation follows the File API's navigation
exception. Lookup ignores fragments and preserves queries. Unknown and foreign
revocations are no-ops. Revocation, document replacement, close and departure
into retained history remove admissions without invalidating an already
retained read. Restored snapshots get fresh environments and do not revive URLs.

UUIDv4 URLs use OS randomness. The store limits admissions plus staged uploads
to 4,096 and retained/reserved data to 256 MiB. The private script protocol is
V8: exact-document upload, 65,536-byte append chunks, complete-only publication,
abort and revocation. Incomplete and wrong-owner uploads cannot publish. Failed
host transfers attempt scoped abort; document retirement also removes staging.

The VM's URL slots belong to GC and are charged to its managed-byte budget.
The constructor supports parsing, serialization, ordinary URL component
getters/setters, subclasses, parse/canParse, and Blob URL static methods.
USVString conversion preserves reentrant updates. Forged Blob/URL receivers
fail native brand checks. BlueTS uses the additive 34-binding object-url V5
profile; preceding stream/reader/drag profiles remain unchanged.

Core can navigate retained HTML and plain-text/JSON documents locally. It
preserves the creator origin for a Blob HTML document's inline script realm.
The real Core/isolated-child test uploads more than 64 KiB, renders the HTML in
another tab, runs its inline script, verifies explicit/creator-lifetime
revocation and proves only the initial HTTP fetch occurs. The existing
FileReader/stream integration case also passes.

Validation: the focused cohort passes 70 cases without failures or ignores;
the later history/budget cohort passes all eight Core cases (including two new
cases). Both actual browser integration cases pass. Strict all-target Clippy
passes for BlueJS, BlueTS/BlueJS, IPC, engine and launcher, and workspace format
check passes. The [receipt](artifacts/object-url-results.json) records input
hashes, logs, scoped commands and retained failed attempts. These results use
the isolated increment, not the separate `83436f846` full acceptance snapshot.

Remaining work includes URLSearchParams and broader URL/WPT coverage, Blob
subresource/Fetch routing, additional MIME/encoding handling, child-frame and
site-partition policy, broader DOM/event APIs and fresh complete workspace,
native and Windows acceptance after integration into the main checkout.

Behavioral references: [URL Standard](https://url.spec.whatwg.org/) and
[File API](https://w3c.github.io/FileAPI/).
