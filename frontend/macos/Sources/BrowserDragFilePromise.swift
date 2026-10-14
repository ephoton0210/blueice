// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Darwin
import UniformTypeIdentifiers

@MainActor
enum NativeDragFilePromises {
    static func provider(_ file: SelectedFile, index: Int, payload: NativeDragPayload) -> NSFilePromiseProvider {
        let delegate = NativeDragFilePromiseDelegate(name: file.name,index: index,payload: payload)
        let type = UTType(mimeType: file.media_type)
        let provider = NSFilePromiseProvider(fileType: type?.conforms(to: .data) == true ? type!.identifier : UTType.data.identifier,
                                             delegate: delegate)
        // The provider's delegate is weak. A receiver can request a promise
        // after the source session ends, so the provider owns this delegate.
        provider.userInfo = delegate
        return provider
    }
}

@MainActor
private final class NativeDragFilePromiseDelegate: NSObject, NSFilePromiseProviderDelegate {
    private let name: String
    private let index: Int
    private let payload: NativeDragPayload
    private let queue = OperationQueue()

    init(name: String,index: Int,payload: NativeDragPayload) {
        self.name = name; self.index = index; self.payload = payload
        queue.maxConcurrentOperationCount = 1
        queue.qualityOfService = .utility
    }

    func filePromiseProvider(_ filePromiseProvider: NSFilePromiseProvider,fileNameForType fileType: String) -> String { name }
    func operationQueue(for filePromiseProvider: NSFilePromiseProvider) -> OperationQueue { queue }

    nonisolated func filePromiseProvider(_ filePromiseProvider: NSFilePromiseProvider,writePromiseTo url: URL,
                                         completionHandler: @escaping (Error?) -> Void) {
        guard url.isFileURL, SelectedFile.validName(url.lastPathComponent) else {
            completionHandler(CocoaError(.fileWriteInvalidFileName)); return
        }
        guard let file = payload.fileSnapshot(index) else {
            completionHandler(CocoaError(.userCancelled)); return
        }
        do {
            // AppKit supplies the explicit receiver URL. Never manufacture a
            // path from page data or replace a receiver's existing document.
            try Self.write(file.bytes,to: url)
            completionHandler(nil)
        } catch { completionHandler(error) }
    }

    nonisolated private static func write(_ bytes: [UInt8],to url: URL) throws {
        func failure() -> POSIXError { POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        let directory = Darwin.open(url.deletingLastPathComponent().path,O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)
        guard directory >= 0 else { throw failure() }
        defer { Darwin.close(directory) }
        let temporary = ".blueice-file-promise-" + UUID().uuidString
        let descriptor = openat(directory,temporary,O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC,mode_t(0o600))
        guard descriptor >= 0 else { throw failure() }
        defer { Darwin.close(descriptor); unlinkat(directory,temporary,0) }
        try bytes.withUnsafeBytes { buffer in
            var offset = 0
            while offset < buffer.count {
                let written = Darwin.write(descriptor,buffer.baseAddress!.advanced(by: offset),buffer.count-offset)
                if written < 0 && errno == EINTR { continue }
                guard written > 0 else { throw failure() }
                offset += written
            }
        }
        guard fsync(descriptor) == 0 else { throw failure() }
        // Both names use the retained receiver-directory descriptor. Exclusive
        // rename publishes complete bytes without replacing files or symlinks.
        guard renameatx_np(directory,temporary,directory,url.lastPathComponent,UInt32(RENAME_EXCL)) == 0 else { throw failure() }
    }
}
