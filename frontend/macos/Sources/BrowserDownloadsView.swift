// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import SwiftUI

struct BrowserDownloadsCommands: Commands {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var model: BrowserModel
    var body: some Commands {
        CommandGroup(after: .newItem) {
            Button(BrowserStrings.text("Downloads…")) { model.downloadsPresented = true }
                .keyboardShortcut("l",modifiers: [.command,.option]).disabled(!model.canInteract)
        }
    }
}
struct BrowserDownloadsView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var downloads: BrowserDownloadsModel
    @Environment(\.dismiss) private var dismiss
    @State private var url = ""
    @State private var name = ""
    @State private var credentialsPresented = false
    private var valid: Bool {
        guard let address = URL(string: url.trimmingCharacters(in: .whitespacesAndNewlines)),
              ["http","https","ftp","ftps","sftp"].contains(address.scheme?.lowercased() ?? ""),
              address.host?.isEmpty == false, address.password == nil else { return false }
        let text = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty || (text.utf8.count <= 255 && text != "." && text != ".." && !text.contains(where: { "/\\:\0".contains($0) }))
    }
    var body: some View {
        Group {
            if credentialsPresented {
                BrowserDownloadCredentialsView(downloads: downloads) { credentialsPresented = false }
            } else {
                downloadList
            }
        }.padding(20).frame(width: 640,height: 570).buttonStyle(.bordered)
            .task { await downloads.connect() }
    }
    private var downloadList: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(BrowserStrings.text("Downloads")).font(.title2).accessibilityIdentifier("downloads-title")
                Spacer()
                Button(BrowserStrings.text("Credentials…")) { credentialsPresented = true }
                    .accessibilityIdentifier("download-credentials")
                Button(BrowserStrings.text("Refresh")) { Task { await downloads.refresh() } }.disabled(!downloads.available).accessibilityIdentifier("downloads-refresh")
                Button(BrowserStrings.text("Done")) { dismiss() }.keyboardShortcut(.cancelAction).accessibilityIdentifier("downloads-close")
            }
            TextField(BrowserStrings.text("Download URL"),text: $url).textFieldStyle(.roundedBorder).accessibilityIdentifier("download-url")
            HStack {
                TextField(BrowserStrings.text("File name (optional)"),text: $name).textFieldStyle(.roundedBorder).accessibilityIdentifier("download-name")
                Button(BrowserStrings.text("Download")) {
                    Task {
                        let fileName = name.trimmingCharacters(in: .whitespacesAndNewlines)
                        if await downloads.start(url.trimmingCharacters(in: .whitespacesAndNewlines),name: fileName.isEmpty ? nil : fileName) != nil { url = ""; name = "" }
                    }
                }.disabled(!valid || downloads.starting || downloads.connecting).accessibilityIdentifier("download-start")
            }
            Text(BrowserStrings.format("Saved in %@", downloads.configuration.directory.path)).font(.caption).textSelection(.enabled)
            if let notice = downloads.notice {
                Text(BrowserStrings.text(notice)).foregroundStyle(.red).font(.caption).accessibilityIdentifier("download-notice")
            }
            if !downloads.available {
                HStack {
                    Text(BrowserStrings.text(downloads.connecting ? "Starting downloads…" : "Download service unavailable")).accessibilityIdentifier("download-service-state")
                    if !downloads.connecting { Button(BrowserStrings.text("Retry")) { Task { await downloads.connect() } }.accessibilityIdentifier("download-retry") }
                }
            }
            Divider()
            ScrollView {
                LazyVStack(alignment: .leading,spacing: 16) {
                    if downloads.available && downloads.transfers.isEmpty { Text(BrowserStrings.text("No downloads")).foregroundStyle(.secondary).accessibilityIdentifier("downloads-empty") }
                    ForEach(downloads.transfers) { info in DownloadRow(downloads: downloads,info: info); Divider() }
                }.frame(maxWidth: .infinity,alignment: .leading)
            }
            Text(BrowserStrings.text("Removing a completed item keeps its file.")).font(.caption).foregroundStyle(.secondary)
        }
    }
}
private struct DownloadRow: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var downloads: BrowserDownloadsModel
    let info: DownloadInfo
    private func operation(_ title: String, _ action: String, _ identifier: String) -> some View {
        Button(BrowserStrings.text(title)) { Task { _ = await downloads.perform(action,id: info.id) } }
            .accessibilityIdentifier("download-\(identifier)-\(info.id)")
    }
    var body: some View {
        VStack(alignment: .leading,spacing: 7) {
            HStack {
                Text(info.name).font(.headline).lineLimit(1).accessibilityIdentifier("download-name-\(info.id)")
                Spacer()
                Text(info.state.title).accessibilityIdentifier("download-state-\(info.id)")
            }
            if let fraction = info.fraction { ProgressView(value: fraction).accessibilityLabel(info.progressText) }
            else if info.state == .active || info.state == .awaitingClearance || info.state == .queued { ProgressView().progressViewStyle(.linear).accessibilityLabel(info.state.title) }
            Text(info.progressText).font(.caption).accessibilityIdentifier("download-progress-\(info.id)")
            if info.state == .active {
                Text(BrowserStrings.format("%@/s · %llu connections%@", DownloadInfo.bytes(info.speedBps), UInt64(info.connections),
                    info.etaSecs.map { BrowserStrings.format(" · %llus remaining", $0) } ?? "")).font(.caption)
            }
            if info.state == .paused && !info.resumeSafe { Text(BrowserStrings.text("Resuming starts again from the beginning.")).font(.caption) }
            if let block = info.blocked { Text(block.reason).font(.caption).foregroundStyle(.red).accessibilityIdentifier("download-blocked-\(info.id)") }
            if let error = info.lastError, info.state == .failed { Text(error).font(.caption).foregroundStyle(.red) }
            if downloads.hasQuarantine(info) {
                Text(BrowserStrings.text("Origin recorded for macOS")).font(.caption).foregroundStyle(.secondary)
                    .accessibilityIdentifier("download-quarantine-\(info.id)")
            }
            HStack {
                if [.queued,.awaitingClearance,.active].contains(info.state) { operation("Pause","Pause","pause"); operation("Cancel","Cancel","cancel") }
                if [.paused,.failed,.blocked].contains(info.state) { operation("Resume","Resume","resume") }
                if info.state == .paused { operation("Cancel","Cancel","cancel") }
                if info.state.terminal { operation("Remove from List","Remove","remove") }
                if info.state == .completed {
                    Button(BrowserStrings.text("Open")) { downloads.open(info,reveal: false) }.accessibilityIdentifier("download-open-\(info.id)")
                    Button(BrowserStrings.text("Show in Finder")) { downloads.open(info,reveal: true) }.accessibilityIdentifier("download-reveal-\(info.id)")
                }
            }.disabled(!downloads.available || downloads.busy.contains(info.id))
            DisclosureGroup(BrowserStrings.text("Details")) {
                VStack(alignment: .leading,spacing: 5) {
                    Text(info.url).textSelection(.enabled)
                    ForEach(Array(info.events.enumerated()),id: \.offset) { _, event in Text(event.message).font(.caption) }
                    ForEach(Array(info.segments.enumerated()),id: \.offset) { index, segment in
                        ProgressView(BrowserStrings.format("Connection %llu", UInt64(index + 1)),value: Double(segment.completed),total: Double(max(1,segment.end - segment.start))).font(.caption)
                    }
                }.frame(maxWidth: .infinity,alignment: .leading)
            }.font(.caption)
        }
    }
}

