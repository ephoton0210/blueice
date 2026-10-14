# Top-level page global aliases

[← Browser integration TODO](BROWSER_INTEGRATION_TODO.md)

M1l installs `window`, `self`, `top` and `parent` in the owner-selected
`core-script-dom-window-v13` File DOM profile. Each initially returns the
same intrinsic realm global as `globalThis`. A page can reach its existing
DOM objects and scheduling functions through any alias. Standalone VMs,
ShadowRealms and the older snapshot/text/mutation/event profiles acquire no
aliases automatically.

The current host admits top-level documents only. This increment does not
implement a `Window` constructor, nested browsing contexts, cross-origin
access or a navigation-stable `WindowProxy`. Navigation creates a new realm;
each tab retains its own global and alias state. A future frame host must
supply the appropriate browsing-context identities before enabling aliases.

## Property behavior

Following the [HTML Window attributes](https://html.spec.whatwg.org/multipage/nav-history-apis.html#the-window-object)
and [Web IDL attribute rules](https://webidl.spec.whatwg.org/#es-attributes):

| Name | Initial value | Descriptor |
| --- | --- | --- |
| `window`, `top` | Current realm global | Enumerable, non-configurable accessor; getter only. |
| `self`, `parent` | Current realm global | Enumerable, configurable accessor; assignment replaces it with a writable, enumerable, configurable data property. |

Getters and setters validate the page-global receiver; null/undefined use
the accessor's own realm global. They are ordinary non-constructible native
functions with the expected names and lengths. Replacing or deleting the
page's `globalThis` property does not redirect the intrinsic alias target.
Global `var` properties and lexical bindings retain their existing semantics.
Installation checks all four names without invoking page accessors before
publishing any alias; collisions and non-extensible globals are refused.

## Types and inventory

The v13 artifact contains **51 runtime inventory entries**: the unchanged
47 v12 entries followed by four aliases. Its `BlueIceWindow` host members
are generated from the existing declarations using the public BlueTS parser
and their original annotation/signature text. DOM, constructor, callback,
optional and rest-argument types share their existing source. The interface
describes the installed host surface; it is not a full `lib.dom.d.ts` or a
replacement for BlueTS's separate ECMAScript standard-library model.

The immutable v12 artifact and all older artifacts remain available. The
child's File DOM installer and checked BlueTS preparation both select v13
and verify the complete binding inventory. Private protocol v59 needs no new
field or page-selected capability.

`backend/launcher/tests/browser_timers.rs` observes every section C global
through `typeof globalThis.<name>` in real JavaScript and checked BlueTS
pages. It also pins the DOM/scheduling names and absence of `Window`,
`postMessage` and `MessageChannel`. Both pages must subsequently publish
three distinct counter frames with matching Representation generations.
The independent inventory expectations make future surface changes explicit.

Public VM/page-runtime regressions cover identity, global writes versus
lexicals, descriptors, strict/sloppy assignments, replacement/deletion,
receiver checks, intrinsic identity, collision refusal, non-extensibility,
ShadowRealm absence, tab isolation and navigation. Public typing tests retain
valid host calls and reject wrong arguments, callback types and writes to
the immutable aliases. Compiler-wide ambient value queries and diagnostics
for missing intermediate members remain separate section H work.
