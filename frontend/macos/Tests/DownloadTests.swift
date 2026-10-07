// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import CoreServices
import Security
import LocalAuthentication
import XCTest

@MainActor
final class DownloadTests: XCTestCase {
    func testDownloadFolderSelectionPreservesHistoryAcrossSaveRelaunchAndDefault() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bi-folder-prefs-" + UUID().uuidString)
        let old = root.appendingPathComponent("original"), next = root.appendingPathComponent("下載 + new")
        for folder in [old,next] { try FileManager.default.createDirectory(at: folder,withIntermediateDirectories: true) }
        defer { try? FileManager.default.removeItem(at: root) }
        let base = DownloadConfiguration(directory: old,dataDirectory: root.appendingPathComponent("data"))
        let domain = "cc.blueice.folder-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain)); defer { defaults.removePersistentDomain(forName: domain) }
        let preferences = BrowserDownloadFolderPreferences(defaults: defaults)
        let selected = try XCTUnwrap(preferences.selection).changing(to: next,base: base)
        XCTAssertNil(defaults.object(forKey: BrowserDownloadFolderPreferences.configurationKey),"choosing is only a draft")
        try preferences.save(selected,base: base)
        let reopened = BrowserDownloadFolderPreferences(defaults: defaults)
        XCTAssertEqual(try reopened.selection?.resolved(base: base).directory,next.resolvingSymlinksInPath())
        XCTAssertEqual(reopened.selection?.previousDirectories,[old.resolvingSymlinksInPath()])
        let restored = try XCTUnwrap(reopened.selection).changing(to: nil,base: base)
        try reopened.save(restored,base: base)
        XCTAssertEqual(try reopened.selection?.resolved(base: base).directory,old)
        XCTAssertEqual(reopened.selection?.previousDirectories,[next.resolvingSymlinksInPath()])
        XCTAssertThrowsError(try restored.changing(to: root.appendingPathComponent("missing"),base: base))
        XCTAssertEqual(BrowserDownloadFolderPreferences(defaults: defaults).selection,restored)
    }
    func testMalformedDownloadFolderPreferencesRefuseImplicitFallback() throws {
        for value: Any in ["future",["version": true,"previous_directories": []],
                          ["version": 2,"previous_directories": []],
                          ["version": 1,"directory": "relative","previous_directories": []],
                          ["version": 1,"directory": "/tmp/path\n","previous_directories": []],
                          ["version": 1,"previous_directories": [42]],
                          ["version": 1,"previous_directories": Array(repeating: "/tmp/a",count: 33)]] {
            XCTAssertThrowsError(try DownloadFolderSelection.decode(value))
        }
        let domain = "cc.blueice.folder-invalid-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain)); defer { defaults.removePersistentDomain(forName: domain) }
        let future: [String: Any] = ["version": 2,"directory": "/future/folder"]
        defaults.set(future,forKey: BrowserDownloadFolderPreferences.configurationKey)
        let preferences = BrowserDownloadFolderPreferences(defaults: defaults)
        XCTAssertNil(preferences.selection); XCTAssertNotNil(preferences.error)
        XCTAssertEqual(defaults.dictionary(forKey: BrowserDownloadFolderPreferences.configurationKey)?["version"] as? Int,2)
    }
    func testDownloadFolderChooserRejectsFilesSymlinksAndNonlocalURLs() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bi-folder-types-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("file"), link = root.appendingPathComponent("link")
        try Data("fixture".utf8).write(to: file)
        try FileManager.default.createSymbolicLink(at: link,withDestinationURL: root)
        for url in [file,link,root.appendingPathComponent("missing"),URL(string: "https://example.test/folder")!] {
            XCTAssertThrowsError(try DownloadFolderSelection.checkedDirectory(url))
        }
        XCTAssertEqual(try DownloadFolderSelection.checkedDirectory(root),root.resolvingSymlinksInPath())
    }
    func testActualDownloadFolderChangePreservesFilesResumeCoreAndCredentialOwnership() async throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-folder-service-" + UUID().uuidString)
        let original = root.appendingPathComponent("original"), next = root.appendingPathComponent("next")
        for folder in [original,next] { try FileManager.default.createDirectory(at: folder,withIntermediateDirectories: true) }
        try FileManager.default.setAttributes([.posixPermissions: 0o750],ofItemAtPath: next.path)
        defer { try? FileManager.default.removeItem(at: root) }
        let domain = "cc.blueice.folder-service-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain)); defer { defaults.removePersistentDomain(forName: domain) }
        let config = DownloadConfiguration(directory: original,dataDirectory: root.appendingPathComponent("data"))
        let fixture = try DownloadFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(contextDefaults: defaults,downloadConfiguration: config); defer { Task { await workspace.stop() } }
        await workspace.start(launcher: launcher); await workspace.downloads.connect()
        let core = try XCTUnwrap(workspace.processID), old = try XCTUnwrap(workspace.downloads.processID)
        let done = try await started(workspace.downloads,fixture.origin + "/notes.txt",name: "notes.txt")
        await wait { workspace.downloads.transfers.first { $0.id == done }?.state == .completed }
        let reviewedValue = await workspace.downloads.resolveCredentialTarget("sftp://fixture@credential-fixture.invalid:2222")
        let reviewed = try XCTUnwrap(reviewedValue)
        fixture.setSlow(true,delay: 1)
        let large = try await started(workspace.downloads,fixture.origin + "/large-folder-change.txt",name: "folder-change.txt")
        await wait { workspace.downloads.transfers.first { $0.id == large }?.completedBytes ?? 0 > 0 }
        let rejected = await workspace.downloads.applyDownloadFolder(root.appendingPathComponent("missing"))
        XCTAssertFalse(rejected); XCTAssertEqual(workspace.downloads.processID,old)
        XCTAssertNil(defaults.object(forKey: BrowserDownloadFolderPreferences.configurationKey))
        let changed = await workspace.downloads.applyDownloadFolder(next)
        XCTAssertTrue(changed); XCTAssertEqual(workspace.processID,core)
        XCTAssertNotEqual(workspace.downloads.processID,old); XCTAssertNotEqual(kill(old,0),0)
        XCTAssertEqual(workspace.downloads.transfers.first { $0.id == large }?.state,.paused)
        let completed = try XCTUnwrap(workspace.downloads.transfers.first { $0.id == done })
        XCTAssertEqual(completed.state,.completed); XCTAssertTrue(workspace.downloads.hasQuarantine(completed))
        XCTAssertEqual(try Data(contentsOf: workspace.downloads.checkedCompletedFile(completed.destPath)),fixture.bytes)
        XCTAssertThrowsError(try workspace.downloads.checkedCompletedFile(root.appendingPathComponent("outside.txt").path))
        XCTAssertEqual((try FileManager.default.attributesOfItem(atPath: next.path)[.posixPermissions] as? NSNumber)?.intValue,0o750)
        let stale = await workspace.downloads.removeCredential(reviewed,kind: .sftpPassword)
        XCTAssertFalse(stale)
        await workspace.stop()
        let requests = fixture.requests.count
        let reopened = BrowserWorkspace(contextDefaults: defaults,downloadConfiguration: config); defer { Task { await reopened.stop() } }
        await reopened.start(launcher: launcher); await reopened.downloads.connect()
        XCTAssertEqual(reopened.downloads.configuration.directory.path,next.resolvingSymlinksInPath().path)
        XCTAssertEqual(reopened.downloads.transfers.first { $0.id == large }?.state,.paused)
        XCTAssertEqual(fixture.requests.count,requests,"restoring the catalog never resumes downloads")
        fixture.setSlow(false); await perform(reopened.downloads,"Resume",id: large)
        await wait { reopened.downloads.transfers.first { $0.id == large }?.state == .completed }
        XCTAssertEqual(try Data(contentsOf: original.appendingPathComponent("folder-change.txt")),fixture.large)
        XCTAssertFalse(FileManager.default.fileExists(atPath: next.appendingPathComponent("folder-change.txt").path))
        let new = try await started(reopened.downloads,fixture.origin + "/notes.txt",name: "new.txt")
        await wait { reopened.downloads.transfers.first { $0.id == new }?.state == .completed }
        XCTAssertEqual(try Data(contentsOf: next.appendingPathComponent("new.txt")),fixture.bytes)
        let restored = await reopened.downloads.applyDownloadFolder(nil)
        XCTAssertTrue(restored); XCTAssertEqual(reopened.downloads.configuration.directory,original)
        XCTAssertEqual(reopened.downloads.configuration.previousDirectories.map(\.path),[next.resolvingSymlinksInPath().path])
        let newInfo = try XCTUnwrap(reopened.downloads.transfers.first { $0.id == new })
        XCTAssertEqual(try Data(contentsOf: reopened.downloads.checkedCompletedFile(newInfo.destPath)),fixture.bytes)
        await reopened.stop()
    }
    func testUnsupportedFolderPreferenceRepairPreservesHistoryUntilOriginalFolderIsExplicitlyChosen() async throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-folder-history-repair-" + UUID().uuidString)
        let original = root.appendingPathComponent("original"), next = root.appendingPathComponent("new")
        for folder in [original,next] { try FileManager.default.createDirectory(at: folder,withIntermediateDirectories: true) }
        defer { try? FileManager.default.removeItem(at: root) }
        let domain = "cc.blueice.folder-history-repair-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain)); defer { defaults.removePersistentDomain(forName: domain) }
        let config = DownloadConfiguration(directory: root.appendingPathComponent("default"),dataDirectory: root.appendingPathComponent("data"))
        let fixture = try DownloadFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let first = BrowserWorkspace(contextDefaults: defaults,downloadConfiguration: config); defer { Task { await first.stop() } }
        await first.start(launcher: launcher); await first.downloads.connect()
        let chosen = await first.downloads.applyDownloadFolder(original)
        XCTAssertTrue(chosen)
        let id = try await started(first.downloads,fixture.origin + "/notes.txt",name: "human-chosen-name.txt")
        await wait { first.downloads.transfers.first { $0.id == id }?.state == .completed }
        await first.stop()
        let catalog = config.dataDirectory.appendingPathComponent("transfers.json")
        let before = try Data(contentsOf: catalog)
        defaults.set(["version": 9,"directory": original.path],forKey: BrowserDownloadFolderPreferences.configurationKey)
        let reopened = BrowserWorkspace(contextDefaults: defaults,downloadConfiguration: config); defer { Task { await reopened.stop() } }
        await reopened.start(launcher: launcher); await reopened.downloads.connect()
        XCTAssertFalse(reopened.downloads.available)
        let core = try XCTUnwrap(reopened.processID)
        let refused = await reopened.downloads.applyDownloadFolder(next)
        XCTAssertFalse(refused); XCTAssertFalse(reopened.downloads.available)
        XCTAssertEqual(try Data(contentsOf: catalog),before,"repair must preserve unapproved history byte for byte")
        XCTAssertEqual(try Data(contentsOf: original.appendingPathComponent("human-chosen-name.txt")),fixture.bytes)
        let recovered = await reopened.downloads.applyDownloadFolder(original)
        XCTAssertTrue(recovered); XCTAssertEqual(reopened.processID,core)
        let info = try XCTUnwrap(reopened.downloads.transfers.first { $0.id == id })
        XCTAssertEqual(info.state,.completed)
        XCTAssertEqual(try Data(contentsOf: reopened.downloads.checkedCompletedFile(info.destPath)),fixture.bytes)
        let changed = await reopened.downloads.applyDownloadFolder(next)
        XCTAssertTrue(changed)
        XCTAssertEqual(reopened.downloads.transfers.first { $0.id == id }?.state,.completed)
        await reopened.stop()
    }
    func testBrokenDownloadFolderPreferencesRequireExplicitRepairBeforeServiceStartup() async throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-folder-repair-" + UUID().uuidString)
        let folder = root.appendingPathComponent("chosen")
        try FileManager.default.createDirectory(at: folder,withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let domain = "cc.blueice.folder-repair-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain)); defer { defaults.removePersistentDomain(forName: domain) }
        defaults.set(["version": 9,"directory": "/future/folder"],forKey: BrowserDownloadFolderPreferences.configurationKey)
        let config = DownloadConfiguration(directory: root.appendingPathComponent("default"),dataDirectory: root.appendingPathComponent("data"))
        let workspace = BrowserWorkspace(contextDefaults: defaults,downloadConfiguration: config); defer { Task { await workspace.stop() } }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await workspace.start(launcher: launcher); await workspace.downloads.connect()
        XCTAssertFalse(workspace.downloads.available); XCTAssertNil(workspace.downloads.processID)
        XCTAssertEqual(defaults.dictionary(forKey: BrowserDownloadFolderPreferences.configurationKey)?["version"] as? Int,9)
        let repaired = await workspace.downloads.applyDownloadFolder(folder)
        XCTAssertTrue(repaired); XCTAssertTrue(workspace.downloads.available)
        XCTAssertEqual(workspace.downloads.configuration.directory.path,folder.resolvingSymlinksInPath().path)
        await workspace.stop()
        let stopped = await workspace.downloads.applyDownloadFolder(nil)
        XCTAssertFalse(stopped)
    }
    private func credentialExists(_ kind: DownloadCredentialKind, target: ReviewedDownloadCredential) -> Bool {
        let context = LAContext(); context.interactionNotAllowed = true
        let query: [CFString: Any] = [kSecClass: kSecClassGenericPassword,
            kSecAttrService: "org.blueice.downloads.\(kind.service).\(target.identity.host):\(target.identity.port)",
            kSecAttrAccount: target.identity.username, kSecReturnAttributes: true,
            kSecUseAuthenticationContext: context]
        var result: CFTypeRef?
        return SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess
    }
    func testCredentialReplyValidationAndPrivateWireEncoding() throws {
        func packet(_ host: String, port: Int = 22, scheme: String = "sftp") throws -> Data {
            try JSONSerialization.data(withJSONObject: ["request_id": 2, "message": ["CredentialTarget":
                ["scheme": scheme,"host": host,"port": port,"username": "a+b@c"]]])
        }
        let reply = try JSONDecoder().decode(DownloadEnvelope.self,from: packet("[::1]"))
        guard case .credentialTarget(let target) = reply.message else { return XCTFail("Missing account identity") }
        XCTAssertEqual(target.host,"[::1]"); XCTAssertEqual(target.username,"a+b@c")
        for data in [try packet("host",port: 0), try packet("host\n"),try packet("host",scheme: "ftp"),try packet(String(repeating: "x",count: 1025))] {
            XCTAssertThrowsError(try JSONDecoder().decode(DownloadEnvelope.self,from: data))
        }
        let command = DownloadCredentialKind.sftpPassword.command(identity: target, secret: "credential-fixture +密碼")
        let framed = try BrowserWire.encode(command,tab: nil,request: 9)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: framed.dropFirst(4)) as? [String: Any])
        let message = try XCTUnwrap(json["message"] as? [String: Any])
        let fields = try XCTUnwrap(message["SetSftpPassword"] as? [String: Any])
        XCTAssertEqual(fields["password"] as? String,"credential-fixture +密碼")
        XCTAssertEqual(fields["host"] as? String,"[::1]")
        XCTAssertEqual(fields["username"] as? String,"a+b@c")
    }
    func testActualCredentialStoreNamespacesPersistenceRemovalAndStaleServiceRefusal() async throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-credential-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let config = DownloadConfiguration(directory: root.appendingPathComponent("files"),dataDirectory: root.appendingPathComponent("data"))
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(downloadConfiguration: config); defer { Task { await workspace.stop() } }
        await workspace.start(launcher: launcher); await workspace.downloads.connect()
        let url = "sftp://bi-ui-" + UUID().uuidString + "@credential-fixture.invalid:2222"
        let reviewedResult = await workspace.downloads.resolveCredentialTarget(url)
        let reviewed = try XCTUnwrap(reviewedResult)
        let marker = "credential-fixture-" + UUID().uuidString
        defer {
            for kind in DownloadCredentialKind.allCases {
                let context = LAContext(); context.interactionNotAllowed = true
                let query: [CFString: Any] = [kSecClass: kSecClassGenericPassword,
                    kSecAttrService: "org.blueice.downloads.\(kind.service).\(reviewed.identity.host):\(reviewed.identity.port)",
                    kSecAttrAccount: reviewed.identity.username,kSecUseAuthenticationContext: context]
                SecItemDelete(query as CFDictionary)
            }
        }
        for kind in [DownloadCredentialKind.sftpPassword,.sftpPrivateKeyPassphrase] {
            XCTAssertFalse(credentialExists(kind,target: reviewed))
            let saved = await workspace.downloads.saveCredential(reviewed,kind: kind,secret: marker)
            XCTAssertTrue(saved); XCTAssertTrue(credentialExists(kind,target: reviewed))
        }
        let ftpsResult = await workspace.downloads.resolveCredentialTarget(url.replacingOccurrences(of: "sftp:",with: "ftps:"))
        let ftps = try XCTUnwrap(ftpsResult)
        let saved = await workspace.downloads.saveCredential(ftps,kind: .ftpsPassword,secret: marker)
        XCTAssertTrue(saved); XCTAssertTrue(credentialExists(.ftpsPassword,target: ftps))
        let rejected = await workspace.downloads.resolveCredentialTarget("sftp://alice:secret-marker@host/file")
        XCTAssertNil(rejected); XCTAssertFalse(workspace.downloads.notice?.contains("secret-marker") == true)
        XCTAssertTrue(workspace.downloads.transfers.isEmpty)
        await workspace.stop()
        XCTAssertTrue(credentialExists(.sftpPassword,target: reviewed))
        let stale = await workspace.downloads.removeCredential(reviewed,kind: .sftpPassword)
        XCTAssertFalse(stale); XCTAssertTrue(credentialExists(.sftpPassword,target: reviewed))
        let reopened = BrowserWorkspace(downloadConfiguration: config); defer { Task { await reopened.stop() } }
        await reopened.start(launcher: launcher); await reopened.downloads.connect()
        let foreign = await reopened.downloads.removeCredential(reviewed,kind: .sftpPassword)
        XCTAssertFalse(foreign); XCTAssertTrue(credentialExists(.sftpPassword,target: reviewed))
        let freshResult = await reopened.downloads.resolveCredentialTarget(url)
        let fresh = try XCTUnwrap(freshResult)
        let freshFTPSResult = await reopened.downloads.resolveCredentialTarget(url.replacingOccurrences(of: "sftp:",with: "ftps:"))
        let freshFTPS = try XCTUnwrap(freshFTPSResult)
        let wrongScheme = await reopened.downloads.saveCredential(fresh,kind: .ftpsPassword,secret: marker)
        XCTAssertFalse(wrongScheme)
        let empty = await reopened.downloads.saveCredential(fresh,kind: .sftpPassword,secret: "")
        XCTAssertFalse(empty)
        let oversized = await reopened.downloads.saveCredential(fresh,kind: .sftpPassword,secret: String(repeating: "x",count: 4097))
        XCTAssertFalse(oversized); XCTAssertTrue(credentialExists(.sftpPassword,target: fresh))
        for (target,kind) in [(fresh,DownloadCredentialKind.sftpPassword),(fresh,.sftpPrivateKeyPassphrase),(freshFTPS,.ftpsPassword)] {
            let removed = await reopened.downloads.removeCredential(target,kind: kind)
            XCTAssertTrue(removed); XCTAssertFalse(credentialExists(kind,target: target))
            let repeated = await reopened.downloads.removeCredential(target,kind: kind)
            XCTAssertTrue(repeated)
        }
        let files = try XCTUnwrap(FileManager.default.enumerator(at: config.dataDirectory,includingPropertiesForKeys: [.isRegularFileKey]))
        for file in files.allObjects.compactMap({ $0 as? URL }) {
            if (try? file.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true {
                XCTAssertFalse((try Data(contentsOf: file)).range(of: Data(marker.utf8)) != nil)
            }
        }
        await reopened.stop()
    }

    func testSFTPFileChangesCheckpointTransfersAndInvalidateReviewedAccounts() async throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-sftp-service-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let domain = "cc.blueice.sftp-service-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let key = root.appendingPathComponent("私鑰 fixture"), hosts = root.appendingPathComponent("known_hosts")
        try Data("PRIVATE-CONTENT-FIXTURE".utf8).write(to: key)
        try Data("# empty host-key fixture\n".utf8).write(to: hosts)
        let config = DownloadConfiguration(directory: root.appendingPathComponent("files"),dataDirectory: root.appendingPathComponent("data"))
        let fixture = try DownloadFixture(); defer { fixture.stop() }
        fixture.setSlow(true)
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(contextDefaults: defaults,downloadConfiguration: config)
        defer { Task { await workspace.stop() } }
        await workspace.start(launcher: launcher); await workspace.downloads.connect()
        let core = try XCTUnwrap(workspace.processID), old = try XCTUnwrap(workspace.downloads.processID)
        let resolved = await workspace.downloads.resolveCredentialTarget("sftp://fixture@credential-fixture.invalid:2222")
        let reviewed = try XCTUnwrap(resolved)
        let id = try await started(workspace.downloads,fixture.origin + "/large-ssh-change.txt",name: "ssh-change.txt")
        await wait { workspace.downloads.transfers.first { $0.id == id }?.completedBytes ?? 0 > 0 }
        let invalid = await workspace.downloads.applySFTPConfiguration(DownloadSFTPConfiguration(privateKey: root.appendingPathComponent("missing")))
        XCTAssertFalse(invalid); XCTAssertEqual(workspace.downloads.processID,old)
        XCTAssertNil(defaults.object(forKey: BrowserSFTPPreferences.configurationKey))
        let applied = await workspace.downloads.applySFTPConfiguration(DownloadSFTPConfiguration(knownHosts: hosts,privateKey: key))
        XCTAssertTrue(applied); XCTAssertEqual(workspace.processID,core)
        let next = try XCTUnwrap(workspace.downloads.processID)
        XCTAssertNotEqual(next,old); XCTAssertNotEqual(kill(old,0),0)
        XCTAssertEqual(workspace.downloads.transfers.first { $0.id == id }?.state,.paused)
        let pausedBytes = try XCTUnwrap(workspace.downloads.transfers.first { $0.id == id }?.completedBytes)
        try await Task.sleep(for: .milliseconds(150))
        XCTAssertEqual(workspace.downloads.transfers.first { $0.id == id }?.state,.paused)
        XCTAssertEqual(workspace.downloads.transfers.first { $0.id == id }?.completedBytes,pausedBytes)
        let stale = await workspace.downloads.removeCredential(reviewed,kind: .sftpPassword)
        XCTAssertFalse(stale)
        let stored = try XCTUnwrap(defaults.dictionary(forKey: BrowserSFTPPreferences.configurationKey))
        XCTAssertEqual(stored["private_key"] as? String,key.resolvingSymlinksInPath().path)
        XCTAssertEqual(stored["known_hosts"] as? String,hosts.resolvingSymlinksInPath().path)
        XCTAssertFalse(String(describing: stored).contains("PRIVATE-CONTENT-FIXTURE"))
        let restored = await workspace.downloads.restoreSFTPDefaults()
        XCTAssertTrue(restored); XCTAssertNotEqual(kill(next,0),0)
        XCTAssertNil(defaults.object(forKey: BrowserSFTPPreferences.configurationKey))
        XCTAssertEqual(workspace.downloads.transfers.first { $0.id == id }?.state,.paused)
        fixture.setSlow(false); await perform(workspace.downloads,"Resume",id: id)
        await wait { workspace.downloads.transfers.first { $0.id == id }?.state == .completed }
        XCTAssertEqual(try Data(contentsOf: config.directory.appendingPathComponent("ssh-change.txt")),fixture.large)
        await workspace.stop()
        let afterStop = await workspace.downloads.applySFTPConfiguration(DownloadSFTPConfiguration(privateKey: key))
        XCTAssertFalse(afterStop); XCTAssertNil(workspace.downloads.processID)
        XCTAssertNil(defaults.object(forKey: BrowserSFTPPreferences.configurationKey))
    }
    func testBrokenSFTPPreferencesRefuseStartupUntilExplicitRepair() async throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-sftp-repair-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let domain = "cc.blueice.sftp-repair-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        defaults.set(["version": 1,"private_key": root.appendingPathComponent("missing").path],forKey: BrowserSFTPPreferences.configurationKey)
        let config = DownloadConfiguration(directory: root.appendingPathComponent("files"),dataDirectory: root.appendingPathComponent("data"))
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(contextDefaults: defaults,downloadConfiguration: config)
        defer { Task { await workspace.stop() } }
        await workspace.start(launcher: launcher); await workspace.downloads.connect()
        XCTAssertFalse(workspace.downloads.available); XCTAssertNil(workspace.downloads.processID)
        XCTAssertNotNil(workspace.downloads.notice)
        XCTAssertNotNil(defaults.object(forKey: BrowserSFTPPreferences.configurationKey))
        let repaired = await workspace.downloads.restoreSFTPDefaults()
        XCTAssertTrue(repaired); XCTAssertTrue(workspace.downloads.available)
        XCTAssertNotNil(workspace.downloads.processID)
        XCTAssertNil(defaults.object(forKey: BrowserSFTPPreferences.configurationKey))
        await workspace.stop()
    }

    private func started(_ model: BrowserDownloadsModel, _ url: String, name: String?) async throws -> UInt64 {
        let result = await model.start(url, name: name); return try XCTUnwrap(result)
    }
    private func perform(_ model: BrowserDownloadsModel, _ action: String, id: UInt64) async {
        let result = await model.perform(action, id: id); XCTAssertTrue(result)
    }
    private func wait(_ condition: @escaping () -> Bool) async {
        let deadline = Date().addingTimeInterval(20)
        while !condition(), Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(condition())
    }
    func testActualDownloadsAreLazyReviewedSharedPersistentAndCheckpointedOnExit() async throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-download-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let config = DownloadConfiguration(directory: root.appendingPathComponent("files"), dataDirectory: root.appendingPathComponent("data"))
        let fixture = try DownloadFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(downloadConfiguration: config); defer { Task { await workspace.stop() } }
        await workspace.start(launcher: launcher)
        XCTAssertNil(workspace.downloads.processID); XCTAssertFalse(FileManager.default.fileExists(atPath: config.directory.path))
        let core = try XCTUnwrap(workspace.processID)
        await workspace.downloads.connect()
        let process = try XCTUnwrap(workspace.downloads.processID)
        let doneID = try await started(workspace.downloads, fixture.origin + "/notes.txt", name: "notes.txt")
        await wait { workspace.downloads.transfers.first { $0.id == doneID }?.state == .completed }
        let done = try XCTUnwrap(workspace.downloads.transfers.first { $0.id == doneID })
        XCTAssertEqual(try Data(contentsOf: URL(fileURLWithPath: done.destPath)),fixture.bytes)
        let quarantine = try XCTUnwrap(URL(fileURLWithPath: done.destPath)
            .resourceValues(forKeys: [.quarantinePropertiesKey]).quarantineProperties)
        XCTAssertEqual(quarantine[kLSQuarantineAgentNameKey as String] as? String,"BlueIce")
        XCTAssertEqual(quarantine[kLSQuarantineAgentBundleIdentifierKey as String] as? String,"cc.blueice.BlueIce")
        XCTAssertEqual(quarantine[kLSQuarantineTypeKey as String] as? String,kLSQuarantineTypeWebDownload as String)
        XCTAssertNotNil(quarantine[kLSQuarantineTimeStampKey as String] as? Date)
        XCTAssertTrue(workspace.downloads.hasQuarantine(done))
        let model = try XCTUnwrap(workspace.models[1]); model.address = "about:downloads"; model.navigateAddress()
        await wait { model.representation?.url == "about:downloads" && model.representation?.nodes.contains { $0.name?.contains("notes.txt") == true } == true }
        XCTAssertEqual(workspace.processID,core); XCTAssertEqual(workspace.downloads.processID,process)
        let blockedID = try await started(workspace.downloads, fixture.origin + "/setup.exe", name: nil)
        await wait { workspace.downloads.transfers.first { $0.id == blockedID }?.state == .blocked }
        XCTAssertEqual(workspace.downloads.transfers.first { $0.id == blockedID }?.blocked?.category,"dangerous-file-type")
        let largeID = try await started(workspace.downloads, fixture.origin + "/large.txt", name: "large.txt")
        await wait { workspace.downloads.transfers.first { $0.id == largeID }?.completedBytes ?? 0 > 0 }
        await perform(workspace.downloads, "Pause", id: largeID)
        XCTAssertEqual(workspace.downloads.transfers.first { $0.id == largeID }?.state,.paused)
        fixture.setSlow(false); await perform(workspace.downloads, "Resume", id: largeID)
        await wait { workspace.downloads.transfers.first { $0.id == largeID }?.state == .completed }
        XCTAssertEqual(try Data(contentsOf: config.directory.appendingPathComponent("large.txt")),fixture.large)
        fixture.setSlow(true)
        let cancelID = try await started(workspace.downloads, fixture.origin + "/large-cancel.txt", name: "cancel.txt")
        await wait { workspace.downloads.transfers.first { $0.id == cancelID }?.completedBytes ?? 0 > 0 }
        await perform(workspace.downloads, "Cancel",id: cancelID)
        XCTAssertEqual(workspace.downloads.transfers.first { $0.id == cancelID }?.state,.cancelled)
        XCTAssertFalse(FileManager.default.fileExists(atPath: config.directory.appendingPathComponent("cancel.txt.blueice-part").path))
        let activeID = try await started(workspace.downloads, fixture.origin + "/large-exit.txt",name: "exit.txt")
        await wait { workspace.downloads.transfers.first { $0.id == activeID }?.completedBytes ?? 0 > 0 }
        await workspace.stop(); XCTAssertNotEqual(kill(process,0),0)
        let reopened = BrowserWorkspace(downloadConfiguration: config); defer { Task { await reopened.stop() } }
        await reopened.start(launcher: launcher); await reopened.downloads.connect()
        XCTAssertEqual(reopened.downloads.transfers.first { $0.id == activeID }?.state,.paused)
        let restoredGeneration = try XCTUnwrap(reopened.downloads.transfers.first { $0.id == activeID }?.generation)
        await perform(reopened.downloads,"Cancel",id: activeID)
        XCTAssertEqual(reopened.downloads.transfers.first { $0.id == activeID }?.state,.cancelled)
        XCTAssertGreaterThan(try XCTUnwrap(reopened.downloads.transfers.first { $0.id == activeID }?.generation),restoredGeneration)
        XCTAssertEqual(reopened.downloads.transfers.first { $0.id == doneID }?.state,.completed)
        XCTAssertTrue(reopened.downloads.hasQuarantine(try XCTUnwrap(reopened.downloads.transfers.first { $0.id == doneID })))
        await perform(reopened.downloads, "Remove",id: doneID)
        XCTAssertTrue(FileManager.default.fileExists(atPath: done.destPath),"Removing history preserves the completed file")
        await reopened.stop()
    }

    func testQuarantineStatusRequiresTheCurrentCompletedAndConfinedFile() throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-quarantine-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("notes.txt"); try Data("hello".utf8).write(to: file)
        let model = BrowserDownloadsModel(browser: BrowserSession(),configuration: DownloadConfiguration(directory: root,dataDirectory: root))
        func record(_ path: String, state: String = "completed", generation: Int = 1) throws -> DownloadInfo {
            let data = try JSONSerialization.data(withJSONObject: ["message": ["Updated": ["id": 1,"generation": generation,
                "url": "https://example.test/notes.txt","dest_path": path,"state": state,"completed_bytes": 5,"total_bytes": 5]]])
            let envelope = try JSONDecoder().decode(DownloadEnvelope.self,from: data)
            model.receive(envelope)
            if case .updated(let info) = envelope.message { return info }
            throw BrowserFailure.invalid("Expected a download record")
        }
        let completed = try record(file.path)
        XCTAssertFalse(model.hasQuarantine(completed),"Existing local files must not claim system quarantine")
        var values = URLResourceValues()
        values.quarantineProperties = [kLSQuarantineAgentNameKey as String: "BlueIce",kLSQuarantineTypeKey as String: kLSQuarantineTypeWebDownload]
        var target = file; try target.setResourceValues(values)
        XCTAssertTrue(model.hasQuarantine(completed))
        XCTAssertFalse(model.hasQuarantine(try record(file.path,state: "active",generation: 2)))
        XCTAssertFalse(model.hasQuarantine(completed),"Old completed metadata must not authorize status for the current record")
        let link = root.appendingPathComponent("link.txt"); try FileManager.default.createSymbolicLink(at: link,withDestinationURL: file)
        XCTAssertFalse(model.hasQuarantine(try record(link.path,generation: 3)))
        XCTAssertFalse(model.hasQuarantine(try record("/etc/hosts",generation: 4)))
    }

    func testDownloadMetadataRejectsMalformedAndDoesNotOpenOutsideOrSymlinkedFiles() throws {
        let root = URL(fileURLWithPath: "/private/tmp/bi-download-file-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("good.txt"); try Data("hello".utf8).write(to: file)
        XCTAssertEqual(try DownloadConfiguration.checkedFile(file.path,root: root).resolvingSymlinksInPath(),file.resolvingSymlinksInPath())
        XCTAssertThrowsError(try DownloadConfiguration.checkedFile("/etc/hosts",root: root))
        let link = root.appendingPathComponent("link.txt"); try FileManager.default.createSymbolicLink(at: link,withDestinationURL: file)
        XCTAssertThrowsError(try DownloadConfiguration.checkedFile(link.path,root: root))
        let malformed = Data("{\"message\":{\"Transfers\":[{\"id\":0,\"generation\":0,\"url\":\"x\",\"dest_path\":\"x\",\"state\":\"active\",\"completed_bytes\":4,\"total_bytes\":1}]}}".utf8)
        XCTAssertThrowsError(try JSONDecoder().decode(DownloadEnvelope.self,from: malformed))
    }

    func testDownloadReplyFramingAndEventsRejectStaleOrUnownedMetadata() throws {
        func packet(_ variant: String, generation: Int, progress: Int, request: Int? = nil) throws -> Data {
            let info: [String: Any] = ["id": 1,"generation": generation,"url": "https://example.test/notes.txt","dest_path": "/tmp/notes.txt","state": "active","completed_bytes": progress,"total_bytes": 10]
            var envelope: [String: Any] = ["message": [variant: info]]
            if let request { envelope["request_id"] = request }
            return try JSONSerialization.data(withJSONObject: envelope)
        }
        let data = try packet("Updated",generation: 3,progress: 7)
        let length = UInt32(data.count)
        var framed = Data((0..<4).map { UInt8(truncatingIfNeeded: length >> ($0 * 8)) }); framed.append(data)
        var offset = 0
        let event = try DownloadEnvelope.read { count in
            let end = min(offset + min(count,3),framed.count); defer { offset = end }; return framed.subdata(in: offset..<end)
        }
        let model = BrowserDownloadsModel(browser: BrowserSession(),configuration: DownloadConfiguration(directory: URL(fileURLWithPath: "/tmp"),dataDirectory: URL(fileURLWithPath: "/tmp")))
        model.receive(event)
        model.receive(try JSONDecoder().decode(DownloadEnvelope.self,from: packet("Updated",generation: 2,progress: 2)))
        model.receive(try JSONDecoder().decode(DownloadEnvelope.self,from: packet("Transfer",generation: 9,progress: 9,request: 99)))
        XCTAssertEqual(model.transfers.first?.completedBytes,7)
        model.receive(try JSONDecoder().decode(DownloadEnvelope.self,from: Data("{\"request_id\":99,\"message\":{\"Transfers\":[]}}".utf8)))
        XCTAssertEqual(model.transfers.count,1)
        model.receive(try JSONDecoder().decode(DownloadEnvelope.self,from: Data("{\"message\":{\"Removed\":{\"id\":1}}}".utf8)))
        model.receive(try JSONDecoder().decode(DownloadEnvelope.self,from: packet("Updated",generation: 10,progress: 10)))
        XCTAssertTrue(model.transfers.isEmpty,"Late events must not resurrect removed history")
        XCTAssertThrowsError(try DownloadEnvelope.read { _ in Data() })
        XCTAssertThrowsError(try DownloadEnvelope.read { _ in Data([0,0,0,0]) })
        XCTAssertThrowsError(try DownloadEnvelope.read { _ in Data([1,0,128,0]) })
    }
}
