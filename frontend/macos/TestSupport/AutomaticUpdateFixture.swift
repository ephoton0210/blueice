// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Darwin
import Foundation

/// A lease on the harness-owned copy of the real service bundle. The
/// application owns process containment; the external harness owns its files.
final class AutomaticUpdateFixture {
    let root: URL
    let directory: URL
    let launcher: URL
    private let port: Int
    private let token: String
    private struct Ready: Decodable { let port: Int; let token: String; let launcher: String }

    init() throws {
        guard let path = Bundle(for: Self.self).object(forInfoDictionaryKey: "BlueIceUpdateFixtureMetadata") as? String,
              path.hasPrefix("/private/tmp/bi-update-ui-server-"), path.hasSuffix("/ready.json") else {
            throw NSError(domain: "AutomaticUpdateFixture", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "Run frontend/macos/test.sh to start the owned update fixture"])
        }
        let metadata = URL(fileURLWithPath: path)
        root = metadata.deletingLastPathComponent()
        directory = root.appendingPathComponent("BlueIce.app/Contents/MacOS")
        let data = try Data(contentsOf: metadata)
        guard data.count <= 8192 else { throw NSError(domain: "AutomaticUpdateFixture", code: 2) }
        let ready = try JSONDecoder().decode(Ready.self, from: data)
        guard (1...65535).contains(ready.port), ready.token.count == 64,
              ready.token.allSatisfy({ $0.isHexDigit }),
              ready.launcher == directory.appendingPathComponent("blueice-launcher-wrapper").path else {
            throw NSError(domain: "AutomaticUpdateFixture", code: 3)
        }
        port = ready.port; token = ready.token
        launcher = URL(fileURLWithPath: ready.launcher)
    }

    func triggerUpdate() throws { try post("update") }
    func stop() { try? post("finish") }

    func processID(_ name: String) -> pid_t? {
        guard let contents = try? String(contentsOf: directory.appendingPathComponent(name + ".pid"), encoding: .utf8),
              let pid = pid_t(contents.trimmingCharacters(in: .whitespacesAndNewlines)), pid > 0 else { return nil }
        return pid
    }

    func startupDiagnostics() -> String {
        let log = (try? String(contentsOf: directory.appendingPathComponent("launcher.log"), encoding: .utf8)) ?? "No launcher output."
        return "root=\(root.path) launcher=\(String(describing: processID("launcher"))) "
            + "serving=\(String(describing: processID("serving")))\n" + log
    }

    private final class Reply: @unchecked Sendable { var accepted = false }

    private func post(_ operation: String) throws {
        var request = URLRequest(url: URL(string: "http://127.0.0.1:\(port)/\(operation)")!)
        request.httpMethod = "POST"; request.httpBody = Data(); request.timeoutInterval = 5
        request.setValue("Bearer " + token, forHTTPHeaderField: "Authorization")
        let done = DispatchSemaphore(value: 0), reply = Reply()
        let task = URLSession.shared.dataTask(with: request) { _, response, error in
            reply.accepted = error == nil && (response as? HTTPURLResponse)?.statusCode == 200
            done.signal()
        }
        task.resume()
        guard done.wait(timeout: .now() + 6) == .success else {
            task.cancel(); throw NSError(domain: "AutomaticUpdateFixture", code: 4)
        }
        guard reply.accepted else { throw NSError(domain: "AutomaticUpdateFixture", code: 5) }
    }
}
