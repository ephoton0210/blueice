# macOS browser keyboard focus

This increment completes keyboard traversal of the browser's enabled visible
controls and their handoff to the existing core-owned page key loop. Complete
native and static acceptance is recorded in
[the dated results](MACOS_CHROME_FOCUS_RESULTS.md); the complete browser delivery
plan remains active.

Tab and Shift-Tab follow the rendered order: window notices, tab/group controls,
navigation/address/profile/download/permission toolbar, optional find controls,
core page, optional assistant controls and page zoom. The loop wraps in either
direction. Disabled controls and collapsed group members are excluded using
their actual view availability. Author text and labels never become focus IDs.
Focused tabs must remain visible in a scrolled strip. Buttons, checkboxes and
menus support normal keyboard activation and show a visible focus indicator.

The shell does not infer or duplicate DOM focus order. Page traversal stays on
the versioned native-input boundary. A confirmed core focus exit chooses the
previous/next browser control according to its direction. Address/search and
assistant fields retain AppKit/SwiftUI text editing, selection and clipboard
behavior. Marked input is handled by the native input method. Native menus,
sheets and the separate private permission child retain their own key loops.

Focus changes and rapid following keys are ordered. Queued keys belong to the
same model, selected tab, lifecycle epoch, service readiness and native window; they cannot replay
into another tab/window, a closed control or a newly opened modal/private panel.
Structural changes recover focus at a surviving neighbor. Existing explicit
address/find/page focus commands take precedence over an older pending move.
Service readiness changes discard pending shell and page keys, including a
stop/restart which retains the selected tab and document epoch. Enabled notice
controls remain keyboard operable after the core becomes unavailable; the
disabled page and navigation controls leave the rendered loop.

Validation requires actual-window Tab/Shift-Tab and Space/Return operations,
all toolbar and dynamic find/assistant/group controls, page boundary handoff,
disabled/collapsed exclusion, scrolling, rapid keys, window/tab/modal lifecycle
rejection, editable field/IME preservation, native regressions and final source/
product/signature/process checks. Screenshots/results remain ignored; the dated
text receipt retains unsuccessful attempts. This increment does not substitute
for physical VoiceOver/IME or the remaining browser requirements.
