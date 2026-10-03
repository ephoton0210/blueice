// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import XCTest

@MainActor
final class DownloadTests: XCTestCase {
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
        await perform(reopened.downloads, "Remove",id: doneID)
        XCTAssertTrue(FileManager.default.fileExists(atPath: done.destPath),"Removing history preserves the completed file")
        await reopened.stop()
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
