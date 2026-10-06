// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Darwin
import UniformTypeIdentifiers

struct FileInputContext: Codable, Equatable, Sendable {
    let tab_id: UInt64
    let frame_source: UInt64
    let document_generation: UInt64
    let node_id: UInt64
    let revision: UInt64
}
struct FileInputState: Decodable, Sendable {
    let context: FileInputContext
    let multiple: Bool
    let accept: String
    let names: [String]
}
struct SelectedFile: Encodable, Sendable {
    let name: String
    let media_type: String
    let bytes: [UInt8]
    static let maximumBytes = 1_048_576
    static func validName(_ name: String) -> Bool {
        !name.isEmpty && name.utf8.count <= 255 && name != "." && name != ".."
            && !name.unicodeScalars.contains { $0.properties.generalCategory == .control || "/\\:".unicodeScalars.contains($0) }
    }
    // This function is called only with URLs returned by the user's native
    // panel. Neither page content nor an incoming browser message supplies URLs.
    static func read(_ urls: [URL], multiple: Bool) throws -> [SelectedFile] {
        guard urls.count <= 16, multiple || urls.count <= 1 else { throw BrowserFailure.invalid("Too many files selected.") }
        var result: [SelectedFile] = []; var total = 0
        for url in urls {
            guard url.isFileURL, validName(url.lastPathComponent) else { throw BrowserFailure.invalid("Unsupported selected filename.") }
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            let descriptor = Darwin.open(url.path,O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
            guard descriptor >= 0 else { throw BrowserFailure.invalid("Could not read the selected file.") }
            let handle = FileHandle(fileDescriptor: descriptor,closeOnDealloc: true)
            defer { try? handle.close() }
            var before = stat()
            guard fstat(descriptor,&before) == 0, before.st_mode & S_IFMT == S_IFREG,
                  before.st_size >= 0, before.st_size <= maximumBytes - total else {
                throw BrowserFailure.invalid("Select regular files with at most 1 MiB of total content.")
            }
            var data = Data()
            while data.count <= maximumBytes - total {
                let chunk = try handle.read(upToCount: min(65536,maximumBytes - total - data.count + 1)) ?? Data()
                if chunk.isEmpty { break }; data.append(chunk)
            }
            var after = stat()
            guard data.count <= maximumBytes - total, fstat(descriptor,&after) == 0,
                  before.st_size == after.st_size, before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
                  before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec, data.count == before.st_size else {
                throw BrowserFailure.invalid("The selected file changed or exceeded the content limit.")
            }
            total += data.count
            let mime = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
            result.append(SelectedFile(name: url.lastPathComponent,media_type: mime,bytes: Array(data)))
        }
        return result
    }
}
enum FileInputAction: Encodable, Sendable {
    case prepare(UInt64,UInt64,UInt64), set(FileInputContext,[SelectedFile])
    private enum Keys: String,CodingKey { case Prepare, Set }
    private struct Prepare: Encodable { let frame_source: UInt64; let document_generation: UInt64; let node_id: UInt64 }
    private struct Set: Encodable { let context: FileInputContext; let files: [SelectedFile] }
    func encode(to encoder: Encoder) throws {
        var value = encoder.container(keyedBy: Keys.self)
        switch self {
        case .prepare(let source,let document,let node): try value.encode(Prepare(frame_source: source,document_generation: document,node_id: node),forKey: .Prepare)
        case .set(let context,let files): try value.encode(Set(context: context,files: files),forKey: .Set)
        }
    }
}

@MainActor
final class BrowserFilePicker {
    private var task: Task<Void,Never>?
    private var panel: NSOpenPanel?
    private var context: FileInputContext?
    private var tab: UInt64?
    private weak var model: BrowserModel?
    static func contentTypes(_ accept: String) -> [UTType] {
        var types: [UTType] = []
        for raw in accept.split(separator: ",").prefix(64) {
            let token = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            let type: UTType?
            if token.hasPrefix(".") { type = UTType(filenameExtension: String(token.dropFirst())) }
            else if token == "image/*" { type = .image }
            else if token == "audio/*" { type = .audio }
            else if token == "video/*" { type = .movie }
            else { type = UTType(mimeType: token) }
            if let type, !types.contains(type) { types.append(type) }
        }
        return types
    }
    func present(node: PageNode, model: BrowserModel, window: NSWindow) {
        guard task == nil, node.state.fileInput, !node.state.disabled else { return }
        self.model = model
        task = Task { @MainActor [weak self, weak model, weak window] in
            guard let self, let model, let window else { return }
            defer { self.task = nil; self.panel = nil; self.context = nil; self.tab = nil }
            guard let tab = model.selected, let state = await model.prepareFileInput(node.id),
                  !Task.isCancelled, model.fileInputIsCurrent(state.context,tab: tab), window.isVisible else { return }
            self.context = state.context; self.tab = tab
            let panel = NSOpenPanel(); self.panel = panel
            panel.title = BrowserStrings.text("Choose Files"); panel.prompt = BrowserStrings.text("Choose")
            panel.canChooseFiles = true; panel.canChooseDirectories = false
            panel.treatsFilePackagesAsDirectories = true; panel.resolvesAliases = false
            panel.allowsMultipleSelection = state.multiple
            let types = Self.contentTypes(state.accept)
            if !types.isEmpty { panel.allowedContentTypes = types }
            let response = await withCheckedContinuation { continuation in
                panel.beginSheetModal(for: window) { response in continuation.resume(returning: response) }
            }
            guard response == .OK, !Task.isCancelled, model.fileInputIsCurrent(state.context,tab: tab) else { return }
            do {
                let urls = panel.urls; let multiple = state.multiple
                let files = try await Task.detached(priority: .userInitiated) { try SelectedFile.read(urls,multiple: multiple) }.value
                guard !Task.isCancelled, model.fileInputIsCurrent(state.context,tab: tab) else { return }
                _ = await model.setFileInput(state.context,files: files,tab: tab)
            } catch {
                if !Task.isCancelled, model.fileInputIsCurrent(state.context,tab: tab) {
                    model.presentFileInputError(error.localizedDescription)
                }
            }
        }
    }
    func invalidateIfNeeded() {
        if let context, let tab, model?.fileInputIsCurrent(context,tab: tab) != true { cancel() }
    }
    func cancel() { task?.cancel(); panel?.cancel(nil) }
}
