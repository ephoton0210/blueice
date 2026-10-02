# BlueIce Windows frontend

The Windows frontend uses WinUI 3 (C# and XAML), Windows App SDK
`2.5.1`, and .NET 10. It is an unpackaged, self-contained x64
application. The Rust core owns DOM, layout, rendering, history and tab
identity; the frontend presents its RGBA frames and forwards input.

Build on Windows with .NET 10, the Windows SDK and the Rust MSVC toolchain:

```powershell
powershell -File frontend/winui/build.ps1
dotnet run --project frontend/winui/tests/ProtocolTests.csproj -- target/debug/blueice-core.exe
```

The build script copies the Rust core beside the frontend. Launch the
reported `BlueIce.WinUI.exe`; `--core-exe <absolute-path>` optionally selects
another core. Keep the generated runtime files beside the executable.

Run the native UI test in a logged-in Windows desktop:

```powershell
powershell -File frontend/winui/tests/WindowSmoke.ps1 `
  -AppExe <path-to-BlueIce.WinUI.exe> `
  -CoreExe <path-to-blueice-core.exe> `
  -ArtifactDirectory <test-output-directory>
```

This first slice provides an address field, graphical tab creation,
selection and closing, back/forward/reload, core-rendered built-in pages,
viewport resizing, pointer clicks, scrolling and basic character/backspace
input. It starts at `about:credits`; `about:settings`, `about:downloads` and
`about:assistant` display their existing core-owned unavailable-service
states when their service is absent.

## Process boundary

The frontend starts one private `blueice-core --stdio --frame-dir <new-path>`
child. Only that child's inherited stdin/stdout pipes carry the existing
version-two, little-endian length-prefixed browser envelopes. No network
listener is opened. Each tab is addressed by its core-owned ID; selection
remains frontend-local. The Rust reader stages complete messages before
session polling so a partial pipe read cannot corrupt framing. Both sides
retain the 8 MiB control-message limit.

Pixels remain on the separate memory-mapped frame plane. The frontend
validates the session directory, dimensions and exact byte length, maps
read-only, copies RGBA to BGRA for WinUI, then releases the mapping. Closing
the window requests shutdown, terminates its owned child if necessary, and
removes its private frame directory.

## Remaining work

The Unix launcher/gatekeeper/downloads/assistant/extension services have not
been ported to Windows. External HTTP(S) navigation therefore reports the
existing unavailable gatekeeper outcome; it never bypasses review. This
private launch mode has no shared launcher/MCP rendezvous or trusted
permission-panel connection yet. Complete IME/selection/clipboard support,
tab-group controls, multiple windows, localization, packaging, printing and
platform accessibility adapters remain in Phase 22. WinUI exposes its native
shell controls to UI Automation; it does not yet expose page semantics as a
Windows accessibility tree.
