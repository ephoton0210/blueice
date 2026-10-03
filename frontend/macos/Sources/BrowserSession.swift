// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

// Blocking socket/pipe reads and process waits stay off the AppKit/main actor.
final class BrowserSession: @unchecked Sendable {
    // Short paths also keep the launcher's internal sockets under Darwin's limit.
    let runtimeDirectory = URL(fileURLWithPath: "/private/tmp/bi-" + UUID().uuidString.replacingOccurrences(of: "-", with: ""))
    var frameDirectory: URL { runtimeDirectory.appendingPathComponent("frames") }
    private let lock = NSLock()
    private let reader = DispatchQueue(label: "cc.blueice.browser.read")
    private let writer = DispatchQueue(label: "cc.blueice.browser.write")
    private let lifecycle = DispatchQueue(label: "cc.blueice.browser.lifecycle")
    private let reading = DispatchGroup()
    private var process: OwnedBrowserProcess?
    private var supervisedExecutable: URL?
    private var connection: BrowserConnection?
    private var ownerLiveness: FileHandle?
    private var ownsDirectory = false
    private var started = false
    private var stopping = false
    private var handshaken = false
    private var requestID: UInt64 = 0 // Accessed only on writer.
    private var callback: (@Sendable (Result<IncomingEnvelope, BrowserFailure>) -> Void)?

    var received: (@Sendable (Result<IncomingEnvelope, BrowserFailure>) -> Void)? {
        get { lock.withLock { callback } }
        set { lock.withLock { callback = newValue } }
    }
    var processID: Int32? { lock.withLock { process?.pid } }
    var downloadsExecutable: URL? { lock.withLock { supervisedExecutable?.deletingLastPathComponent().appendingPathComponent("blueice-downloads") } }

