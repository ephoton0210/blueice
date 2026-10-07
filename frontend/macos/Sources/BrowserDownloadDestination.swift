// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine
import CoreFoundation
import SwiftUI

struct DownloadFolderSelection: Equatable {
    let directory: URL?
    let previousDirectories: [URL]
    init(directory: URL? = nil, previousDirectories: [URL] = []) {
        self.directory = directory; self.previousDirectories = previousDirectories
    }
    private static func path(_ value: Any) throws -> URL {
        guard let text = value as? String, text.hasPrefix("/"), text.utf8.count <= 4096,
              !text.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains) else {
            throw BrowserFailure.invalid("The saved download folder settings are unsupported. Choose a folder explicitly.")
        }
        return URL(fileURLWithPath: text).standardizedFileURL
    }
    static func decode(_ value: Any?) throws -> Self {
        guard let value else { return Self() }
        guard let fields = value as? [String: Any], let version = fields["version"] as? NSNumber,
              CFGetTypeID(version) != CFBooleanGetTypeID(), version.doubleValue == 1,
              let previous = fields["previous_directories"] as? [String], previous.count <= 32 else {
            throw BrowserFailure.invalid("The saved download folder settings are unsupported. Choose a folder explicitly.")
        }
        return try Self(directory: fields["directory"].map(path), previousDirectories: previous.map(path))
    }
    var preferenceValue: [String: Any] {
        var value: [String: Any] = ["version": 1,"previous_directories": previousDirectories.map(\.path)]
        if let directory { value["directory"] = directory.path }
        return value
    }
    static func checkedDirectory(_ url: URL) throws -> URL {
        guard url.isFileURL, url.host == nil || url.host == "" || url.host == "localhost" else {
            throw BrowserFailure.invalid("Choose a readable and writable local folder.")
        }
        let file = try path(url.path)
        let values = try file.resourceValues(forKeys: [.isDirectoryKey,.isSymbolicLinkKey,.isAliasFileKey])
        guard values.isDirectory == true, values.isSymbolicLink != true, values.isAliasFile != true,
              FileManager.default.isReadableFile(atPath: file.path), FileManager.default.isWritableFile(atPath: file.path) else {
            throw BrowserFailure.invalid("Choose a readable and writable local folder.")
        }
        return file.resolvingSymlinksInPath().standardizedFileURL
    }
    func resolved(base: DownloadConfiguration) throws -> DownloadConfiguration {
        let chosen = try directory.map(Self.checkedDirectory) ?? base.directory
        let previous = try previousDirectories.map { url -> URL in
            do { return try Self.checkedDirectory(url) }
            catch { throw BrowserFailure.invalid("A previous download folder is unavailable. Restore its location to keep the download history.") }
        }
        return DownloadConfiguration(directory: chosen,dataDirectory: base.dataDirectory,previousDirectories: previous)
    }
    func changing(to proposed: URL?, base: DownloadConfiguration) throws -> Self {
        let current = try resolved(base: base)
        let selected = try proposed.map(Self.checkedDirectory)
        let target = (selected ?? base.directory).resolvingSymlinksInPath().standardizedFileURL
        var previous = current.previousDirectories
        if FileManager.default.fileExists(atPath: current.directory.path) {
            previous.append(try Self.checkedDirectory(current.directory))
        }
        var unique: [URL] = []
        for url in previous where url.path != target.path && !unique.contains(where: { $0.path == url.path }) { unique.append(url) }
        guard unique.count <= 32 else { throw BrowserFailure.invalid("At most 32 previous download folders are supported.") }
        return Self(directory: selected,previousDirectories: unique)
    }
}

@MainActor
final class BrowserDownloadFolderPreferences: ObservableObject {
    static let configurationKey = "browser.downloads.destination.configuration"
    private let defaults: UserDefaults
    @Published private(set) var selection: DownloadFolderSelection?
    @Published private(set) var error: String?
    init(defaults: UserDefaults) {
        self.defaults = defaults
        do { selection = try DownloadFolderSelection.decode(defaults.object(forKey: Self.configurationKey)) }
        catch { self.error = error.localizedDescription }
    }
    func save(_ selection: DownloadFolderSelection, base: DownloadConfiguration) throws {
        _ = try selection.resolved(base: base)
        defaults.set(selection.preferenceValue,forKey: Self.configurationKey)
        self.selection = selection; error = nil
    }
}

