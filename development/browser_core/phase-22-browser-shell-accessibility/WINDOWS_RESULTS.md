# Windows WinUI 3 first-slice validation

Date: 2026-10-02. Baseline: `80499aba6`. This adds an initial Windows shell;
it does not complete all Phase 22 capabilities.

## Implementation

- C#/XAML WinUI 3, pinned Windows App SDK `2.5.1`, .NET 10, x64,
  unpackaged/self-contained deployment with a NuGet lock file.
- Native address field, graphical tabs, tab creation/selection/closing,
  back/forward/reload and settings navigation. Reload uses the committed
  core URL rather than submitting an uncommitted address-field draft.
- Core-owned DOM/layout/raster/session/history through a private inherited
  stdin/stdout browser-protocol connection. Control frames remain bounded to
  8 MiB and retain protocol version, request IDs and tab IDs.
- Read-only memory-mapped RGBA frames are validated and converted to WinUI's
  BGRA bitmap. Input and viewport sizes return to that same core instance.
- Asynchronous window closing sends shutdown, stops the owned child and
  removes its unique frame directory. Existing directories are never reused
  or removed by a failed core startup.

## Native checks

Linux runs through `ssh pb60g.bravotekcorp.cc`; Windows runs in its
`omnisolve-winui-automation` KVM with the MSVC toolchain. Only the isolated
`blueice-merge-fixes` validation checkout is modified on each host.

| Check | Linux | Windows |
|---|---|---|
| Core and IPC crate tests, including doctests | 993 passed, 0 failed | 449 passed, 0 failed |
| New private-pipe public-boundary tests (included above) | 4 passed | 4 passed |
| C# protocol/frame adapter tests against an actual core process | — | 5 passed |
| Workspace all-target Clippy, `-D warnings` | Passed | Passed |
| Workspace formatting | Passed | Passed |
| WinUI native build | — | Passed, 0 warnings/errors |
| `build.ps1`, locked restore and bundled core | — | Passed; bundled/core-build SHA-256 matched |
| Native window UI Automation and visible pixels | — | 7 checks passed |

The pipe regression tests prove complete child-process frame rendering,
tab/request correlation, partial reads across poll timeouts, oversized-frame
rejection, unavailable gatekeeper review without any HTTP connection, and
ownership-preserving directory cleanup. These are focused core/IPC suites,
not a new unfiltered full-workspace test or fresh coverage measurement.

The C# tests prove canonical UTF-8 envelopes, fragmented reads, bounded and
truncated messages, path/dimension/byte-length frame checks, RGBA-to-BGRA
conversion and real core rendering through the C# adapter.

## Native window evidence

`frontend/winui/tests/WindowSmoke.ps1` runs in the existing logged-in desktop
using UI Automation. It exercises built-in page rendering, settings,
back/forward, reload, tab creation/selection/closing, the unavailable-review
response and owned process/frame cleanup. A desktop pixel check rejects a
white/blank page even when the automation tree exists. It temporarily keeps
the display awake and releases that request at exit; no permanent VM power
or graphics setting is changed.

The final test launches the generated frontend directly with its sibling
core, with no explicit `--core-exe` argument. All seven checks passed,
including 2,999 dark samples in the core-rendered credits viewport. See the
[window screenshot](artifacts/windows-winui.png) and
[native UI results](artifacts/windows-ui-results.txt).

## Remaining capabilities

Windows launcher/service transports and shared MCP rendezvous remain open.
External HTTP(S) navigation therefore reports unavailable gatekeeper review;
this slice never bypasses review. Full IME/text selection/clipboard, group
controls, multi-window behavior, trusted permission/assistant panels,
localization, downloads controls, printing and page accessibility adapters
are still Phase 22 work. Native shell UI Automation is distinct from a
Windows accessibility bridge for core page semantics.

The final local/Linux/Windows digest across 20 implementation, test and
frontend build files is:

`2cac149d968a45a329f02d667147a7efa0f2e25f0d3387fc20f7b39ef242f3b6`

## Complete same-source follow-up on 2026-10-12

Frozen product source `83436f846` passes the complete Windows Rust workspace:
5,670 passed, zero failed, 64 ignored across 495 groups. All 25 workspace
packages/474 formatter targets, strict workspace/all-target Clippy and the
complete all-target build pass. WinUI native build and five actual-Core
protocol cases also pass. An independent logged-in session-1 desktop run
passes all seven existing cases, including 2,999 dark viewport samples and
ordinary window close stopping Core/removing frame directories. The desktop
task and full bootstrap both exit zero; all 2,112 source files remain
unchanged. A separate final audit finds no owned task, desktop helper or app.

The original build chose GNU through an ambiguous Rust toolchain selector;
the external validation helper pins MSVC and the production build fix is
committed separately. The UIA provider omitted WindowPattern, so the corrected
smoke driver validates the owned HWND/PID before sending ordinary WM_CLOSE.
The seven assertions are preserved. Subsequent STA and MTA driver attempts
passed all seven assertions but stalled during JSON publication: Windows
PowerShell 5.1 Get-Content strings carry provider/drive metadata. The direct
traces and independent plain-string probe identify that serializer boundary.
Reading results with ReadAllText fixes publication without changing any test
assertion. Microsoft documents the
[string-property serialization behavior](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/convertto-json?view=powershell-7.6).

Build products, caches and TEMP/TMP use the dedicated B: test space. Final
free space is C: 6.75 GiB/B: 275.28 GiB; original disk guards remain active.
The [complete receipt](artifacts/windows-83436f846-results.json),
[desktop results](artifacts/windows-83436f846-20261012/ui-results.txt) and
[screenshot](artifacts/windows-83436f846-20261012/window.png) preserve source,
log/artifact hashes, failed attempts, actual command scopes and cleanup.
This acceptance predates the later object URL/AbortSignal/CustomEvent code;
those changes need fresh complete gates. Physical-system acceptance remains
independent.
