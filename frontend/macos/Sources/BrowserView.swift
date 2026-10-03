// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

struct BrowserView: View {
    @ObservedObject var model: BrowserModel
    @ObservedObject private var appearance: BrowserAppearance
    @FocusState private var addressFocused: Bool
    init(model: BrowserModel) { self.model = model; self.appearance = model.appearance }

    var body: some View {
        VStack(spacing: 0) {
            if let workspace = model.windowManager { BrowserWorkspaceNotice(workspace: workspace) }
            BrowserTabStrip(model: model)
            Divider()
            if let error = model.groupError, model.groupEditor == nil {
                HStack {
                    Text(error).accessibilityIdentifier("tab-group-notice")
                    Spacer()
                    button("xmark", "Dismiss tab group notice", "dismiss-tab-group-notice") { model.dismissTabGroupError() }
                }.font(.caption).padding(.horizontal, 12).padding(.vertical, 6)
                Divider()
            }
            HStack(spacing: 12) {
                button("chevron.left", "Back", "back") { model.action(.unit("GoBack")) }
                    .disabled(!model.history.back)
                button("chevron.right", "Forward", "forward") { model.action(.unit("GoForward")) }
                    .disabled(!model.history.forward)
                button("arrow.clockwise", "Reload", "reload") { model.reload() }
                    .keyboardShortcut("r", modifiers: .command)
                TextField("Search or enter address", text: $model.address)
                    .focused($addressFocused)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Address")
                    .accessibilityIdentifier("address")
                    .onSubmit { addressFocused = false; model.navigateAddress() }
                button("arrow.right", "Go", "go") { model.navigateAddress() }
                button("gearshape", "Settings", "settings") {
                    model.action(.values("Navigate", ["url": .string("about:settings")]))
                }
                if let workspace = model.windowManager {
                    BrowserProfileMenu(model: model, workspace: workspace)
                    button("arrow.down.circle", "Downloads", "downloads") { model.downloadsPresented = true }
                }
            }
            .disabled(!model.ready || model.selected == nil)
            .padding(12)
            Divider()
            if model.findVisible {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass").accessibilityHidden(true)
                    FindField(model: model).frame(minWidth: 150, maxWidth: 300)
                    Text(model.findSummary).font(.caption).accessibilityLabel(model.findSummary).accessibilityIdentifier("find-results")
                    Spacer()
                    Toggle("Match case", isOn: Binding(get: { model.findCaseSensitive }, set: model.setFindCaseSensitive))
                        .toggleStyle(.checkbox).accessibilityIdentifier("find-case")
                    button("chevron.up", "Previous match", "find-previous") { model.findNext(backwards: true) }
                        .disabled(model.findResult?.matchCount ?? 0 == 0)
                    button("chevron.down", "Next match", "find-next") { model.findNext() }
                        .disabled(model.findResult?.matchCount ?? 0 == 0)
                    button("xmark", "Close find", "find-close") { model.closeFind() }
                }
                .disabled(!model.ready || model.selected == nil)
                .padding(.horizontal, 12).padding(.vertical, 8)
                Divider()
            }
            ZStack {
                PageViewport(model: model)
                if model.image == nil {
                    Text(model.selected == nil ? "Open a new tab" : "Waiting for page…")
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("empty-page")
                        .allowsHitTesting(false)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            Divider()
            HStack {
                Text(model.status).lineLimit(2).accessibilityIdentifier("status")
                Spacer()
                Button("\(model.zoomPercent)%") { model.setZoom(1) }
                    .accessibilityLabel("Page zoom")
                    .accessibilityValue("\(model.zoomPercent)%")
                    .accessibilityIdentifier("page-zoom")
                    .help("Reset page zoom")
                    .disabled(!model.ready || model.selected == nil)
            }
            .font(.caption).foregroundStyle(appearance.resolved.highContrast ? .primary : .secondary).padding(.horizontal, 12).padding(.vertical, 6)
        }
        .alert("Resend form data?", isPresented: $model.resubmissionPresented, presenting: model.resubmission) { prompt in
            Button("Resend") { model.resolveResubmission(prompt.confirmationID, accept: true) }
            Button("Cancel", role: .cancel) { model.resolveResubmission(prompt.confirmationID, accept: false) }
        } message: { prompt in
            Text("Resending will repeat the previous form submission to \(URL(string: prompt.url)?.host ?? prompt.url).")
        }
        .alert("Could not print",isPresented: $model.printErrorPresented) { Button("OK",role: .cancel) {} } message: { Text(model.printError) }
        .alert("Could not select files",isPresented: $model.fileInputErrorPresented) { Button("OK",role: .cancel) {} } message: { Text(model.fileInputError) }
        .sheet(item: $model.groupEditor) { editor in BrowserTabGroupEditor(model: model, editor: editor) }
        .sheet(item: $model.profileEditor) { editor in
            if let workspace = model.windowManager { BrowserProfileEditor(model: model, workspace: workspace, editor: editor) }
        }
        .sheet(isPresented: $model.downloadsPresented) {
            if let workspace = model.windowManager { BrowserDownloadsView(downloads: workspace.downloads) }
        }
        .buttonStyle(.plain)
        .transaction { if appearance.resolved.reducedMotion { $0.animation = nil; $0.disablesAnimations = true } }
        .background(AppearanceWindow(settings: appearance).frame(width: 0, height: 0).accessibilityHidden(true))
        .frame(minWidth: 720, minHeight: 480)
        .onChange(of: model.addressFocusSerial) { _, _ in addressFocused = true }
        .onChange(of: model.findFocusSerial) { _, _ in addressFocused = false }
    }

    private func button(_ symbol: String, _ label: String, _ identifier: String, action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: symbol).frame(width: 24, height: 24) }
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(appearance.resolved.highContrast ? Color.primary : Color.clear, lineWidth: 1).allowsHitTesting(false).accessibilityHidden(true))
            .help(label).accessibilityLabel(label).accessibilityIdentifier(identifier)
    }
}