@MainActor
private final class DownloadFolderPicker: ObservableObject {
    weak var window: NSWindow?
    private var panel: NSOpenPanel?
    @Published private(set) var choosing = false
    func choose() async throws -> URL? {
        guard !choosing, let window, window.isVisible else { return nil }
        choosing = true; defer { choosing = false; panel = nil }
        let panel = NSOpenPanel(); self.panel = panel
        panel.title = BrowserStrings.text("Choose Download Folder")
        panel.prompt = BrowserStrings.text("Choose")
        panel.canChooseFiles = false; panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false; panel.resolvesAliases = false
        panel.canCreateDirectories = true
        let response = await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                panel.beginSheetModal(for: window) { continuation.resume(returning: $0) }
            }
        } onCancel: { Task { @MainActor [weak self] in self?.cancel() } }
        guard response == .OK, !Task.isCancelled, self.window === window, window.isVisible,
              let url = panel.url else { return nil }
        return try DownloadFolderSelection.checkedDirectory(url)
    }
    func cancel() { panel?.cancel(nil) }
}

private struct DownloadFolderPickerAnchor: NSViewRepresentable {
    let picker: DownloadFolderPicker
    final class Anchor: NSView {
        weak var picker: DownloadFolderPicker?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); picker?.window = window }
    }
    func makeNSView(context: Context) -> Anchor { let view = Anchor(); view.picker = picker; return view }
    func updateNSView(_ view: Anchor,context: Context) { view.picker = picker; picker.window = view.window }
}

struct BrowserDownloadFolderView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var downloads: BrowserDownloadsModel
    @ObservedObject var preferences: BrowserDownloadFolderPreferences
    let close: () -> Void
    @StateObject private var picker = DownloadFolderPicker()
    @State private var directory: URL?
    @State private var error: String?
    @State private var message: String?
    @State private var lifetime = UUID()
    @State private var operation: Task<Void,Never>?
    var body: some View {
        VStack(alignment: .leading,spacing: 14) {
            HStack {
                Text(BrowserStrings.text("Download folder")).font(.title2).accessibilityIdentifier("download-folder-title")
                Spacer()
                Button(BrowserStrings.text("Back to downloads"),action: close).accessibilityIdentifier("download-folder-close")
            }
            ScrollView { VStack(alignment: .leading,spacing: 14) {
                Text(BrowserStrings.text("Choose where new downloads are saved. Existing downloads keep their original folders and files."))
                Text(directory?.path ?? downloads.defaultDirectory.path).textSelection(.enabled)
                    .fixedSize(horizontal: false,vertical: true).accessibilityIdentifier("download-folder-path")
                HStack {
                    Button(BrowserStrings.text("Choose Folder…")) {
                        let current = lifetime
                        operation = Task {
                            do {
                                guard let url = try await picker.choose(), current == lifetime else { return }
                                directory = url; error = nil; message = nil
                            } catch { if current == lifetime { self.error = error.localizedDescription } }
                        }
                    }.accessibilityIdentifier("download-folder-choose")
                    Button(BrowserStrings.text("Use default folder")) { directory = nil; message = nil }.accessibilityIdentifier("download-folder-default")
                }
                Text(BrowserStrings.text("Applying settings pauses unfinished downloads. Resume them explicitly; they continue in their original folders."))
                    .font(.caption).fixedSize(horizontal: false,vertical: true)
                Button(BrowserStrings.text("Apply and pause downloads")) {
                    let current = lifetime, proposed = directory
                    error = nil; message = nil
                    operation = Task {
                        let success = await downloads.applyDownloadFolder(proposed)
                        guard current == lifetime, !Task.isCancelled else { return }
                        if success { directory = preferences.selection?.directory; message = "Download folder applied. Existing downloads keep their original locations." }
                    }
                }.keyboardShortcut(.defaultAction).accessibilityIdentifier("download-folder-apply")
                if let error = error ?? preferences.error { Text(BrowserStrings.text(error)).foregroundStyle(.red).accessibilityIdentifier("download-folder-error") }
                if let notice = downloads.notice { Text(BrowserStrings.text(notice)).foregroundStyle(.red).accessibilityIdentifier("download-folder-notice") }
                if let message { Text(BrowserStrings.text(message)).accessibilityIdentifier("download-folder-result") }
                if downloads.configuringDestination { ProgressView().controlSize(.small).accessibilityLabel(BrowserStrings.text("Applying download folder…")) }
                if let previous = preferences.selection?.previousDirectories, !previous.isEmpty {
                    Text(BrowserStrings.text("Original folders retained for download history")).font(.headline)
                    ForEach(previous,id: \.path) { Text($0.path).font(.caption).textSelection(.enabled).fixedSize(horizontal: false,vertical: true) }
                }
            }.frame(maxWidth: .infinity,alignment: .leading) }
        }.background(DownloadFolderPickerAnchor(picker: picker).frame(width: 0,height: 0).accessibilityHidden(true))
            .disabled(picker.choosing || !downloads.canConfigureSFTP)
            .onAppear { directory = preferences.selection?.directory }
            .onDisappear { lifetime = UUID(); operation?.cancel(); picker.cancel(); operation = nil }
    }
}
