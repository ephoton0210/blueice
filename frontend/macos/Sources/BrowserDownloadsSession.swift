// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

final class BrowserDownloadsSession: @unchecked Sendable {
    private let executable: URL
    private let runtime: URL
    private let configuration: DownloadConfiguration
    private let lock = NSLock()
    private let reader = DispatchQueue(label: "cc.blueice.downloads.read", qos: .userInitiated)
    private let writer = DispatchQueue(label: "cc.blueice.downloads.write", qos: .userInitiated)
    private let lifecycle = DispatchQueue(label: "cc.blueice.downloads.lifecycle", qos: .userInitiated)
    private let reading = DispatchGroup()
    private var process: OwnedBrowserProcess?
    private var connection: BrowserConnection?
    private var owner: FileHandle?
    private var stopping = false
    private var ready = false
    private var nextRequest: UInt64 = 1
    var received: (@Sendable (Result<DownloadEnvelope, BrowserFailure>) -> Void)?
    var processID: Int32? { lock.withLock { process?.pid } }
    init(executable: URL, runtime: URL, configuration: DownloadConfiguration) {
        self.executable = executable; self.runtime = runtime; self.configuration = configuration
    }
    func start() async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void,Error>) in
            reading.enter()
            reader.async {
                defer { self.reading.leave() }
                var resumed = false
                var timeout: DispatchWorkItem?
                do {
                    guard FileManager.default.isExecutableFile(atPath: self.executable.path) else { throw BrowserFailure.invalid("BlueIce download service was not found.") }
                    let pipe = Pipe()
                    var environment = ProcessInfo.processInfo.environment
                    environment["TMPDIR"] = self.runtime.path; environment["XDG_RUNTIME_DIR"] = self.runtime.path
                    let socket = self.runtime.appendingPathComponent("blueice/downloads.sock")
                    let child = try self.lock.withLock {
                        guard !self.stopping else { throw BrowserFailure.invalid("Downloads are closed") }
                        let child = try OwnedBrowserProcess(executable: self.executable, arguments: [
                            "--socket", socket.path, "--download-dir", self.configuration.directory.path,
                            "--data-dir", self.configuration.dataDirectory.path,
                            "--gatekeeper-socket", self.runtime.appendingPathComponent("blueice/gatekeeper.sock").path,
                            "--exit-on-stdin-eof"
                        ], environment: environment, input: pipe.fileHandleForReading)
                        self.process = child; self.owner = pipe.fileHandleForWriting; return child
                    }
                    try? pipe.fileHandleForReading.close()
                    let deadline = Date().addingTimeInterval(5)
                    var transport: BrowserConnection?
                    while transport == nil, child.isRunning, Date() < deadline, !self.lock.withLock({ self.stopping }) {
                        transport = try? BrowserConnection(socketPath: socket.path)
                        if transport == nil { Thread.sleep(forTimeInterval: 0.02) }
                    }
                    guard let transport else { throw BrowserFailure.invalid("Download service could not start.") }
                    self.lock.withLock { self.connection = transport }
                    let limit = DispatchWorkItem { transport.interrupt() }
                    timeout = limit; DispatchQueue.global().asyncAfter(deadline: .now() + 5, execute: limit)
                    try transport.output.write(contentsOf: BrowserWire.encode(.values("Hello", ["protocol_version": .unsigned(1)]),tab: nil,request: 1))
                    let hello = try DownloadEnvelope.read { try transport.input.read(upToCount: $0) ?? Data() }
                    guard case .hello(1) = hello.message, hello.requestID == 1 else { throw BrowserFailure.invalid("Incompatible download service protocol") }
                    limit.cancel()
                    try self.lock.withLock {
                        guard !self.stopping else { throw BrowserFailure.invalid("Downloads are closed") }
                        self.ready = true
                    }
                    resumed = true; continuation.resume()
                    while !self.lock.withLock({ self.stopping }) {
                        self.received?(.success(try DownloadEnvelope.read { try transport.input.read(upToCount: $0) ?? Data() }))
                    }
                } catch {
                    timeout?.cancel()
                    if !resumed { continuation.resume(throwing: error) }
                    else if !self.lock.withLock({ self.stopping }) { self.received?(.failure(.invalid("Download service connection ended: \(error.localizedDescription)"))) }
                }
            }
        }
    }
    func send(_ message: BrowserCommand, willSend: @escaping @Sendable (UInt64) -> Void) async throws -> UInt64 {
        try await withCheckedThrowingContinuation { continuation in
            writer.async {
                do {
                    guard let connection = self.lock.withLock({ self.ready && !self.stopping ? self.connection : nil }) else { throw BrowserFailure.invalid("Downloads are unavailable") }
                    self.nextRequest += 1; let request = self.nextRequest
                    let packet = try BrowserWire.encode(message,tab: nil,request: request)
                    willSend(request); try connection.output.write(contentsOf: packet)
                    continuation.resume(returning: request)
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
    func stop() async {
        lock.withLock { stopping = true; connection?.interrupt(); try? owner?.close(); owner = nil }
        await withCheckedContinuation { continuation in
            lifecycle.async {
                self.writer.sync {}
                self.lock.withLock { self.process }?.finish(grace: 5)
                self.reading.wait()
                self.lock.withLock { self.connection?.close(); self.connection = nil; self.process = nil; self.ready = false }
                continuation.resume()
            }
        }
    }
}
