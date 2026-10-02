// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import XCTest

final class ProtocolTests: XCTestCase {
    func testTabGroupWireRetainsCoreIDsAndRejectsMalformedMetadata() throws {
        func message(_ value: [String: Any]) throws -> IncomingEnvelope {
            try JSONDecoder().decode(IncomingEnvelope.self, from: JSONSerialization.data(withJSONObject:
                ["request_id": 7, "tab_id": 2, "message": value]))
        }
        let group: [String: Any] = ["id": 9, "name": "研究", "color": "#4477cc", "collapsed": true]
        for variant in ["TabGroupCreated", "TabGroupUpdated"] {
            let envelope = try message([variant: group])
            XCTAssertEqual(envelope.requestID, 7)
            if case .groupChanged(let decoded, let created) = envelope.message {
                XCTAssertEqual(decoded.id, 9); XCTAssertEqual(decoded.name, "研究")
                XCTAssertEqual(decoded.color, "#4477cc"); XCTAssertTrue(decoded.collapsed)
                XCTAssertEqual(created, variant == "TabGroupCreated")
            } else { XCTFail("Core group state must reach native chrome") }
        }
        if case .groups(let groups) = try message(["TabGroups": [group]]).message { XCTAssertEqual(groups.count, 1) }
        else { XCTFail("Group list must decode") }
        for invalid in [
            ["TabGroups": [group, group]],
            ["TabGroupCreated": ["id": 0, "name": "Research", "color": "#4477cc", "collapsed": false]],
            ["TabGroupUpdated": ["id": 9, "name": "", "color": "#4477cc", "collapsed": false]],
            ["TabGroupUpdated": ["id": 9, "name": "Research", "color": "red", "collapsed": false]],
            ["TabGroupUpdated": ["id": 9, "name": "Research", "color": "#4477cc", "collapsed": "yes"]],
            ["TabGroupAssigned": ["tab_id": 2]],
            ["TabGroupAssigned": ["tab_id": 0, "group_id": NSNull()]],
            ["TabGroupAssigned": ["tab_id": 2, "group_id": 0]],
            ["TabGroupClosed": ["group_id": 0]]
        ] as [[String: Any]] {
            if case .groupsUnavailable = try message(invalid).message {} else { XCTFail("Invalid group metadata must fail soft") }
        }
        if case .tabs(let tabs) = try message(["Tabs": [["id": 2, "url": "about:credits", "group_id": 9], ["id": 3, "url": NSNull()]]]).message {
            XCTAssertEqual(tabs[0].groupID, 9); XCTAssertNil(tabs[1].groupID)
        } else { XCTFail("Legacy ungrouped and core-grouped tabs must coexist") }
        if case .groupAssigned(let tab, let id) = try message(["TabGroupAssigned": ["tab_id": 2, "group_id": 9]]).message {
            XCTAssertEqual(tab, 2); XCTAssertEqual(id, 9)
        } else { XCTFail("Assignment must retain its tab identity") }
        if case .groupAssigned(let tab, nil) = try message(["TabGroupAssigned": ["tab_id": 2, "group_id": NSNull()]]).message {
            XCTAssertEqual(tab, 2)
        } else { XCTFail("Only explicit null removes membership") }
        if case .groupClosed(let id) = try message(["TabGroupClosed": ["group_id": 9]]).message { XCTAssertEqual(id, 9) }
        else { XCTFail("Group removal must decode independently of tab closure") }
    }

