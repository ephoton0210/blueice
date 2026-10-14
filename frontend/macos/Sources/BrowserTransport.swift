// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Darwin
import Foundation

// Create the process group before the launcher can spawn any services. Keep
// its PID reserved until shutdown completes so signals cannot hit a reused PID.
final class OwnedBrowserProcess: @unchecked Sendable {
    let pid: pid_t
    private let lock = NSLock()
    private var reaped = false

    init(executable: URL, arguments: [String], environment: [String: String],
         input: FileHandle? = nil, output: FileHandle? = nil) throws {
        let null = try FileHandle(forUpdating: URL(fileURLWithPath: "/dev/null"))
        defer { try? null.close() }
        var actions: posix_spawn_file_actions_t?
        var attributes: posix_spawnattr_t?
        posix_spawn_file_actions_init(&actions)
        posix_spawnattr_init(&attributes)
        defer {
            posix_spawn_file_actions_destroy(&actions)
            posix_spawnattr_destroy(&attributes)
        }
        // Only standard input/output/error survive; unrelated GUI descriptors
        // must not leak into the launcher or its children.
        let setup = [
            posix_spawn_file_actions_adddup2(&actions, input?.fileDescriptor ?? null.fileDescriptor, STDIN_FILENO),
            posix_spawn_file_actions_adddup2(&actions, output?.fileDescriptor ?? null.fileDescriptor, STDOUT_FILENO),
            posix_spawn_file_actions_adddup2(&actions, STDERR_FILENO, STDERR_FILENO),
            posix_spawnattr_setpgroup(&attributes, 0),
            posix_spawnattr_setflags(&attributes, Int16(POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_CLOEXEC_DEFAULT))
        ]
        guard setup.allSatisfy({ $0 == 0 }) else { throw BrowserFailure.invalid("Cannot configure the browser service process.") }
        func cStrings(_ strings: [String]) -> [UnsafeMutablePointer<CChar>?] { strings.map { strdup($0) } + [nil] }
        var argv = cStrings([executable.path] + arguments)
        var envp = cStrings(environment.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" })
        defer { argv.forEach { free($0) }; envp.forEach { free($0) } }
        var child: pid_t = 0
        let error = posix_spawn(&child, executable.path, &actions, &attributes, &argv, &envp)
        guard error == 0 else { throw BrowserFailure.invalid("Cannot start BlueIce services: \(String(cString: strerror(error)))") }
        pid = child
    }

    var isRunning: Bool {
        lock.withLock {
            guard !reaped else { return false }
            var information = siginfo_t()
            return waitid(P_PID, id_t(pid), &information, WEXITED | WNOHANG | WNOWAIT) == 0 && information.si_pid == 0
        }
    }

    func signalGroup(_ signal: Int32) {
        lock.withLock { if !reaped { _ = kill(-pid, signal) } }
    }

    func finish(grace: TimeInterval = 2) {
        let deadline = Date().addingTimeInterval(grace)
        while isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.02) }
        if isRunning {
            signalGroup(SIGTERM)
            let terminateDeadline = Date().addingTimeInterval(1)
            while isRunning && Date() < terminateDeadline { Thread.sleep(forTimeInterval: 0.02) }
        }
        lock.withLock {
            guard !reaped else { return }
            // Clear descendants even after an abnormal early launcher exit.
            // The unreaped leader still reserves this private group ID.
            _ = kill(-pid, SIGKILL)
            var status: Int32 = 0
            while waitpid(pid, &status, 0) == -1 && errno == EINTR {}
            reaped = true
        }
    }
}

final class BrowserConnection: @unchecked Sendable {
    let input: FileHandle
    let output: FileHandle
    private let socket: Int32?

    init(input: FileHandle, output: FileHandle) {
        self.input = input; self.output = output; socket = nil
        _ = fcntl(output.fileDescriptor, F_SETNOSIGPIPE, 1)
    }

    init(socketPath: String) throws {
        var address = sockaddr_un()
        let path = Array(socketPath.utf8) + [0]
        guard path.count <= MemoryLayout.size(ofValue: address.sun_path) else {
            throw BrowserFailure.invalid("Browser socket path is too long.")
        }
        address.sun_family = sa_family_t(AF_UNIX)
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: path) }
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        var noSignal: Int32 = 1
        var timeout = timeval(tv_sec: 2, tv_usec: 0)
        let configured = setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout.size(ofValue: noSignal))) == 0
            && setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout))) == 0
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard configured && result == 0 else {
            let error = errno
            Darwin.close(fd)
            throw POSIXError(POSIXErrorCode(rawValue: error) ?? .EIO)
        }
        socket = fd
        input = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        output = input
    }

    // Unblock a read without reusing its descriptor while another queue owns it.
    // Close only after the reader exits.
    func interrupt() { if let socket { _ = Darwin.shutdown(socket, SHUT_RDWR) } }
    func close() {
        try? input.close()
        if socket == nil { try? output.close() }
    }
}
