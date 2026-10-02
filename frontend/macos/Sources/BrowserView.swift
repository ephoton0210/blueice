// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import SwiftUI

struct BrowserView: View {
    @ObservedObject var model: BrowserModel
    @FocusState private var addressFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                ScrollView(.horizontal) {
                    HStack(spacing: 4) {
                        ForEach(model.tabs) { tab in
                            HStack(spacing: 4) {
                                Button { model.select(tab.id) } label: {
                                    Text(tab.url ?? "New tab").lineLimit(1).frame(maxWidth: 200)
                                }
                                .accessibilityIdentifier("tab-\(tab.id)")
                                .accessibilityLabel(tab.url ?? "New tab")
                                .accessibilityValue(model.selected == tab.id ? "Selected" : "")
                                Button { model.action(.unit("CloseTab"), tab: tab.id) } label: {
                                    Image(systemName: "xmark").font(.caption)
                                }
                                .accessibilityLabel("Close tab")
                                .accessibilityIdentifier("close-tab-\(tab.id)")
                            }
                            .padding(6)
                            .background(model.selected == tab.id ? Color.accentColor.opacity(0.15) : Color.clear)
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                        }
                    }
                }
                button("plus", "New tab", "add-tab") { model.action(.values("OpenTab", ["url": .null])) }
                    .keyboardShortcut("t", modifiers: .command)
            }
            .disabled(!model.ready)
            .padding(.horizontal, 12).padding(.vertical, 8)
            Divider()
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
            }
            .disabled(!model.ready || model.selected == nil)
            .padding(12)
            Divider()
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
            }
            .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 12).padding(.vertical, 6)
        }
        .alert("Resend form data?", isPresented: $model.resubmissionPresented, presenting: model.resubmission) { prompt in
            Button("Resend") { model.resolveResubmission(prompt.confirmationID, accept: true) }
            Button("Cancel", role: .cancel) { model.resolveResubmission(prompt.confirmationID, accept: false) }
        } message: { prompt in
            Text("Resending will repeat the previous form submission to \(URL(string: prompt.url)?.host ?? prompt.url).")
        }
        .buttonStyle(.plain)
        .frame(minWidth: 720, minHeight: 480)
        .onChange(of: model.addressFocusSerial) { _, _ in addressFocused = true }
    }

    private func button(_ symbol: String, _ label: String, _ identifier: String, action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: symbol).frame(width: 24, height: 24) }
            .help(label).accessibilityLabel(label).accessibilityIdentifier(identifier)
    }
}
