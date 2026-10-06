// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine
import SwiftUI

@main
struct BlueIcePanelsApp: App {
    @ObservedObject private var localization = BrowserLocalization.shared
    @NSApplicationDelegateAdaptor(PermissionPanelDelegate.self) private var delegate
    var body: some Scene { Settings { EmptyView() } }
}
@MainActor
final class PermissionPanelDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var window: NSWindow?
    private var model: PermissionPanelModel?
    private var assistant: AssistantSettingsModel?
    private var languageObservation: AnyCancellable?
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Stay hidden until the user opens Permissions in their browser. The
        // initial inspection satisfies launcher's existing readiness barrier.
        NSApp.setActivationPolicy(.accessory)
        let arguments = ProcessInfo.processInfo.arguments
        guard arguments.contains("--trusted-window-stdio"), let option = arguments.firstIndex(of: "--socket"),
              arguments.indices.contains(option + 1) else { NSApp.terminate(nil); return }
        do {
            let service = try NativePermissionService(socket: arguments[option + 1])
            let model = PermissionPanelModel(service: service); self.model = model
            let assistant = AssistantSettingsModel(service: service); self.assistant = assistant
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 560),
                                  styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            window.title = BrowserStrings.text("BlueIce Browser Panels"); window.identifier = .init("permissions-window")
            window.isReleasedWhenClosed = false; window.delegate = self; window.center()
            window.contentView = NSHostingView(rootView: TrustedPanelsView(permissions: model, assistant: assistant))
            self.window = window
            languageObservation = BrowserLocalization.shared.$language.sink { [weak self] _ in
                Task { @MainActor in self?.window?.title = BrowserStrings.text("BlueIce Browser Panels") }
            }
            Task { await model.refresh(); await assistant.refresh() }
        } catch { NSApp.terminate(nil) }
    }
    func applicationDidBecomeActive(_ notification: Notification) { openWindow() }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        openWindow(); return false
    }
    private func openWindow() {
        let refresh = window?.isVisible == false
        window?.makeKeyAndOrderFront(nil)
        if refresh, let model, let assistant { Task { await model.refresh(); await assistant.refresh() } }
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        model?.cancel(); assistant?.cancel(); sender.orderOut(nil); NSApp.hide(nil); return false
    }
}
struct PermissionPanelView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var model: PermissionPanelModel
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(BrowserStrings.text("Extension Permissions")).font(.title2)
                Spacer()
                Button(BrowserStrings.text("Refresh")) { Task { await model.refresh() } }.accessibilityIdentifier("permissions-refresh")
            }
            Text(BrowserStrings.text("Only your visible confirmation changes permissions. Closing this window cancels an unfinished decision."))
                .font(.caption).foregroundStyle(.secondary)
            if let notice = model.notice { Text(BrowserStrings.text(notice)).textSelection(.enabled).accessibilityIdentifier("permissions-notice") }
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if let snapshot = model.snapshot {
                        if let installed = snapshot.installed {
                            Text(installed.name).font(.headline).accessibilityIdentifier("permission-package-name")
                            Text(BrowserStrings.format("Version %@", installed.version)).font(.caption)
                            Text(installed.extension_id).font(.caption).textSelection(.enabled).accessibilityIdentifier("permission-package-id")
                            ForEach(installed.optional) { entry in
                                VStack(alignment: .leading, spacing: 6) {
                                    HStack {
                                        Text(entry.capability).font(.headline)
                                        Spacer()
                                        Text(BrowserStrings.text(entry.granted ? "Allowed" : "Not allowed")).accessibilityIdentifier("permission-state-\(entry.capability)")
                                        Button(BrowserStrings.text(entry.granted ? "Revoke…" : "Allow…")) { model.prepareChange(entry) }
                                            .accessibilityIdentifier("permission-change-\(entry.capability)")
                                    }
                                    origins(entry.origins)
                                }
                            }
                            if installed.optional.isEmpty { Text(BrowserStrings.text("No optional permissions declared.")) }
                            if !installed.runtime_ephemeral.isEmpty {
                                Divider()
                                Text(BrowserStrings.text("One-time page access")).font(.headline)
                                Text(BrowserStrings.text("Review the exact live document, then allow one DOM read. Passwords remain protected.")).font(.caption)
                                origins(installed.runtime_ephemeral.first?.origins ?? [])
                                Picker(BrowserStrings.text("Page"), selection: $model.selectedTab) {
                                    Text(BrowserStrings.text("Choose a page")).tag(Optional<UInt64>.none)
                                    ForEach(model.tabs) { tab in Text(BrowserStrings.format("Tab %llu: %@", tab.id, tab.url ?? BrowserStrings.text("Blank page"))).tag(Optional(tab.id)) }
                                }.accessibilityIdentifier("permission-tab")
                                Button(BrowserStrings.text("Review document…")) { Task { await model.reviewOneShot() } }
                                    .disabled(model.selectedTab == nil).accessibilityIdentifier("permission-review-document")
                            }
                        } else { Text(BrowserStrings.text("No extension installed.")).accessibilityIdentifier("permissions-empty") }
                    } else if model.busy { ProgressView(BrowserStrings.text("Inspecting permissions…")) }
                    else { Text(BrowserStrings.text("Permission state unavailable. Refresh to inspect the active core.")) }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            if let confirmation = model.confirmation, let installed = confirmation.snapshot.installed {
                Divider()
                Text(BrowserStrings.format(confirmation.permission.granted ? "Revoke %@ for %@?" : "Allow %@ for %@?",
                    confirmation.permission.capability, installed.name))
                    .font(.headline).accessibilityIdentifier("permission-confirmation")
                Text(BrowserStrings.text("This decision applies to the displayed package and current core session.")).font(.caption)
                HStack {
                    Button(BrowserStrings.text("Cancel")) { model.cancel() }.accessibilityIdentifier("permission-cancel")
                    Spacer()
                    Button(BrowserStrings.text(confirmation.permission.granted ? "Revoke" : "Allow")) { Task { await model.confirmChange() } }
                        .accessibilityIdentifier("permission-confirm")
                }
            }
            if let review = model.review {
                Divider()
                Text(BrowserStrings.format("Allow one DOM read for %@?", review.installed.name)).font(.headline)
                Text(review.url).textSelection(.enabled).accessibilityIdentifier("permission-reviewed-url")
                Text(BrowserStrings.format("Tab %llu · Document %llu", review.tab_id, review.document_epoch)).font(.caption)
                HStack {
                    Button(BrowserStrings.text("Cancel")) { model.cancel() }.accessibilityIdentifier("permission-cancel")
                    Spacer()
                    Button(BrowserStrings.text("Allow one read")) { Task { await model.confirmOneShot() } }.accessibilityIdentifier("permission-confirm-one-shot")
                }
            }
        }.padding(20).frame(minWidth: 560, minHeight: 460).disabled(model.busy)
    }
    private func origins(_ values: [String]) -> some View {
        Text(values.isEmpty ? BrowserStrings.text("Scope: all origins permitted by the installed declaration") : BrowserStrings.format("Scope: %@", values.joined(separator: ", ")))
            .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
    }
}