    @MainActor
    func openPermissions() -> Bool {
        guard let owner = processID, let app = lock.withLock({ supervisedExecutable?.deletingLastPathComponent().appendingPathComponent("BlueIcePanels.app") }) else { return false }
        // Activate only the private child of this owned launcher. Another
        // browser instance's panel cannot receive this person's decision.
        guard let child = NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIcePanels").first(where: {
            getpgid($0.processIdentifier) == owner && $0.bundleURL?.resolvingSymlinksInPath() == app.resolvingSymlinksInPath()
        }) else { return false }
        return child.activate(options: [])
    }

    func startForBrowser(launcher: URL? = nil) async throws {
        let arguments = ProcessInfo.processInfo.arguments
        if let launcher { try await start(launcher: launcher); return }
        if let option = arguments.firstIndex(of: "--core-exe") {
            guard arguments.indices.contains(option + 1) else { throw BrowserFailure.invalid("--core-exe requires a core executable path.") }
            try await start(executable: URL(fileURLWithPath: arguments[option + 1])); return
        }
        if let option = arguments.firstIndex(of: "--launcher-exe") {
            guard arguments.indices.contains(option + 1) else { throw BrowserFailure.invalid("--launcher-exe requires a launcher executable path.") }
            try await start(launcher: URL(fileURLWithPath: arguments[option + 1])); return
        }
        try await start(launcher: Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/blueice-launcher"))
    }

    // Default GUI mode always uses the supervised, policy-checked stack.
    func start(launcher: URL) async throws { try await start(executable: launcher, supervised: true) }

    // Explicit diagnostic mode retains the fail-closed private-pipe contract.
    func start(executable: URL) async throws { try await start(executable: executable, supervised: false) }

    private func start(executable: URL, supervised: Bool) async throws {
        try lock.withLock {
            guard !started, !stopping else { throw BrowserFailure.invalid("Browser session is already started or closed.") }
            started = true
        }
        do {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                reading.enter()
                reader.async {
                    defer { self.reading.leave() }
                    var resumed = false
                    var timeout: DispatchWorkItem?
                    do {
                        let child: OwnedBrowserProcess = try self.lock.withLock {
                            guard !self.stopping else { throw BrowserFailure.invalid("Browser session is closed.") }
                            let names = supervised ? ["blueice-launcher", "blueice-core", "blueice-ai-gatekeeper"] : ["blueice-core"]
                            for (index, name) in names.enumerated() {
                                let path = index == 0 ? executable : executable.deletingLastPathComponent().appendingPathComponent(name)
                                guard FileManager.default.isExecutableFile(atPath: path.path) else {
                                    throw BrowserFailure.invalid("BlueIce \(name) was not found. Build the app with build.sh first.")
                                }
                            }
                            // Never adopt/remove a directory owned by a different process.
                            guard mkdir(self.runtimeDirectory.path, 0o700) == 0 else {
                                throw BrowserFailure.invalid("Cannot create the private browser runtime directory.")
                            }
                            self.ownsDirectory = true
                            var environment = ProcessInfo.processInfo.environment
                            environment["TMPDIR"] = self.runtimeDirectory.path
                            environment["XDG_RUNTIME_DIR"] = self.runtimeDirectory.path
                            if supervised {
                                self.supervisedExecutable = executable
                                environment["BLUEICE_NATIVE_DOWNLOADS_MANAGED"] = "1"
                                let owner = Pipe()
                                var launcherArguments = [
                                    "--socket", self.runtimeDirectory.appendingPathComponent("browser.sock").path,
                                    "--control-socket", self.runtimeDirectory.appendingPathComponent("control.sock").path,
                                    "--owned-gatekeeper-socket", self.runtimeDirectory.appendingPathComponent("blueice/gatekeeper.sock").path,
                                    "--frame-dir", self.frameDirectory.path,
                                    "--width", "1024", "--height", "640", "--exit-on-stdin-eof"
                                ]
                                let panels = executable.deletingLastPathComponent().appendingPathComponent("BlueIcePanels.app/Contents/MacOS/BlueIcePanels")
                                if FileManager.default.isExecutableFile(atPath: panels.path) {
                                    launcherArguments.append("--trusted-frontend")
                                }
                                // Owner startup input only. No browser/AI IPC
                                // message can select or replace this package.
                                let arguments = ProcessInfo.processInfo.arguments
                                if let index = arguments.firstIndex(of: "--extension-manifest"), arguments.indices.contains(index + 1) {
                                    launcherArguments += ["--extension-manifest", arguments[index + 1]]
                                }
                                let child = try OwnedBrowserProcess(executable: executable, arguments: launcherArguments,
                                    environment: environment, input: owner.fileHandleForReading)
                                try? owner.fileHandleForReading.close()
                                self.ownerLiveness = owner.fileHandleForWriting
                                self.process = child
                                return child
                            }
                            let toCore = Pipe(), fromCore = Pipe()
                            let child = try OwnedBrowserProcess(executable: executable,
                                arguments: ["--stdio", "--frame-dir", self.frameDirectory.path], environment: environment,
                                input: toCore.fileHandleForReading, output: fromCore.fileHandleForWriting)
                            try? toCore.fileHandleForReading.close()
                            try? fromCore.fileHandleForWriting.close()
                            self.connection = BrowserConnection(input: fromCore.fileHandleForReading, output: toCore.fileHandleForWriting)
                            self.process = child
                            return child
                        }
                        let deadline = DispatchWorkItem {
                            self.lock.withLock {
                                if !self.handshaken {
                                    self.connection?.interrupt()
                                    child.signalGroup(SIGKILL)
                                }
                            }
                        }
                        timeout = deadline
                        DispatchQueue.global().asyncAfter(deadline: .now() + 12, execute: deadline)
                        if supervised {
                            let connectDeadline = Date().addingTimeInterval(10)
                            var connected: BrowserConnection?
                            while connected == nil && Date() < connectDeadline && child.isRunning && !self.lock.withLock({ self.stopping }) {
                                connected = try? BrowserConnection(socketPath: self.runtimeDirectory.appendingPathComponent("browser.sock").path)
                                if connected == nil { Thread.sleep(forTimeInterval: 0.02) }
                            }
                            guard let connected else { throw BrowserFailure.invalid("BlueIce launcher could not start its core and gatekeeper services.") }
                            self.lock.withLock { self.connection = connected }
                        }
                        guard let transport = self.lock.withLock({ self.connection }) else { throw BrowserFailure.invalid("Browser connection is unavailable.") }
                        _ = try self.writer.sync { try self.write(.values("Hello", ["protocol_version": .unsigned(UInt64(BrowserWire.version))]), tab: nil) }
                        func read() throws -> IncomingEnvelope {
                            try BrowserWire.read { try transport.input.read(upToCount: $0) ?? Data() }
                        }
                        let hello = try read()
                        guard case .hello(BrowserWire.version) = hello.message, hello.requestID == 1 else {
                            throw BrowserFailure.invalid("BlueIce services use an incompatible browser protocol.")
                        }
                        try self.lock.withLock {
                            guard !self.stopping else { throw BrowserFailure.invalid("Browser session is closed.") }
                            self.handshaken = true
                        }
                        deadline.cancel()
                        resumed = true
                        continuation.resume()
                        while !self.lock.withLock({ self.stopping }) { self.received?(.success(try read())) }
                    } catch {
                        timeout?.cancel()
                        if !resumed { continuation.resume(throwing: error) }
                        else if !self.lock.withLock({ self.stopping }) {
                            self.received?(.failure(.invalid("Browser service connection ended: \(error.localizedDescription)")))
                        }
                    }
                }
            }
        } catch { await stop(); throw error }
    }

    @discardableResult
    func send(_ command: BrowserCommand, tab: UInt64? = nil, willSend: (@Sendable (UInt64) -> Void)? = nil) async throws -> UInt64 {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<UInt64, Error>) in
            writer.async {
                do {
                    guard self.lock.withLock({ !self.stopping && self.handshaken && self.process?.isRunning == true }) else {
                        throw BrowserFailure.invalid("BlueIce browser services are not running.")
                    }
                    let request = try self.write(command, tab: tab, willSend: willSend)
                    continuation.resume(returning: request)
                } catch { continuation.resume(throwing: error) }
            }
        }
    }

    @discardableResult
    private func write(_ command: BrowserCommand, tab: UInt64?, willSend: (@Sendable (UInt64) -> Void)? = nil) throws -> UInt64 {
        guard let transport = lock.withLock({ connection }) else { throw BrowserFailure.invalid("Browser connection is unavailable.") }
        requestID += 1
        let bytes = try BrowserWire.encode(command, tab: tab, request: requestID)
        // Register reply ownership before the reader can receive a response.
        // Resuming the send continuation alone does not establish that order.
        willSend?(requestID)
        try transport.output.write(contentsOf: bytes)
        return requestID
    }

    func stop() async {
        lock.withLock { stopping = true }
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            lifecycle.async {
                self.writer.sync {
                    if self.lock.withLock({ self.handshaken }) { _ = try? self.write(.unit("Shutdown"), tab: nil) }
                }
                self.lock.withLock { self.connection?.interrupt() }
                self.lock.withLock { try? self.ownerLiveness?.close(); self.ownerLiveness = nil }
                self.lock.withLock { self.process }?.finish()
                self.reading.wait()
                self.lock.withLock {
                    self.connection?.close()
                    self.connection = nil; self.process = nil; self.callback = nil
                    if self.ownsDirectory {
                        try? FileManager.default.removeItem(at: self.runtimeDirectory)
                        self.ownsDirectory = false
                    }
                }
                continuation.resume()
            }
        }
    }
}