    func testNativeTabGroupCommandsEncodeExactTargetAndNullableMembership() throws {
        for (action, name) in [(TabGroupAction.create("Research", "#4477cc"), "CreateTabGroup"),
                               (.assign(nil), "SetTabGroup"), (.rename(9, "Work"), "RenameTabGroup"),
                               (.color(9, "#cc3344"), "SetTabGroupColor"), (.collapse(9, true), "SetTabGroupCollapsed"),
                               (.remove(9), "CloseTabGroup")] {
            let bytes = try BrowserWire.encode(.tabGroup(action), tab: 2, request: 7)
            let root = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes.dropFirst(4)) as? [String: Any])
            XCTAssertEqual(root["tab_id"] as? UInt64, 2); XCTAssertEqual(root["request_id"] as? UInt64, 7)
            let payload = try XCTUnwrap((root["message"] as? [String: Any])?[name] as? [String: Any])
            if name == "SetTabGroup" { XCTAssertTrue(payload["group_id"] is NSNull) }
            if name == "SetTabGroupCollapsed" { XCTAssertEqual(payload["collapsed"] as? Bool, true) }
        }
    }
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

    func testFindWireContextAndMalformedResults() throws {
        let context = TextInputContext(version: 1, frame_source: 19, document_generation: 4, focus_generation: 3)
        let packet = try BrowserWire.encode(.find(2, context, .update("冰晶", false)), tab: 2, request: 9)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: packet.dropFirst(4)) as? [String: Any])
        let message = try XCTUnwrap(json["message"] as? [String: Any])
        let find = try XCTUnwrap(message["Find"] as? [String: Any])
        XCTAssertEqual(find["tab_id"] as? Int, 2)
        XCTAssertEqual(find["document_generation"] as? Int, 4)
        XCTAssertNil(find["focus_generation"], "Find survives control focus changes")
        func decode(_ count: Int, _ active: Int?, _ width: Double) throws -> BrowserMessage {
            let state: [String: Any] = ["tab_id": 2, "frame_source": 19, "document_generation": 4, "revision": 1,
                                        "query": "冰晶", "case_sensitive": false, "match_count": count,
                                        "active_match": active as Any? ?? NSNull(), "wrapped": false, "limited": false,
                                        "rects": [["x": 0, "y": 0, "width": width, "height": 20]]]
            let data = try JSONSerialization.data(withJSONObject: ["tab_id": 2, "message": ["FindState": state]])
            return try JSONDecoder().decode(IncomingEnvelope.self, from: data).message
        }
        if case .findState(let value) = try decode(2, 1, 30) { XCTAssertEqual(value.matchCount, 2) }
        else { XCTFail("Expected bounded find result") }
        for value in [try decode(2, 3, 30), try decode(10001, 1, 30), try decode(2, 1, -1), try decode(0, nil, 30)] {
            if case .findUnavailable = value {} else { XCTFail("Invalid geometry or result count must fail soft") }
        }
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

    private var launcher: URL {
        Bundle(for: Self.self).bundleURL.deletingLastPathComponent()
            .appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
    }

    func testLauncherReviewsHTTPAndPreservesCommittedPageOnDenial() async throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        let session = BrowserSession()
        let events = SessionEvents(directory: session.frameDirectory)
        session.received = { events.record($0) }
        try await session.start(launcher: launcher)
        let pid = try XCTUnwrap(session.processID)
        do { try await session.start(launcher: launcher); XCTFail("Duplicate startup must fail") }
        catch { XCTAssertTrue(error.localizedDescription.contains("already")) }
        XCTAssertEqual(session.processID, pid, "Rejecting a duplicate start must retain the live stack")
        XCTAssertEqual(getpgid(pid), pid, "The service stack must have its own process group")
        let attributes = try FileManager.default.attributesOfItem(atPath: session.runtimeDirectory.path)
        XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o700)
        let safe = fixture.origin + "/safe"
        try await session.send(.values("Navigate", ["url": .string(safe)]), tab: 1)
        await events.wait { $0.urls.contains(safe) && $0.greenFrame }
        XCTAssertEqual(fixture.requests, ["/safe"])
        let committedGeneration = events.snapshot.generation
        try await session.send(.unit("GetHistoryState"), tab: 1)
        await events.wait { $0.historyCount == 1 }
        let committedHistory = try XCTUnwrap(events.snapshot.history)

        try await session.send(.values("Navigate", ["url": .string("http://malware.test/")]), tab: 1)
        await events.wait { $0.blocked == 1 }
        XCTAssertEqual(fixture.requests, ["/safe"])
        try await session.send(.values("Navigate", ["url": .string(fixture.origin + "/blocked")]), tab: 1)
        await events.wait { $0.blocked == 2 }
        XCTAssertEqual(fixture.requests, ["/safe", "/blocked"])
        XCTAssertEqual(events.snapshot.urls, [safe])
        XCTAssertEqual(events.snapshot.generation, committedGeneration)
        try await session.send(.unit("ListTabs"))
        try await session.send(.unit("GetHistoryState"), tab: 1)
        await events.wait { $0.committedURL == safe && $0.historyCount == 2 }
        XCTAssertEqual(try XCTUnwrap(events.snapshot.history).back, committedHistory.back)
        XCTAssertEqual(try XCTUnwrap(events.snapshot.history).forward, committedHistory.forward)
        XCTAssertTrue(events.snapshot.errors.isEmpty, events.snapshot.errors.joined(separator: "\n"))
        await session.stop()
        await session.stop()
        XCTAssertEqual(kill(pid, 0), -1)
        XCTAssertEqual(kill(-pid, 0), -1, "Core and gatekeeper must also be gone")
        XCTAssertFalse(FileManager.default.fileExists(atPath: session.runtimeDirectory.path))
    }

    func testMissingGatekeeperFailsWithoutStartingUnreviewedCore() async throws {
        let directory = URL(fileURLWithPath: "/private/tmp/blueice-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        for name in ["blueice-launcher", "blueice-core"] {
            try FileManager.default.copyItem(at: launcher.deletingLastPathComponent().appendingPathComponent(name),
                                            to: directory.appendingPathComponent(name))
        }
        let session = BrowserSession()
        do { try await session.start(launcher: directory.appendingPathComponent("blueice-launcher")); XCTFail("Expected failure") }
        catch { XCTAssertTrue(error.localizedDescription.contains("gatekeeper"), error.localizedDescription) }
        await session.stop()
        XCTAssertNil(session.processID)
        XCTAssertFalse(FileManager.default.fileExists(atPath: session.runtimeDirectory.path))
    }

    func testForcedShutdownKillsOwnedDescendants() throws {
        // The owner ignores TERM and keeps a child alive, exercising the
        // bounded process-group fallback rather than normal launcher exit.
        let process = try OwnedBrowserProcess(executable: URL(fileURLWithPath: "/bin/sh"),
            arguments: ["-c", "trap '' TERM; sleep 120 & wait"], environment: ProcessInfo.processInfo.environment)
        let pid = process.pid
        XCTAssertEqual(getpgid(pid), pid)
        process.finish(grace: 0.1)
        XCTAssertEqual(kill(pid, 0), -1)
        // Orphaned grandchildren can briefly be zombies until launchd reaps them.
        let deadline = Date().addingTimeInterval(5)
        while kill(-pid, 0) == 0 && Date() < deadline { Thread.sleep(forTimeInterval: 0.02) }
        XCTAssertEqual(kill(-pid, 0), -1)
    }

    func testUnavailableReviewServiceDeniesHTTPWithoutFetching() async throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        let session = BrowserSession()
        let events = SessionEvents(directory: session.frameDirectory)
        session.received = { events.record($0) }
        try await session.start(launcher: launcher)
        try await session.send(.values("Navigate", ["url": .string("about:credits")]), tab: 1)
        await events.wait { $0.urls == ["about:credits"] && $0.generation > 0 }
        let generation = events.snapshot.generation
        let sockets = try FileManager.default.contentsOfDirectory(at: session.runtimeDirectory.appendingPathComponent("blueice"),
                                                                 includingPropertiesForKeys: nil)
        let gatekeeper = try XCTUnwrap(sockets.first { $0.lastPathComponent.hasPrefix("blueice-launcher-gatekeeper-") })
        try FileManager.default.removeItem(at: gatekeeper)
        try await session.send(.values("Navigate", ["url": .string(fixture.origin + "/safe")]), tab: 1)
        await events.wait { $0.blocked == 1 }
        XCTAssertTrue(fixture.requests.isEmpty, "Unavailable review must deny before fetching HTTP")
        XCTAssertEqual(events.snapshot.urls, ["about:credits"])
        XCTAssertEqual(events.snapshot.generation, generation)
        let pid = try XCTUnwrap(session.processID)
        await session.stop()
        XCTAssertEqual(kill(-pid, 0), -1)
        XCTAssertFalse(FileManager.default.fileExists(atPath: session.runtimeDirectory.path))
    }

    func testStopDuringLauncherStartupIsBoundedAndCleansDescendants() async throws {
        let directory = URL(fileURLWithPath: "/private/tmp/blueice-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let executable = directory.appendingPathComponent("blueice-launcher")
        try Data("#!/bin/sh\ntrap '' TERM\nsleep 120 &\nwait\n".utf8).write(to: executable)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: executable.path)
        for name in ["blueice-core", "blueice-ai-gatekeeper"] {
            try FileManager.default.createSymbolicLink(at: directory.appendingPathComponent(name),
                withDestinationURL: launcher.deletingLastPathComponent().appendingPathComponent(name))
        }
        let session = BrowserSession()
        let startup = Task { try await session.start(launcher: executable) }
        let deadline = Date().addingTimeInterval(5)
        while session.processID == nil && Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        let pid = try XCTUnwrap(session.processID)
        let before = Date()
        await session.stop()
        XCTAssertLessThan(Date().timeIntervalSince(before), 5)
        do { try await startup.value; XCTFail("A stopped startup must not report ready") } catch {}
        XCTAssertEqual(kill(pid, 0), -1)
        XCTAssertFalse(FileManager.default.fileExists(atPath: session.runtimeDirectory.path))
    }
    func testResubmissionProtocolHasMetadataAndTypedBooleanConsent() throws {
        let payload = Data(#"{"tab_id":2,"message":{"FormResubmission":{"confirmation_id":42,"url":"https://example.test/submit"}}}"#.utf8)
        let envelope = try JSONDecoder().decode(IncomingEnvelope.self, from: payload)
        if case .formResubmission(let prompt) = envelope.message { XCTAssertEqual(prompt.confirmationID, 42); XCTAssertEqual(prompt.url, "https://example.test/submit") }
        else { XCTFail("Expected bounded form prompt metadata") }
        let packet = try BrowserWire.encode(.values("ConfirmFormResubmission", ["confirmation_id": .unsigned(42), "accept": .boolean(false)]), tab: 2, request: 8)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: packet.dropFirst(4)) as? [String: Any])
        let message = try XCTUnwrap(json["message"] as? [String: [String: Any]])
        XCTAssertEqual(message["ConfirmFormResubmission"]?["accept"] as? Bool, false)
        XCTAssertEqual(message["ConfirmFormResubmission"]?["confirmation_id"] as? Int, 42)
    }

}