private struct FindField: NSViewRepresentable {
    @ObservedObject var model: BrowserModel
    func makeCoordinator() -> Coordinator { Coordinator(model) }
    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.placeholderString = "Find in page"
        field.setAccessibilityLabel("Find in page")
        field.setAccessibilityIdentifier("find-query")
        field.delegate = context.coordinator
        field.sendsSearchStringImmediately = true
        return field
    }
    func updateNSView(_ field: NSSearchField, context: Context) {
        let coordinator = context.coordinator
        coordinator.synchronizing = true
        defer { coordinator.synchronizing = false }
        let changedTab = coordinator.tab != model.selected
        coordinator.tab = model.selected
        if changedTab { field.abortEditing() }
        let composing = (field.currentEditor() as? NSTextView)?.hasMarkedText() == true
        // AppKit owns temporary marked text. A repaint must not replace a
        // pending dead key/IME sequence with the last committed model query.
        if !composing && field.stringValue != model.findQuery { field.stringValue = model.findQuery }
        field.isEnabled = model.ready && model.selected != nil
        guard context.coordinator.serial != model.findFocusSerial else { return }
        context.coordinator.serial = model.findFocusSerial
        let serial = model.findFocusSerial
        let tab = model.selected
        Task { @MainActor [weak field, weak model] in
            await Task.yield()
            guard let field, let model, model.findVisible, model.selected == tab, model.findFocusSerial == serial else { return }
            field.window?.makeFirstResponder(field)
            field.selectText(nil)
        }
    }
    final class Coordinator: NSObject, NSSearchFieldDelegate {
        weak var model: BrowserModel?
        var serial: UInt64?
        var tab: UInt64?
        var synchronizing = false
        init(_ model: BrowserModel) { self.model = model }
        func controlTextDidChange(_ notification: Notification) {
            guard !synchronizing, let field = notification.object as? NSSearchField,
                  (field.currentEditor() as? NSTextView)?.hasMarkedText() != true else { return }
            model?.setFindQuery(field.stringValue)
        }
        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            if selector == #selector(NSResponder.insertNewline(_:)) {
                model?.findNext(backwards: NSApp.currentEvent?.modifierFlags.contains(.shift) == true)
                return true
            }
            if selector == #selector(NSResponder.cancelOperation(_:)) { model?.closeFind(); return true }
            return false
        }
    }
}
