// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import XCTest

final class ProtocolTests: XCTestCase {
    func testFragmentedEnvelopePreservesRequestAndTabIdentity() throws {
        let packet = try BrowserWire.encode(.unit("ListTabs"), tab: 19, request: 7)
        var offset = 0
        let decoded = try BrowserWire.read { count in
            let end = min(offset + min(count, 1), packet.count)
            defer { offset = end }
            return packet.subdata(in: offset..<end)
        }
        XCTAssertEqual(decoded.requestID, 7)
        XCTAssertEqual(decoded.tabID, 19)
        if case .unknown = decoded.message {} else { XCTFail("Unknown variants must fail soft") }
    }

    func testRejectsOversizedAndEmptyFramesBeforeReadingPayload() {
        for prefix in [Data([0, 0, 0, 0]), Data([1, 0, 128, 0])] {
            var reads = 0
            XCTAssertThrowsError(try BrowserWire.read { _ in reads += 1; return prefix })
            XCTAssertEqual(reads, 1)
        }
    }

    func testRejectsTruncatedMessage() {
        var reads = 0
        XCTAssertThrowsError(try BrowserWire.read { _ in
            reads += 1
            return reads == 1 ? Data([4, 0, 0, 0]) : Data()
        })
    }

    func testUnsolicitedBroadcastDoesNotRequireRequestIdentity() throws {
        let payload = Data(#"{"tab_id":2,"message":{"Navigated":{"url":"about:credits"}}}"#.utf8)
        var chunks = [Data([UInt8(payload.count), 0, 0, 0]), payload]
        let envelope = try BrowserWire.read { _ in chunks.removeFirst() }
        XCTAssertNil(envelope.requestID)
        XCTAssertEqual(envelope.tabID, 2)
        if case .navigated("about:credits") = envelope.message {} else { XCTFail("Expected navigation") }
    }

    func testCommandsMatchRustEnvelope() throws {
        let packet = try BrowserWire.encode(.values("Navigate", ["url": .string("about:credits")]), tab: 2, request: 8)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: packet.dropFirst(4)) as? [String: Any])
        XCTAssertEqual(json["request_id"] as? Int, 8)
        XCTAssertEqual(json["tab_id"] as? Int, 2)
        let message = try XCTUnwrap(json["message"] as? [String: [String: String]])
        XCTAssertEqual(message["Navigate"]?["url"], "about:credits")
        let open = try BrowserWire.encode(.values("OpenTab", ["url": .null]), tab: nil, request: 9)
        let openJSON = try XCTUnwrap(JSONSerialization.jsonObject(with: open.dropFirst(4)) as? [String: Any])
        let openMessage = try XCTUnwrap(openJSON["message"] as? [String: [String: Any]])
        XCTAssertTrue(openMessage["OpenTab"]?["url"] is NSNull)
    }

    func testFramePathSizeAndSymlinkBoundaries() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("frame.rgba")
        try Data([10, 20, 30, 255]).write(to: file)
        let frame = FrameNotice(path: file.path, width: 1, height: 1, generation: 1)
        let pixels = try FramePixels.read(frame, directory: root)
        XCTAssertEqual(pixels.data, Data([10, 20, 30, 255]))
        XCTAssertEqual(try pixels.image().width, 1)
        XCTAssertThrowsError(try FramePixels.read(FrameNotice(path: file.path, width: 2, height: 1, generation: 1), directory: root))
        XCTAssertThrowsError(try FramePixels.read(FrameNotice(path: file.path, width: 4097, height: 1, generation: 1), directory: root))
        XCTAssertThrowsError(try FramePixels.read(frame, directory: root.appendingPathComponent("other")))
        let link = root.appendingPathComponent("escape.rgba")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: URL(fileURLWithPath: "/etc/hosts"))
        XCTAssertThrowsError(try FramePixels.read(FrameNotice(path: link.path, width: 1, height: 1, generation: 1), directory: root))
    }

    func testRealCoreRendersAndStopsOwnedProcess() async throws {
        let core = Bundle(for: Self.self).bundleURL.deletingLastPathComponent()
            .appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-core")
        let rendered = expectation(description: "core pixels")
        let blocked = expectation(description: "external navigation denied")
        let session = BrowserSession()
        session.received = { result in
            do {
                let envelope = try result.get()
                switch envelope.message {
                case .frame(let notice):
                    XCTAssertEqual(envelope.tabID, 1)
                    do {
                        let frame = try FramePixels.read(notice, directory: session.frameDirectory)
                        XCTAssertTrue(frame.data.contains { $0 < 200 })
                        rendered.fulfill()
                    } catch { XCTFail("Frame: \(error)") }
                case .blocked: blocked.fulfill()
                default: break
                }
            } catch { XCTFail("Connection: \(error)") }
        }
        try await session.start(executable: core)
        let pid = try XCTUnwrap(session.processID)
        try await session.send(.values("Navigate", ["url": .string("about:credits")]), tab: 1)
        await fulfillment(of: [rendered], timeout: 20)
        try await session.send(.values("Navigate", ["url": .string("https://example.invalid")]), tab: 1)
        await fulfillment(of: [blocked], timeout: 20)
        await session.stop()
        await session.stop()
        XCTAssertFalse(FileManager.default.fileExists(atPath: session.frameDirectory.path))
        XCTAssertEqual(kill(pid, 0), -1)
    }

    func testMissingCoreFailsWithoutCreatingFrames() async {
        let session = BrowserSession()
        do { try await session.start(executable: URL(fileURLWithPath: "/nonexistent/blueice-core")); XCTFail("Expected failure") }
        catch {}
        await session.stop()
        XCTAssertFalse(FileManager.default.fileExists(atPath: session.frameDirectory.path))
    }
}
