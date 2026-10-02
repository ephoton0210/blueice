// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

// Blocking pipe reads and process waits stay off the AppKit/main actor.
final class BrowserSession: @unchecked Sendable {
    let frameDirectory = FileManager.default.temporaryDirectory
        .appendingPathComponent("blueice-macos-" + UUID().uuidString)
    private let lock = NSLock()
    private let reader = DispatchQueue(label: "cc.blueice.browser.read")
    private let writer = DispatchQueue(label: "cc.blueice.browser.write")
    private let lifecycle = DispatchQueue(label: "cc.blueice.browser.lifecycle")
    private let reading = DispatchGroup()
    private var core: Process?
    private var input: FileHandle?
    private var started = false
    private var stopping = false
    private var handshaken = false
    private var requestID: UInt64 = 0 // Accessed only on writer.
    private var callback: (@Sendable (Result<IncomingEnvelope, BrowserFailure>) -> Void)?

    var received: (@Sendable (Result<IncomingEnvelope, BrowserFailure>) -> Void)? {
        get { lock.withLock { callback } }
        set { lock.withLock { callback = newValue } }
    }
    var processID: Int32? { lock.withLock { core?.processIdentifier } }

    func start(executable: URL) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            reading.enter()
            reader.async {
                defer { self.reading.leave() }
                var resumed = false
                var timeout: DispatchWorkItem?
                do {
                    let process = Process()
                    let toCore = Pipe(), fromCore = Pipe()
                    process.executableURL = executable
                    process.arguments = ["--stdio", "--frame-dir", self.frameDirectory.path]
                    process.standardInput = toCore
                    process.standardOutput = fromCore
                    process.standardError = FileHandle.standardError
                    try self.lock.withLock {
                        guard !self.started, !self.stopping else { throw BrowserFailure.invalid("Browser session is already started or closed.") }
                        self.started = true
                        guard FileManager.default.isExecutableFile(atPath: executable.path) else {
                            throw BrowserFailure.invalid("BlueIce core was not found. Build the app with build.sh first.")
                        }
                        try process.run()
                        self.core = process
                        self.input = toCore.fileHandleForWriting
                    }
                    defer {
                        try? toCore.fileHandleForWriting.close()
                        try? fromCore.fileHandleForReading.close()
                    }
                    let deadline = DispatchWorkItem {
                        if self.lock.withLock({ !self.handshaken }), process.isRunning { process.terminate() }
                    }
                    timeout = deadline
                    DispatchQueue.global().asyncAfter(deadline: .now() + 10, execute: deadline)
                    try self.writer.sync { try self.write(.values("Hello", ["protocol_version": .unsigned(UInt64(BrowserWire.version))]), tab: nil) }
                    func read() throws -> IncomingEnvelope {
                        try BrowserWire.read { try fromCore.fileHandleForReading.read(upToCount: $0) ?? Data() }
                    }
                    let hello = try read()
                    guard case .hello(BrowserWire.version) = hello.message, hello.requestID == 1 else {
                        throw BrowserFailure.invalid("BlueIce core uses an incompatible browser protocol.")
                    }
                    self.lock.withLock { self.handshaken = true }
                    deadline.cancel()
                    resumed = true
                    continuation.resume()
                    while !self.lock.withLock({ self.stopping }) { self.received?(.success(try read())) }
                } catch {
                    timeout?.cancel()
                    if !resumed { continuation.resume(throwing: error) }
                    else if !self.lock.withLock({ self.stopping }) {
                        self.received?(.failure(.invalid("Core connection ended: \(error.localizedDescription)")))
                    }
                }
            }
        }
    }

    func send(_ command: BrowserCommand, tab: UInt64? = nil) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            writer.async {
                do {
                    guard self.lock.withLock({ !self.stopping && self.handshaken && self.core?.isRunning == true }) else {
                        throw BrowserFailure.invalid("BlueIce core is not running.")
                    }
                    try self.write(command, tab: tab)
                    continuation.resume()
                } catch { continuation.resume(throwing: error) }
            }
        }
    }

    private func write(_ command: BrowserCommand, tab: UInt64?) throws {
        guard let pipe = lock.withLock({ input }) else { throw BrowserFailure.invalid("Core connection is unavailable.") }
        requestID += 1
        try pipe.write(contentsOf: BrowserWire.encode(command, tab: tab, request: requestID))
    }

    func stop() async {
        lock.withLock { stopping = true }
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            lifecycle.async {
                if let process = self.lock.withLock({ self.core }), process.isRunning {
                    let terminate = DispatchWorkItem { if process.isRunning { process.terminate() } }
                    let force = DispatchWorkItem { if process.isRunning { kill(process.processIdentifier, SIGKILL) } }
                    DispatchQueue.global().asyncAfter(deadline: .now() + 2, execute: terminate)
                    DispatchQueue.global().asyncAfter(deadline: .now() + 3, execute: force)
                    self.writer.sync { try? self.write(.unit("Shutdown"), tab: nil) }
                    process.waitUntilExit()
                    terminate.cancel()
                    force.cancel()
                }
                self.reading.wait()
                self.lock.withLock { self.input = nil; self.core = nil; self.callback = nil }
                // This path is a fresh UUID owned exclusively by this child.
                try? FileManager.default.removeItem(at: self.frameDirectory)
                continuation.resume()
            }
        }
    }
}
