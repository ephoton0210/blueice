// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import XCTest
#if SWIFT_PACKAGE
@testable import SFTPConfigurationSupport
#endif

@MainActor
final class SFTPConfigurationTests: XCTestCase {
    private func assertSameFile(_ actual: URL?, _ expected: URL) throws {
        let actual = try XCTUnwrap(actual)
        let left = try FileManager.default.attributesOfItem(atPath: actual.path)
        let right = try FileManager.default.attributesOfItem(atPath: expected.path)
        XCTAssertEqual(left[.systemNumber] as? NSNumber,right[.systemNumber] as? NSNumber)
        XCTAssertEqual(left[.systemFileNumber] as? NSNumber,right[.systemFileNumber] as? NSNumber)
    }
    private func root() throws -> URL {
        let root = URL(fileURLWithPath: "/private/tmp/bi-sftp-config-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: false)
        return root
    }
    func testConfigurationRoundTripsPathsWithoutReadingPrivateContents() throws {
        let root = try root(); defer { try? FileManager.default.removeItem(at: root) }
        let key = root.appendingPathComponent("私鑰 + test"), hosts = root.appendingPathComponent("known_hosts")
        try Data("PRIVATE-CONTENT-FIXTURE".utf8).write(to: key); try Data("HOST-KEY-FIXTURE".utf8).write(to: hosts)
        let original = DownloadSFTPConfiguration(knownHosts: hosts,privateKey: key)
        let checked = try original.validated()
        XCTAssertEqual(try DownloadSFTPConfiguration.decode(checked.preferenceValue),checked)
        let arguments = try checked.launchArguments()
        XCTAssertEqual(arguments.count,4); XCTAssertEqual(arguments[0],"--sftp-known-hosts"); XCTAssertEqual(arguments[2],"--sftp-private-key")
        try assertSameFile(URL(fileURLWithPath: arguments[1]),hosts)
        try assertSameFile(URL(fileURLWithPath: arguments[3]),key)
        XCTAssertFalse(String(describing: checked.preferenceValue).contains("PRIVATE-CONTENT-FIXTURE"))
        XCTAssertFalse(String(describing: checked.preferenceValue).contains("HOST-KEY-FIXTURE"))
        XCTAssertEqual(try DownloadSFTPConfiguration.decode(nil),DownloadSFTPConfiguration())
        XCTAssertEqual(try DownloadSFTPConfiguration().launchArguments(),[])
    }
    func testMalformedPreferencesRefuseFallbackAndSecretOrURLFields() throws {
        for value: Any in ["future",["version": true],["version": 1.5],["version": 2],
                          ["version": 1,"private_key": "relative/key"],
                          ["version": 1,"private_key": "https://example.test/key"],
                          ["version": 1,"private_key": "/private/tmp/key\n"],
                          ["version": 1,"private_key": String(repeating: "/",count: 4097)],
                          ["version": 1,"known_hosts": 42]] {
            XCTAssertThrowsError(try DownloadSFTPConfiguration.decode(value))
        }
    }
    func testSelectedFilesRejectSymlinksDirectoriesMissingAndNonFileURLs() throws {
        let root = try root(); defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("key"), link = root.appendingPathComponent("key-link")
        try Data("fixture".utf8).write(to: file); try FileManager.default.createSymbolicLink(at: link,withDestinationURL: file)
        try assertSameFile(try DownloadSFTPConfiguration.checkedFile(file),file)
        for url in [root,link,root.appendingPathComponent("missing"),URL(string: "https://example.test/key")!] {
            XCTAssertThrowsError(try DownloadSFTPConfiguration.checkedFile(url))
        }
        XCTAssertThrowsError(try DownloadSFTPConfiguration(privateKey: link).launchArguments())
    }
    func testExplicitSaveRelaunchResetAndRejectedDraftPreserveStoredConfiguration() throws {
        let root = try root(); defer { try? FileManager.default.removeItem(at: root) }
        let key = root.appendingPathComponent("key"); try Data("fixture".utf8).write(to: key)
        let domain = "cc.blueice.sftp-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain)); defer { defaults.removePersistentDomain(forName: domain) }
        defaults.set(["version": 2,"private_key": "/future/key"],forKey: BrowserSFTPPreferences.configurationKey)
        let settings = BrowserSFTPPreferences(defaults: defaults)
        XCTAssertNil(settings.configuration); XCTAssertNotNil(settings.configurationError)
        XCTAssertEqual(defaults.dictionary(forKey: BrowserSFTPPreferences.configurationKey)?["version"] as? Int,2)
        try settings.save(DownloadSFTPConfiguration(privateKey: key))
        let reopened = BrowserSFTPPreferences(defaults: defaults)
        try assertSameFile(reopened.configuration?.privateKey,key); XCTAssertNil(reopened.configurationError)
        XCTAssertThrowsError(try settings.save(DownloadSFTPConfiguration(privateKey: root.appendingPathComponent("missing"))))
        try assertSameFile(BrowserSFTPPreferences(defaults: defaults).configuration?.privateKey,key)
        settings.restoreDefaults()
        XCTAssertNil(defaults.object(forKey: BrowserSFTPPreferences.configurationKey))
        XCTAssertEqual(BrowserSFTPPreferences(defaults: defaults).configuration,DownloadSFTPConfiguration())
    }
}
