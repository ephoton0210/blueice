// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import Network

final class DownloadFixture: @unchecked Sendable {
    struct Request: Sendable { let method: String; let path: String; let body: Data }
    let responseBytes = Data([0xff, 0, 0x80, 0xfe, 13, 10, 42])
    private var requestRecords: [Request] = []
    var responses: [Request] { lock.withLock { requestRecords } }
    let bytes = Data(String(repeating: "BlueIce download 中文\n", count: 256).utf8)
    let large = Data(repeating: 65, count: 8 * 1024 * 1024)
    private let queue = DispatchQueue(label: "cc.blueice.tests.download-http", qos: .userInitiated)
    private let listener: NWListener
    private let lock = NSLock()
    private var connections: [NWConnection] = []
    private var slow = true
    private var slowDelay: TimeInterval = 0.1
    private var recorded: [String] = []
    private(set) var origin = ""
    var requests: [String] { lock.withLock { recorded } }
    func setSlow(_ value: Bool, delay: TimeInterval = 0.1) { lock.withLock { slow = value; slowDelay = delay } }

    init() throws {
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        listener = try NWListener(using: parameters)
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in switch state { case .ready, .failed: ready.signal(); default: break } }
        listener.newConnectionHandler = { [weak self] connection in
            guard let self else { connection.cancel(); return }
            self.lock.withLock { self.connections.append(connection) }
            connection.start(queue: self.queue); self.read(connection, prefix: Data())
        }
        listener.start(queue: queue)
        guard ready.wait(timeout: .now() + 5) == .success, case .ready = listener.state, let port = listener.port else {
            listener.cancel(); throw NSError(domain: "DownloadFixture",code: 1)
        }
        origin = "http://127.0.0.1:\(port.rawValue)"
    }
    private func read(_ connection: NWConnection, prefix: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 4096) { [weak self] bytes, _, done, error in
            guard let self else { connection.cancel(); return }
            let input = prefix + (bytes ?? Data())
            guard error == nil, input.count <= 1_048_576 + 16384 else { connection.cancel(); return }
            guard let boundary = input.range(of: Data("\r\n\r\n".utf8)) else {
                guard input.count < 16384 else { connection.cancel(); return }
                if !done { self.read(connection, prefix: input) } else { connection.cancel() }; return
            }
            guard boundary.upperBound <= 16384,
                  let request = String(data: input[..<boundary.upperBound], encoding: .utf8) else { connection.cancel(); return }
            let lines = request.components(separatedBy: "\r\n")
            let fields = lines[0].split(separator: " ")
            guard fields.count >= 2 else { connection.cancel(); return }
            let method = String(fields[0]), path = String(fields[1])
            let rawLength = lines.first { $0.lowercased().hasPrefix("content-length:") }?.split(separator: ":",maxSplits: 1).last?.trimmingCharacters(in: .whitespaces)
            let length = rawLength.flatMap(Int.init) ?? 0
            guard length >= 0, length <= 1_048_576 else { connection.cancel(); return }
            guard input.count >= boundary.upperBound + length else {
                if !done { self.read(connection,prefix: input) } else { connection.cancel() }; return
            }
            let body = input.subdata(in: boundary.upperBound..<(boundary.upperBound + length))
            let count = self.lock.withLock { () -> Int in
                self.recorded.append(path)
                self.requestRecords.append(Request(method: method,path: path,body: body))
                return self.requestRecords.filter { $0.path == path }.count
            }
            let original = path.hasPrefix("/response") && path != "/response-links"
            if path == "/response-post", method != "POST" || count != 1 {
                connection.send(content: Data("HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".utf8),completion: .contentProcessed { _ in connection.cancel() }); return
            }
            let payload: Data
            if path == "/links" { payload = Data("<!doctype html><html><body><a href='/notes.txt'>Download notes</a></body></html>".utf8) }
            else if path == "/response-links" { payload = Data("<!doctype html><html><body><h1>Download source</h1><input aria-label='Retained editor' value='Keep this document'><a href='/response.bin' aria-label='Download response'>Binary download</a><form method=post action='/response-post'><input name=token value=one-shot><button aria-label='Download POST'>Download POST</button></form></body></html>".utf8) }
            else if original && !path.contains("large") { payload = self.responseBytes }
            else { payload = path.contains("large") ? self.large : self.bytes }
            var start = 0, end = payload.count - 1
            let range = lines.first { $0.lowercased().hasPrefix("range:") }
            if let raw = range?.components(separatedBy: "bytes=").last, let dash = raw.firstIndex(of: "-") {
                start = Int(raw[..<dash]) ?? 0; end = Int(raw[raw.index(after: dash)...]) ?? end
            }
            guard start >= 0, end < payload.count, end >= start else { connection.cancel(); return }
            let status = range == nil ? (path == "/response-post" ? "201 Created" : "200 OK") : "206 Partial Content"
            let type = path == "/links" || path == "/response-links" ? "text/html" : (original ? "application/octet-stream" : (path.hasSuffix(".exe") ? "application/x-msdownload" : "text/plain"))
            let contentRange = range == nil ? "" : "Content-Range: bytes \(start)-\(end)/\(payload.count)\r\n"
            let disposition = original ? "Content-Disposition: attachment; filename=original.bin; filename*=UTF-8''%E4%B8%8B%E8%BC%89.bin\r\n" : ""
            let headers = "HTTP/1.1 \(status)\r\nContent-Type: \(type)\r\nContent-Length: \(end - start + 1)\r\nAccept-Ranges: bytes\r\nETag: \"blueice-native-download-v1\"\r\n\(contentRange)\(disposition)Connection: close\r\n\r\n"
            connection.send(content: Data(headers.utf8), completion: .contentProcessed { error in
                guard error == nil else { connection.cancel(); return }
                self.send(connection, data: payload, offset: start, end: end + 1, delayed: path.contains("large"))
            })
        }
    }
    private func send(_ connection: NWConnection, data: Data, offset: Int, end: Int, delayed: Bool) {
        let next = min(offset + 4096, end)
        connection.send(content: data.subdata(in: offset..<next), completion: .contentProcessed { [weak self] error in
            guard let self, error == nil else { connection.cancel(); return }
            if next == end { connection.cancel(); return }
            let delay = delayed ? self.lock.withLock({ self.slow ? self.slowDelay : 0 }) : 0
            self.queue.asyncAfter(deadline: .now() + delay) { self.send(connection, data: data, offset: next, end: end, delayed: delayed) }
        })
    }
    func stop() { listener.cancel(); lock.withLock { connections.forEach { $0.cancel() } }; queue.sync {} }
}