private struct BrowserDownloadCredentialsView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var downloads: BrowserDownloadsModel
    let close: () -> Void
    @State private var url = ""
    @State private var secret = ""
    @State private var kind = DownloadCredentialKind.sftpPassword
    @State private var reviewed: ReviewedDownloadCredential?
    @State private var message: String?
    @State private var lifetime = UUID()
    @State private var operation: Task<Void,Never>?
    private func resetReview() { reviewed = nil; secret = ""; message = nil; lifetime = UUID() }
    private func review() {
        let current = lifetime
        operation = Task {
            let result = await downloads.resolveCredentialTarget(url)
            guard current == lifetime, !Task.isCancelled else { return }
            reviewed = result
            if let result, !kind.supports(result.identity) {
                kind = result.identity.scheme == "ftps" ? .ftpsPassword : .sftpPassword
            }
        }
    }
    private func change(remove: Bool) {
        guard let reviewed else { return }
        let current = lifetime, value = secret, selected = kind
        secret = ""; message = nil
        operation = Task {
            let success = remove ? await downloads.removeCredential(reviewed,kind: selected)
                : await downloads.saveCredential(reviewed,kind: selected,secret: value)
            guard current == lifetime, !Task.isCancelled else { return }
            if success { message = remove ? "Credential removed from macOS Keychain." : "Credential saved in macOS Keychain." }
        }
    }
    var body: some View {
        VStack(alignment: .leading,spacing: 14) {
            HStack {
                Text(BrowserStrings.text("Download credentials")).font(.title2).accessibilityIdentifier("download-credentials-title")
                Spacer()
                Button(BrowserStrings.text("Back to downloads"),action: close).keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("download-credentials-close")
            }
            Text(BrowserStrings.text("Use the same SFTP or FTPS URL as your download. Include the username; keep passwords out of URLs."))
                .font(.callout).fixedSize(horizontal: false,vertical: true)
            TextField(BrowserStrings.text("Account URL"),text: $url).textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("download-credential-url").disabled(downloads.credentialBusy)
            Button(BrowserStrings.text("Review account"),action: review).disabled(url.isEmpty || downloads.credentialBusy)
                .accessibilityIdentifier("download-credential-review")
            if let reviewed {
                let identity = reviewed.identity
                Text(BrowserStrings.format("Account: %@ · %@ · port %llu",identity.username,identity.host,UInt64(identity.port)))
                    .textSelection(.enabled).accessibilityIdentifier("download-credential-account")
                Picker(BrowserStrings.text("Credential type"),selection: $kind) {
                    ForEach(DownloadCredentialKind.allCases.filter { $0.supports(identity) }) { kind in Text(kind.title).tag(kind) }
                }.accessibilityLabel(BrowserStrings.text("Credential type"))
                    .accessibilityIdentifier("download-credential-kind").disabled(downloads.credentialBusy)
                SecureField(BrowserStrings.text("Password or passphrase"),text: $secret).textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("download-credential-secret").disabled(downloads.credentialBusy)
                if kind == .sftpPrivateKeyPassphrase {
                    Text(BrowserStrings.text("The SFTP private key must already be configured for the download service."))
                        .font(.caption).fixedSize(horizontal: false,vertical: true)
                }
                HStack {
                    Button(BrowserStrings.text("Save in Keychain")) { change(remove: false) }
                        .keyboardShortcut(.defaultAction).disabled(secret.isEmpty || secret.utf8.count > 4096 || downloads.credentialBusy)
                        .accessibilityIdentifier("download-credential-save")
                    Button(BrowserStrings.text("Remove from Keychain")) { change(remove: true) }
                        .disabled(downloads.credentialBusy).accessibilityIdentifier("download-credential-remove")
                }
            }
            if let message { Text(BrowserStrings.text(message)).accessibilityIdentifier("download-credential-result") }
            if let notice = downloads.notice {
                Text(BrowserStrings.text(notice)).foregroundStyle(.red).font(.caption)
                    .accessibilityIdentifier("download-credential-notice")
            }
            if downloads.credentialBusy { ProgressView().controlSize(.small).accessibilityLabel(BrowserStrings.text("Updating credential…")) }
            Spacer(minLength: 0)
            Text(BrowserStrings.text("Saved credentials remain in macOS Keychain until you remove them. Server host keys and TLS certificates are still verified before use."))
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false,vertical: true)
        }.onChange(of: url) { resetReview() }
            .onChange(of: kind) { secret = ""; message = nil }
            .onDisappear { lifetime = UUID(); secret = ""; reviewed = nil; operation?.cancel(); operation = nil }
    }
}
