# Phase 22 — Browser Shell, Native Interaction, and Accessibility

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Proposed — the reference frontend presents pixels and basic pointer/scroll/resize input, but it is not yet a complete browser shell or an operating-system accessibility client.

## Objective

Deliver a human browser experience that can use the same pages an AI can inspect: correct native input and text editing, tabs/windows/downloads/printing/permissions, system accessibility and display adaptation. The shell is a client of `core`; it does not own DOM, navigation policy, layout or a second page instance.

## Decisions

### Native input and editing

Define `blueice_ipc::input` as the canonical event stream from any frontend, automation client or permitted candidate presenter. It carries physical/logical keyboard state, IME composition/commit/cancel, pointer/touch/pen contacts, wheel/gesture, focus, selection, drag/drop, clipboard and file-selection results with viewport/device-scale/window/document generations. Phase 20 dispatches it through the real DOM event/default-action model; the frontend never guesses a form control's semantics.

Text selection, caret/selection geometry, copy/cut/paste, drag/drop payloads, virtual keyboard and input-method candidates remain document-originated state with a privacy policy. A page cannot read arbitrary system clipboard/file content without permission and a user gesture. An AI may request a normal interaction under a controller lease, but it cannot impersonate human IME, disclose clipboard/file bytes, or dismiss a permission/file-picker dialog.

### Browser chrome and operating-system integration

Provide a first-party shell contract for tab strip/group UI, multiple top-level windows, profiles/contexts, address/search UI, navigation controls, zoom/text scale, fullscreen/picture-in-picture, context menus, find-in-page, print/print preview, downloads shelf, file picker, crash/reload UI and per-site permission prompts. Phase 16's `TabManager` remains the source of truth for tabs/groups; the shell only renders and controls it through versioned IPC. Window state, DPI/multi-monitor changes, color space, dark/light mode, high contrast and reduced-motion settings are inputs to one viewport/media-query/compositor state.

Downloads use Phase 10's separate manager. Print output is generated from a print media/layout profile with resource/pagination limits and follows the same document/security policy as screen rendering. A browser-chrome permission prompt is human-only: MCP may inspect its existence and request that the human be prompted, but only the shell's visible user decision creates a capability grant.

### Accessibility as a platform boundary

Phase 1's accessibility-tree-shaped representation becomes the single source for AT-SPI, UI Automation, NSAccessibility and equivalent platform bridges. It maps DOM/ARIA/HTML semantics, names, states, relations, bounds, focus, selection, values, live regions, actions and text ranges to platform APIs, with stable IDs and document generations. Assistive-technology actions return through the same input/default-action path as ordinary user interaction; no platform bridge directly mutates DOM.

The bridge follows WCAG 2.2-relevant keyboard, focus, contrast, reflow, motion and alternative-content expectations, with the exact supported criteria/version exposed as a capability report. Human and AI diagnostics must see the same resolved accessible name/role/state and the source/provenance that produced it; private text and password values remain redacted according to caller scope.

### AI MCP integration

`shell_*` and `accessibility_*` MCP capabilities expose window/tab/profile metadata, viewport/zoom/theme state, bounded input/focus/selection traces, download/print states, permission-prompt state and platform-independent accessibility snapshots/actions. A control action needs the normal controller lease; accessibility actions follow the same policy as their equivalent user action. There is no MCP operation to silently grant permission, read a password/clipboard/file, fabricate a trusted user gesture, or control an unregistered OS window.

## Delivery and acceptance

1. Implement canonical input/IME/selection/clipboard/file-picker protocol and deterministic event-sequence tests.
2. Build tab/window/profile/download/print/permission shell UI on the existing launcher/core boundary; retain same-page/core guarantees across every shell surface.
3. Add OS accessibility bridges and test with real screen-reader/automation adapters on supported platforms.
4. Add `shell_*`/`accessibility_*` MCP resources and prove they correlate to the same DOM/frame/action generations visible to a human.

Acceptance requires CJK/RTL IME editing, keyboard-only form completion, selection/clipboard policy denial, touch/pen/drag input, DPI/theme/reduced-motion transition, a screen-reader action, download/print/permission flow and multi-window/tab handoff. Tests must prove no AI action bypasses the visible human permission or privacy boundary.

## Explicit non-goals

- A shell-owned parallel DOM or renderer.
- Treating programmatic MCP input as a trusted human gesture.
- Exposing OS credentials, clipboard, files, notifications or permissions by default.
