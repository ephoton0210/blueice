// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

/// Read-only lease on the test harness-owned loopback origin. OpenSSH runs
/// outside the UI runner sandbox; the harness reaps it and removes its files.
final class SFTPFixture: Sendable {
    static let passphrase = "blueice-local-key-fixture"
    static let payload = Data("BlueIce authenticated SFTP 下載 fixture\n".utf8)
    let root: URL
    let port: Int
    let username: String
    let url: String
    let privateKey: URL
    let knownHosts: URL
    private struct Ready: Decodable {
        let port: Int
        let username: String
        let url: String
        let private_key: String
        let known_hosts: String
        let server_pid: Int32
    }
    init() throws {
        guard let path = Bundle(for: Self.self).object(forInfoDictionaryKey: "BlueIceSFTPFixtureMetadata") as? String,
              path.hasPrefix("/private/tmp/bi-sftp-ui-server-"), path.hasSuffix("/ready.json") else {
            throw NSError(domain: "SFTPFixture",code: 1,userInfo: [NSLocalizedDescriptionKey: "Run frontend/macos/test.sh to start the owned SFTP fixture"])
        }
        let metadata = URL(fileURLWithPath: path)
        root = metadata.deletingLastPathComponent()
        let data = try Data(contentsOf: metadata)
        guard data.count <= 8192 else { throw NSError(domain: "SFTPFixture",code: 2) }
        let value = try JSONDecoder().decode(Ready.self,from: data)
        guard (1...65535).contains(value.port), value.server_pid > 1,
              value.private_key.hasPrefix(root.path + "/"), value.known_hosts.hasPrefix(root.path + "/"),
              value.url.hasPrefix("sftp://" + value.username + "@127.0.0.1:") else {
            throw NSError(domain: "SFTPFixture",code: 3)
        }
        port = value.port; username = value.username; url = value.url
        privateKey = URL(fileURLWithPath: value.private_key)
        knownHosts = URL(fileURLWithPath: value.known_hosts)
    }
}
