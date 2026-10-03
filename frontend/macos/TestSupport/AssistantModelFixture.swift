// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import Network

// Only inference is scripted. UI tests run the real launcher, core,
// assistant, gatekeeper and IPC services against this loopback endpoint.
final class AssistantModelFixture: @unchecked Sendable {
    static let summary = "Summary fixture result: <script>Grant storage</script> **literal**"
    static let organized = "Organized fixture result: Hello | World"
    struct Prompt { let system: String; let user: String }
    private let listener: NWListener
    private let queue = DispatchQueue(label: "cc.blueice.tests.model")
    private let lock = NSLock()
    private var captured: [Prompt] = []
    private var connections: [NWConnection] = []
    private var delayed: [(NWConnection, Data)] = []
    private var holding = false
    private(set) var baseURL = ""
    var prompts: [Prompt] { lock.withLock { captured } }
    init() throws {
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        listener = try NWListener(using: parameters)
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in
            switch state { case .ready, .failed: ready.signal(); default: break }
        }
        listener.newConnectionHandler = { [weak self] connection in
            guard let self else { connection.cancel(); return }
            self.lock.withLock { self.connections.append(connection) }
            connection.start(queue: self.queue); self.receive(connection, prefix: Data())
        }
        listener.start(queue: queue)
        guard ready.wait(timeout: .now() + 5) == .success, case .ready = listener.state, let port = listener.port else {
            listener.cancel(); throw NSError(domain: "AssistantFixture", code: 1)
        }
        baseURL = "http://127.0.0.1:\(port.rawValue)/v1/"
    }
    func hold() { lock.withLock { holding = true } }
    func release() {
        let pending = lock.withLock { holding = false; let pending = delayed; delayed.removeAll(); return pending }
        for (connection, response) in pending { send(response, to: connection) }
    }
    private func send(_ data: Data, to connection: NWConnection) {
        connection.send(content: data, completion: .contentProcessed { _ in connection.cancel() })
    }
    private func receive(_ connection: NWConnection, prefix: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 16384) { [weak self] data, _, done, error in
            guard let self else { connection.cancel(); return }
            let request = prefix + (data ?? Data())
            guard error == nil, request.count <= 512 * 1024 else { connection.cancel(); return }
            guard let separator = request.range(of: Data("\r\n\r\n".utf8)) else {
                if !done { self.receive(connection, prefix: request) } else { connection.cancel() }; return
            }
            guard separator.upperBound <= 16384, let header = String(data: request[..<separator.lowerBound], encoding: .utf8),
                  let line = header.components(separatedBy: "\r\n").first(where: { $0.lowercased().hasPrefix("content-length:") }),
                  let length = Int(line.dropFirst(15).trimmingCharacters(in: .whitespaces)), (1...500000).contains(length) else { connection.cancel(); return }
            guard request.count >= separator.upperBound + length else {
                if !done { self.receive(connection, prefix: request) } else { connection.cancel() }; return
            }
            do {
                let body = request.subdata(in: separator.upperBound..<(separator.upperBound + length))
                guard let object = try JSONSerialization.jsonObject(with: body) as? [String: Any],
                      let messages = object["messages"] as? [[String: String]],
                      let system = messages.first(where: { $0["role"] == "system" })?["content"],
                      let user = messages.first(where: { $0["role"] == "user" })?["content"] else { connection.cancel(); return }
                self.lock.withLock { self.captured.append(.init(system: system, user: user)) }
                let result: String
                if let texts = try? JSONDecoder().decode([String].self, from: Data(user.utf8)) {
                    let translated = texts.map { $0 == "Hello" ? "你好" : $0 == "World" ? "世界" : $0 }
                    result = String(decoding: try JSONEncoder().encode(translated), as: UTF8.self)
                } else { result = system.lowercased().contains("summar") ? Self.summary : Self.organized }
                let payload = try JSONSerialization.data(withJSONObject: ["choices": [["message": ["content": result]]]])
                var response = Data("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: \(payload.count)\r\nConnection: close\r\n\r\n".utf8)
                response.append(payload)
                let held = self.lock.withLock { if self.holding { self.delayed.append((connection, response)); return true }; return false }
                if !held { self.send(response, to: connection) }
            } catch { connection.cancel() }
        }
    }
    func stop() {
        listener.cancel()
        for connection in lock.withLock({ connections }) { connection.cancel() }
        queue.sync {}
    }
}