private final class SessionEvents: @unchecked Sendable {
    struct Snapshot {
        var urls: [String] = []
        var committedURL: String?
        var history: HistoryState?
        var historyCount = 0
        var blocked = 0
        var greenFrame = false
        var generation: UInt64 = 0
        var errors: [String] = []
    }
    private let directory: URL
    private let lock = NSLock()
    private var value = Snapshot()
    init(directory: URL) { self.directory = directory }
    var snapshot: Snapshot { lock.withLock { value } }
    func record(_ result: Result<IncomingEnvelope, BrowserFailure>) {
        do {
            let envelope = try result.get()
            if case .frame(let notice) = envelope.message {
                let frame = try FramePixels.read(notice, directory: directory)
                let green = frame.data.withUnsafeBytes { bytes in
                    stride(from: 0, to: bytes.count, by: 4).contains {
                        bytes[$0] < 60 && bytes[$0 + 1] > 100 && bytes[$0 + 1] < 160 && bytes[$0 + 2] < 80
                    }
                }
                lock.withLock { value.generation = notice.generation; value.greenFrame = green }
            } else {
                lock.withLock {
                    switch envelope.message {
                    case .navigated(let url): value.urls.append(url)
                    case .tabs(let tabs): value.committedURL = tabs.first?.url
                    case .history(let history): value.history = history; value.historyCount += 1
                    case .blocked: value.blocked += 1
                    case .error(let error): value.errors.append(error)
                    default: break
                    }
                }
            }
        } catch { lock.withLock { value.errors.append(error.localizedDescription) } }
    }
    func wait(_ predicate: (Snapshot) -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
        let deadline = Date().addingTimeInterval(15)
        while !predicate(snapshot) && Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(snapshot), "Timed out; errors: \(snapshot.errors)", file: file, line: line)
    }


}
