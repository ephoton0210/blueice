// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

@MainActor
private final class SFTPFilePicker: ObservableObject {
    weak var window: NSWindow?
    private var panel: NSOpenPanel?
    @Published private(set) var choosing = false
    func choose(privateKey: Bool) async throws -> URL? {
        guard !choosing, let window, window.isVisible else { return nil }
        choosing = true; defer { choosing = false; panel = nil }
        let panel = NSOpenPanel(); self.panel = panel
        panel.title = BrowserStrings.text(privateKey ? "Choose SFTP Private Key" : "Choose SSH Known Hosts")
        panel.prompt = BrowserStrings.text("Choose")
        panel.canChooseFiles = true; panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false; panel.resolvesAliases = false
        panel.treatsFilePackagesAsDirectories = true
        let response = await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                panel.beginSheetModal(for: window) { response in continuation.resume(returning: response) }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.cancel() }
        }
        guard response == .OK, !Task.isCancelled, self.window === window, window.isVisible,
              let selected = panel.url else { return nil }
        return try DownloadSFTPConfiguration.checkedFile(selected)
    }
    func cancel() { panel?.cancel(nil) }
}

private struct SFTPFilePickerAnchor: NSViewRepresentable {
    let picker: SFTPFilePicker
    final class Anchor: NSView {
        weak var picker: SFTPFilePicker?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); picker?.window = window }
    }
    func makeNSView(context: Context) -> Anchor {
        let view = Anchor(); view.picker = picker; return view
    }
    func updateNSView(_ view: Anchor,context: Context) { view.picker = picker; picker.window = view.window }
}

struct BrowserSFTPConfigurationView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var downloads: BrowserDownloadsModel
    @ObservedObject var preferences: BrowserSFTPPreferences
    let close: () -> Void
    @StateObject private var picker = SFTPFilePicker()
    @State private var knownHosts: URL?
    @State private var privateKey: URL?
    @State private var error: String?
    @State private var message: String?
    @State private var lifetime = UUID()
    @State private var operation: Task<Void,Never>?
    private func choose(privateKey: Bool) {
        let current = lifetime
        operation = Task {
            do {
                guard let url = try await picker.choose(privateKey: privateKey), current == lifetime else { return }
                if privateKey { self.privateKey = url } else { knownHosts = url }
                error = nil; message = nil
            } catch { if current == lifetime { self.error = error.localizedDescription } }
        }
    }
    private func apply(restore: Bool) {
        let current = lifetime, proposed = DownloadSFTPConfiguration(knownHosts: knownHosts,privateKey: privateKey)
        error = nil; message = nil
        operation = Task {
            let success = restore ? await downloads.restoreSFTPDefaults() : await downloads.applySFTPConfiguration(proposed)
            guard current == lifetime, !Task.isCancelled else { return }
            if success {
                knownHosts = preferences.configuration?.knownHosts; privateKey = preferences.configuration?.privateKey
                message = "SSH file settings applied. Interrupted downloads remain paused."
            }
        }
    }
    var body: some View {
        VStack(alignment: .leading,spacing: 14) {
            HStack {
                Text(BrowserStrings.text("SFTP files")).font(.title2).accessibilityIdentifier("download-sftp-title")
                Spacer()
                Button(BrowserStrings.text("Back to downloads"),action: close).accessibilityIdentifier("download-sftp-close")
            }
            ScrollView { VStack(alignment: .leading,spacing: 14) {
            Text(BrowserStrings.text("Choose local SSH files for SFTP. Private-key contents stay on this Mac; passphrases are managed in Credentials."))
                .font(.callout).fixedSize(horizontal: false,vertical: true)
            GroupBox(BrowserStrings.text("SSH known hosts")) {
                VStack(alignment: .leading,spacing: 8) {
                    Text(knownHosts?.path ?? BrowserStrings.text("Use ~/.ssh/known_hosts"))
                        .textSelection(.enabled).fixedSize(horizontal: false,vertical: true).accessibilityIdentifier("download-sftp-hosts-path")
                    HStack {
                        Button(BrowserStrings.text("Choose Known Hosts…")) { choose(privateKey: false) }.accessibilityIdentifier("download-sftp-hosts-choose")
                        Button(BrowserStrings.text("Use default")) { knownHosts = nil; message = nil }.accessibilityIdentifier("download-sftp-hosts-default")
                    }
                }.frame(maxWidth: .infinity,alignment: .leading)
            }
            GroupBox(BrowserStrings.text("SFTP private key")) {
                VStack(alignment: .leading,spacing: 8) {
                    Text(privateKey?.path ?? BrowserStrings.text("No private-key file"))
                        .textSelection(.enabled).fixedSize(horizontal: false,vertical: true).accessibilityIdentifier("download-sftp-key-path")
                    HStack {
                        Button(BrowserStrings.text("Choose Private Key…")) { choose(privateKey: true) }.accessibilityIdentifier("download-sftp-key-choose")
                        Button(BrowserStrings.text("Use no private-key file")) { privateKey = nil; message = nil }.accessibilityIdentifier("download-sftp-key-default")
                    }
                }.frame(maxWidth: .infinity,alignment: .leading)
            }
            Text(BrowserStrings.text("Applying settings restarts the download service and pauses unfinished downloads. Resume them explicitly after checking the settings."))
                .font(.caption).fixedSize(horizontal: false,vertical: true)
            HStack {
                Button(BrowserStrings.text("Apply and pause downloads")) { apply(restore: false) }
                    .keyboardShortcut(.defaultAction).accessibilityIdentifier("download-sftp-apply")
                Button(BrowserStrings.text("Restore SSH defaults")) { apply(restore: true) }.accessibilityIdentifier("download-sftp-restore")
            }
            if let error = error ?? preferences.configurationError { Text(BrowserStrings.text(error)).foregroundStyle(.red).accessibilityIdentifier("download-sftp-error") }
            if let notice = downloads.notice { Text(BrowserStrings.text(notice)).foregroundStyle(.red).accessibilityIdentifier("download-sftp-notice") }
            if let message { Text(BrowserStrings.text(message)).accessibilityIdentifier("download-sftp-result") }
            if downloads.configuringSFTP { ProgressView().controlSize(.small).accessibilityLabel(BrowserStrings.text("Applying SSH file settings…")) }
            }.frame(maxWidth: .infinity,alignment: .leading) }
            Text(BrowserStrings.text("Host-key verification remains required. Changing files grants no server trust and never resumes a transfer automatically."))
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false,vertical: true)
        }.background(SFTPFilePickerAnchor(picker: picker).frame(width: 0,height: 0).accessibilityHidden(true))
            .disabled(picker.choosing || downloads.configuringSFTP || !downloads.canConfigureSFTP)
            .onAppear { knownHosts = preferences.configuration?.knownHosts; privateKey = preferences.configuration?.privateKey }
            .onDisappear { lifetime = UUID(); operation?.cancel(); picker.cancel(); operation = nil }
    }
}
