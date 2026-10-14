// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Darwin
import Foundation

// The anonymous stdin/stdout pipes are inherited only by the exact child
// launcher starts. This service never sends a grant to a socket, and the
// browser socket below is used only for Hello and ListTabs.
@MainActor
final class NativePermissionService: PermissionService {
    private let channel: PermissionChannel
    init(socket: String) throws { channel = try PermissionChannel(socket: socket) }
    func request(_ request: PermissionRequest) async throws -> PermissionReply { try await channel.request(request) }
    func listTabs() async throws -> [PermissionTab] { try await channel.listTabs() }
}
private final class PermissionChannel: @unchecked Sendable {
    private let input = STDIN_FILENO
    private let output = STDOUT_FILENO
    private let socketPath: String
    private let queue = DispatchQueue(label: "cc.blueice.permissions.pipe")
    private var failed = false // queue-owned
    init(socket: String) throws {
        socketPath = socket
        for descriptor in [input, output] {
            var info = stat()
            guard fstat(descriptor, &info) == 0, info.st_mode & S_IFMT == S_IFIFO,
                  fcntl(descriptor, F_SETFL, fcntl(descriptor, F_GETFL) | O_NONBLOCK) == 0 else {
                throw PanelFailure.invalid("Permissions require a launcher-owned private pipe.")
            }
        }
        _ = fcntl(output, F_SETNOSIGPIPE, 1)
    }
    func request(_ request: PermissionRequest) async throws -> PermissionReply {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                do {
                    guard !self.failed else { throw PanelFailure.invalid("The private permission connection ended.") }
                    let deadline = ProcessInfo.processInfo.systemUptime + 5
                    try Self.write(PermissionFraming.frame(request), to: self.output, deadline: deadline)
                    let body = try Self.readFrame(from: self.input, maximum: PermissionFraming.maximumBytes, deadline: deadline)
                    continuation.resume(returning: try JSONDecoder().decode(PermissionReply.self, from: body))
                } catch {
                    // A timed-out or malformed private reply cannot be reused
                    // as the acknowledgement for a later human decision.
                    self.failed = true
                    Darwin.close(self.input); Darwin.close(self.output)
                    continuation.resume(throwing: error)
                }
            }
        }
    }
    func listTabs() async throws -> [PermissionTab] {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                do { continuation.resume(returning: try self.readTabs()) }
                catch { continuation.resume(throwing: error) }
            }
        }
    }
    private static func wait(_ descriptor: Int32, event: Int16, deadline: TimeInterval) throws {
        while true {
            let remaining = deadline - ProcessInfo.processInfo.systemUptime
            guard remaining > 0 else { throw PanelFailure.invalid("The permission service did not reply in time.") }
            var state = pollfd(fd: descriptor, events: event, revents: 0)
            let result = Darwin.poll(&state, 1, Int32(min(remaining * 1000 + 1, 5000)))
            if result < 0 && errno == EINTR { continue }
            guard result > 0, state.revents & (event | Int16(POLLHUP)) != 0 else {
                throw PanelFailure.invalid("The permission service connection ended.")
            }
            return
        }
    }
    private static func read(_ count: Int, from descriptor: Int32, deadline: TimeInterval) throws -> Data {
        var bytes = [UInt8](repeating: 0, count: count); var offset = 0
        while offset < count {
            try wait(descriptor, event: Int16(POLLIN), deadline: deadline)
            let got = bytes.withUnsafeMutableBytes { Darwin.read(descriptor, $0.baseAddress!.advanced(by: offset), count - offset) }
            if got < 0 && [EINTR, EAGAIN].contains(errno) { continue }
            guard got > 0 else { throw PanelFailure.invalid("The permission service connection ended.") }
            offset += got
        }
        return Data(bytes)
    }
    private static func write(_ bytes: Data, to descriptor: Int32, deadline: TimeInterval) throws {
        var offset = 0
        while offset < bytes.count {
            try wait(descriptor, event: Int16(POLLOUT), deadline: deadline)
            let sent = bytes.withUnsafeBytes { Darwin.write(descriptor, $0.baseAddress!.advanced(by: offset), bytes.count - offset) }
            if sent < 0 && [EINTR, EAGAIN].contains(errno) { continue }
            guard sent > 0 else { throw PanelFailure.invalid("The permission service connection ended.") }
            offset += sent
        }
    }
    private static func readFrame(from descriptor: Int32, maximum: Int, deadline: TimeInterval) throws -> Data {
        let count = try PermissionFraming.length(read(4, from: descriptor, deadline: deadline), maximum: maximum)
        return try read(count, from: descriptor, deadline: deadline)
    }
    private func readTabs() throws -> [PermissionTab] {
        var address = sockaddr_un(); let path = Array(socketPath.utf8) + [0]
        guard path.count <= MemoryLayout.size(ofValue: address.sun_path) else { throw PanelFailure.invalid("Invalid browser socket.") }
        address.sun_family = sa_family_t(AF_UNIX); address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: path) }
        let descriptor = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw PanelFailure.invalid("Cannot inspect browser tabs.") }
        defer { Darwin.close(descriptor) }
        let connected = withUnsafePointer(to: &address) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            Darwin.connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
        } }
        guard connected == 0, fcntl(descriptor, F_SETFL, O_NONBLOCK) == 0 else { throw PanelFailure.invalid("Cannot inspect browser tabs.") }
        _ = fcntl(descriptor, F_SETNOSIGPIPE, 1)
        let deadline = ProcessInfo.processInfo.systemUptime + 5
        let request = UInt64.random(in: 1_000_000_000...UInt64.max - 2)
        func send(_ message: Any, id: UInt64) throws {
            let body = try JSONSerialization.data(withJSONObject: ["request_id": id, "message": message])
            var count = UInt32(body.count).littleEndian
            var frame = withUnsafeBytes(of: &count) { Data($0) }; frame.append(body)
            try Self.write(frame, to: descriptor, deadline: deadline)
        }
        try send(["Hello": ["protocol_version": 2]], id: request)
        try send("ListTabs", id: request + 1)
        // Broadcasts have no authority here. Only the exact correlated tab
        // list is read; no unsolicited payload opens or confirms native UI.
        var total = 0
        for _ in 0..<256 {
            let body = try Self.readFrame(from: descriptor, maximum: 16 * 1024 * 1024, deadline: deadline)
            total += body.count
            guard total <= 32 * 1024 * 1024 else { throw PanelFailure.invalid("The browser tab reply exceeded its limit.") }
            guard let root = try JSONSerialization.jsonObject(with: body) as? [String: Any],
                  (root["request_id"] as? NSNumber)?.uint64Value == request + 1,
                  let message = root["message"] as? [String: Any], let list = message["Tabs"] else { continue }
            let tabs = try JSONDecoder().decode([PermissionTab].self, from: JSONSerialization.data(withJSONObject: list))
            guard tabs.count <= 256, tabs.allSatisfy({ $0.id > 0 && ($0.url?.utf8.count ?? 0) <= 16384 }),
                  Set(tabs.map(\.id)).count == tabs.count else { throw PanelFailure.invalid("Invalid browser tab state.") }
            return tabs
        }
        throw PanelFailure.invalid("The browser tab state is unavailable.")
    }
}
