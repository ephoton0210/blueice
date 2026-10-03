// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

@main
struct BlueIcePanelsApp: App {
    @NSApplicationDelegateAdaptor(PermissionPanelDelegate.self) private var delegate
    var body: some Scene { Settings { EmptyView() } }
}
@MainActor
final class PermissionPanelDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var window: NSWindow?
    private var model: PermissionPanelModel?
    private var assistant: AssistantSettingsModel?
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
            window.title = "BlueIce Browser Panels"; window.identifier = .init("permissions-window")
            window.isReleasedWhenClosed = false; window.delegate = self; window.center()
            window.contentView = NSHostingView(rootView: TrustedPanelsView(permissions: model, assistant: assistant))
            self.window = window
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
    @ObservedObject var model: PermissionPanelModel
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Extension Permissions").font(.title2)
                Spacer()
                Button("Refresh") { Task { await model.refresh() } }.accessibilityIdentifier("permissions-refresh")
            }
            Text("Only your visible confirmation changes permissions. Closing this window cancels an unfinished decision.")
                .font(.caption).foregroundStyle(.secondary)
            if let notice = model.notice { Text(notice).textSelection(.enabled).accessibilityIdentifier("permissions-notice") }
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if let snapshot = model.snapshot {
                        if let installed = snapshot.installed {
                            Text(installed.name).font(.headline).accessibilityIdentifier("permission-package-name")
                            Text("Version \(installed.version)").font(.caption)
                            Text(installed.extension_id).font(.caption).textSelection(.enabled).accessibilityIdentifier("permission-package-id")
                            ForEach(installed.optional) { entry in
                                VStack(alignment: .leading, spacing: 6) {
                                    HStack {
                                        Text(entry.capability).font(.headline)
                                        Spacer()
                                        Text(entry.granted ? "Allowed" : "Not allowed").accessibilityIdentifier("permission-state-\(entry.capability)")
                                        Button(entry.granted ? "Revoke…" : "Allow…") { model.prepareChange(entry) }
                                            .accessibilityIdentifier("permission-change-\(entry.capability)")
                                    }
                                    origins(entry.origins)
                                }
                            }
                            if installed.optional.isEmpty { Text("No optional permissions declared.") }
                            if !installed.runtime_ephemeral.isEmpty {
                                Divider()
                                Text("One-time page access").font(.headline)
                                Text("Review the exact live document, then allow one DOM read. Passwords remain protected.").font(.caption)
                                origins(installed.runtime_ephemeral.first?.origins ?? [])
                                Picker("Page", selection: $model.selectedTab) {
                                    Text("Choose a page").tag(Optional<UInt64>.none)
                                    ForEach(model.tabs) { tab in Text("Tab \(tab.id): \(tab.url ?? "Blank page")").tag(Optional(tab.id)) }
                                }.accessibilityIdentifier("permission-tab")
                                Button("Review document…") { Task { await model.reviewOneShot() } }
                                    .disabled(model.selectedTab == nil).accessibilityIdentifier("permission-review-document")
                            }
                        } else { Text("No extension installed.").accessibilityIdentifier("permissions-empty") }
                    } else if model.busy { ProgressView("Inspecting permissions…") }
                    else { Text("Permission state unavailable. Refresh to inspect the active core.") }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            if let confirmation = model.confirmation, let installed = confirmation.snapshot.installed {
                Divider()
                Text("\(confirmation.permission.granted ? "Revoke" : "Allow") \(confirmation.permission.capability) for \(installed.name)?")
                    .font(.headline).accessibilityIdentifier("permission-confirmation")
                Text("This decision applies to the displayed package and current core session.").font(.caption)
                HStack {
                    Button("Cancel") { model.cancel() }.accessibilityIdentifier("permission-cancel")
                    Spacer()
                    Button(confirmation.permission.granted ? "Revoke" : "Allow") { Task { await model.confirmChange() } }
                        .accessibilityIdentifier("permission-confirm")
                }
            }
            if let review = model.review {
                Divider()
                Text("Allow one DOM read for \(review.installed.name)?").font(.headline)
                Text(review.url).textSelection(.enabled).accessibilityIdentifier("permission-reviewed-url")
                Text("Tab \(review.tab_id) · Document \(review.document_epoch)").font(.caption)
                HStack {
                    Button("Cancel") { model.cancel() }.accessibilityIdentifier("permission-cancel")
                    Spacer()
                    Button("Allow one read") { Task { await model.confirmOneShot() } }.accessibilityIdentifier("permission-confirm-one-shot")
                }
            }
        }.padding(20).frame(minWidth: 560, minHeight: 460).disabled(model.busy)
    }
    private func origins(_ values: [String]) -> some View {
        Text(values.isEmpty ? "Scope: all origins permitted by the installed declaration" : "Scope: " + values.joined(separator: ", "))
            .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
    }
}
