// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import CoreFoundation
import Combine

struct SFTPConfigurationFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// Process-local SSH file choices. Neither private-key bytes nor passphrases
/// are preferences or download protocol fields.
struct DownloadSFTPConfiguration: Equatable, Sendable {
    let knownHosts: URL?
    let privateKey: URL?
    init(knownHosts: URL? = nil, privateKey: URL? = nil) {
        self.knownHosts = knownHosts; self.privateKey = privateKey
    }
    private static func checkedPath(_ path: String) throws -> URL {
        guard path.hasPrefix("/"), path.utf8.count <= 4096,
              !path.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains) else {
            throw SFTPConfigurationFailure(message: "The SSH file setting must be an absolute local path of at most 4096 bytes.")
        }
        return URL(fileURLWithPath: path).standardizedFileURL
    }
    static func decode(_ object: Any?) throws -> Self {
        guard let object else { return Self() }
        guard let fields = object as? [String: Any],
              let version = fields["version"] as? NSNumber,
              CFGetTypeID(version) != CFBooleanGetTypeID(), version.doubleValue == 1 else {
            throw SFTPConfigurationFailure(message: "The saved SSH file settings are unsupported. Choose files or restore the defaults.")
        }
        func url(_ key: String) throws -> URL? {
            guard let value = fields[key] else { return nil }
            guard let path = value as? String else {
                throw SFTPConfigurationFailure(message: "The saved SSH file settings are unsupported. Choose files or restore the defaults.")
            }
            return try checkedPath(path)
        }
        return try Self(knownHosts: url("known_hosts"),privateKey: url("private_key"))
    }
    var preferenceValue: [String: Any] {
        var fields: [String: Any] = ["version": 1]
        if let knownHosts { fields["known_hosts"] = knownHosts.path }
        if let privateKey { fields["private_key"] = privateKey.path }
        return fields
    }
    static func checkedFile(_ url: URL) throws -> URL {
        guard url.isFileURL, url.host == nil || url.host == "" || url.host == "localhost" else {
            throw SFTPConfigurationFailure(message: "Choose a readable regular local SSH file.")
        }
        let file = try checkedPath(url.path)
        let values = try file.resourceValues(forKeys: [.isRegularFileKey,.isSymbolicLinkKey,.isAliasFileKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true, values.isAliasFile != true,
              FileManager.default.isReadableFile(atPath: file.path) else {
            throw SFTPConfigurationFailure(message: "Choose a readable regular local SSH file.")
        }
        return file.resolvingSymlinksInPath().standardizedFileURL
    }
    func validated() throws -> Self {
        try Self(knownHosts: knownHosts.map(Self.checkedFile),privateKey: privateKey.map(Self.checkedFile))
    }
    func launchArguments() throws -> [String] {
        let checked = try validated()
        var arguments: [String] = []
        if let hosts = checked.knownHosts { arguments += ["--sftp-known-hosts",hosts.path] }
        if let key = checked.privateKey { arguments += ["--sftp-private-key",key.path] }
        return arguments
    }
}

@MainActor
final class BrowserSFTPPreferences: ObservableObject {
    static let configurationKey = "browser.downloads.sftp.configuration"
    private let defaults: UserDefaults
    @Published private(set) var configuration: DownloadSFTPConfiguration?
    @Published private(set) var configurationError: String?
    init(defaults: UserDefaults) {
        self.defaults = defaults
        do { configuration = try DownloadSFTPConfiguration.decode(defaults.object(forKey: Self.configurationKey)) }
        catch { configurationError = error.localizedDescription }
    }
    func save(_ proposed: DownloadSFTPConfiguration) throws {
        let checked = try proposed.validated()
        defaults.set(checked.preferenceValue,forKey: Self.configurationKey)
        configuration = checked; configurationError = nil
    }
    func restoreDefaults() {
        defaults.removeObject(forKey: Self.configurationKey)
        configuration = DownloadSFTPConfiguration(); configurationError = nil
    }
}
