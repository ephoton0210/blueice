# @blueice/automation

A TypeScript browser/context/page/locator client for `blueice_ipc::automation`
(`development/browser_core/phase-17-automation-devtools-and-ajax/PLAN.md`,
Slice 1 item 4), and a small terminal DevTools inspection tool built on top of
it (`devtools/inspect.ts`).

This is a minimal first slice: it covers the representative automation
commands `core`'s automation service actually implements today (lifecycle,
DOM/accessibility/screenshot inspection, CSS-selector locator resolution, a
same-page click), not the full eventual Playwright-shaped surface. See
`src/index.ts`'s own module docs for the exact scope and the specific,
documented limitations (`Locator.click` inherits `AutomationRequest::Click`'s
own same-page-only scope; there is no `Page.goto`/navigation or `waitFor*` yet
-- both need the async gated-navigation completion path a later slice wires
into this protocol).

## Requirements

Runs as plain `.ts` files under Node's own native type-stripping -- no
build step, no bundler, no `tsc` dependency. Requires a Node version whose
type-stripping supports this file's syntax (no TypeScript "parameter
property" shorthand is used, since Node's strip-only mode rejects it); the
version this repository's CI pins is sufficient.

The real `blueice-core` and `blueice-automation` binaries must already be
built (`cargo build --workspace` from the repository root) before running
this package's tests or the `devtools/inspect.ts` CLI against a real process.

## Usage

```ts
import { Browser } from "@blueice/automation";

const browser = await Browser.connect({
  socketPath: "/path/to/blueice-automation.sock",
  token: "<contents of blueice-automation's token file>",
});

const page = browser.page(1); // the default tab
console.log(await page.dom());
console.log(await page.accessibilityTree());

const lease = await browser.acquireControllerLease();
await page.locator("#save").click();
await browser.releaseControllerLease(lease);

browser.close();
```

## DevTools inspection tool

```sh
node devtools/inspect.ts <adapter-socket> <token-file> [tab-id]
```

Prints the Elements panel (the tab's raw canonical DOM dump) and the
Accessibility panel (a real, structured tree built from the tab's live
accessibility snapshot) for one tab, then exits. This is deliberately a
terminal tool rather than a browser-hosted graphical panel in this slice --
see `src/devtools.ts`'s module docs for why (a browser sandbox has no way to
open the raw Unix-domain socket this protocol uses at all).

## Testing

```sh
npm test
```

Runs `devtools.test.ts`'s pure unit tests plus real subprocess-level tests
(`client.test.ts`, `devtools_inspect_cli.test.ts`) that spawn the actual
compiled `blueice-core`/`blueice-automation` binaries.
