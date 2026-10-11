// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Darwin
import XCTest

@MainActor
final class DragDropTests: XCTestCase {
    func testActualStaleHoverReplyPreservesDenialButUnownedErrorsRemainVisible() async throws {
        try await withDropDocument { fixture in
            let model = fixture.model, session = fixture.workspace.session
            model.address = "http://malware.test/hover-denial"; model.navigateAddress()
            try await fixture.waitUntil { model.status.hasPrefix("Navigation blocked: ") }
            let denial = model.status
            let original = session.received
            let gate = HoverFrameReplyGate()
            let observing = XCTestExpectation(description: "The reader has entered the controlled delivery callback")
            let refused = XCTestExpectation(description: "Actual Core rejects the displayed hover frame")
            let ordinary = XCTestExpectation(description: "An unowned request error still reaches browser feedback")
            session.received = { result in
                if case .success(let envelope) = result, case .error(let message) = envelope.message {
                    let count = gate.recordError(message)
                    original?(result)
                    DispatchQueue.main.async {
                        if count == 1 { refused.fulfill() }
                        else if count == 2 { ordinary.fulfill() }
                    }
                } else if !gate.holdIfNeeded(result) {
                    original?(result)
                    if case .success(let envelope) = result, case .textInputState = envelope.message,
                       gate.observeState() {
                        DispatchQueue.main.async { observing.fulfill() }
                    }
                }
            }
            defer {
                session.received = original
                for reply in gate.take() { original?(reply) }
            }
            // The reader may already have captured the previous callback for
            // one blocking read. Two replies establish this callback first.
            for _ in 0..<2 { _ = try await fixture.workspace.send(.unit("GetTextInputState"),tab: model.selected,owner: model) }
            await fulfillment(of: [observing],timeout: 10)
            try await fixture.waitUntil {
                !model.textInputBusy && model.textInputState?.frame_generation == model.generation
                    && model.displayState?.frameGeneration == model.generation
            }
            let displayed = model.generation
            let pointer = try XCTUnwrap(model.textInputState).context
            let context = NativePointerContext(version: 1,frame_source: pointer.frame_source,
                document_generation: pointer.document_generation)
            gate.startHolding()
            // Delay GUI notifications while a separate owned peer changes Core.
            // This exercises the real reply to a hover sent from the old frame.
            let changed = try fixture.changeCoreTextBeforeGUIReceivesItsFrame("New Core frame")
            XCTAssertGreaterThan(changed.frame_generation,displayed)
            XCTAssertEqual(model.generation,displayed)
            model.nativeHover(CGPoint(x: 10,y: 10))
            await fulfillment(of: [refused],timeout: 10)
            XCTAssertEqual(model.status,denial,"A refused background hover must preserve navigation feedback")

            _ = try await fixture.workspace.send(.nativeHover(context,0,CGPoint(x: 10,y: 10)),
                tab: model.selected,owner: model)
            await fulfillment(of: [ordinary],timeout: 10)
            XCTAssertEqual(gate.errors(),Array(repeating: "Native hover frame is stale or outside the viewport",count: 2))
            XCTAssertEqual(model.status,"Native hover frame is stale or outside the viewport",
                "Only errors belonging to model hover requests may be consumed")
            XCTAssertEqual(fixture.http.requests,["/native-drop"])
        }
    }

    func testNativeDropNegotiatesTheActualCoreOperationInTheEntryCallback() async throws {
        try await withDropDocument { fixture in
            let node = try XCTUnwrap(fixture.model.representation?.nodes.first { $0.name == "Drop text" })
            let info = try fixture.drag(to: node,text: "Not dropped",sequence: 81)
            XCTAssertEqual(fixture.view.draggingEntered(info),.copy,
                "AppKit must receive the actual Core operation before the entry callback returns")
            let before = try await fixture.readCore()
            XCTAssertEqual(before.nodes.first { $0.id == node.id }?.state.value, "")
            XCTAssertTrue(fixture.view.prepareForDragOperation(info))
            fixture.view.draggingExited(info)
            let after = try await fixture.readCore()
            XCTAssertEqual(after.nodes.first { $0.id == node.id }?.state.value, "")
            XCTAssertEqual(fixture.http.requests,["/native-drop"])
        }
    }

    func testNativeDropRefusesACoreFrameThatTheGUIHasNotReceived() async throws {
        try await withDropDocument { fixture in
            let node = try XCTUnwrap(fixture.model.representation?.nodes.first { $0.name == "Drop text" })
            let info = try fixture.drag(to: node,text: "Stale displayed-frame payload",sequence: 71)
            try await fixture.waitForCopy(info)
            XCTAssertTrue(fixture.view.prepareForDragOperation(info))
            let displayed = fixture.model.generation
            // The synchronous owned peer changes the actual Core document
            // while this MainActor turn prevents GUI notification delivery.
            let changed = try fixture.changeCoreTextBeforeGUIReceivesItsFrame("Unseen Core value")
            XCTAssertGreaterThan(changed.frame_generation, displayed)
            XCTAssertEqual(fixture.model.generation, displayed)
            XCTAssertFalse(fixture.view.performDragOperation(info), "A broker must not replace the displayed frame with a newer Core frame")
            let actual = try await fixture.readCore()
            XCTAssertEqual(actual.nodes.first { $0.id == node.id }?.state.value, "Unseen Core value")
            fixture.view.concludeDragOperation(info)
            XCTAssertEqual(fixture.http.requests, ["/native-drop"])
        }
    }

    func testDragHeartbeatContinuesInTrackingModeAndStopsAfterOwnerRefusal() {
        var updates = 0
        let heartbeat = NativeDragHeartbeat(interval: 0.01,modes: [.common,.eventTracking]) {
            updates += 1
            return updates < 3
        }
        let deadline = Date().addingTimeInterval(1)
        while updates < 3 && Date() < deadline {
            _ = RunLoop.main.run(mode: .eventTracking,before: deadline)
        }
        XCTAssertEqual(updates, 3)
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertEqual(updates, 3)
        heartbeat.stop()
    }

    func testStoppedDragHeartbeatCannotDispatchADeferredCallback() {
        var updates = 0
        let heartbeat = NativeDragHeartbeat(interval: 0.01,modes: [.common,.eventTracking]) {
            updates += 1; return true
        }
        heartbeat.stop(); heartbeat.stop()
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        XCTAssertEqual(updates, 0)
    }

    func testCustomWebDragFormatsUseValidDistinctPasteboardIdentifiers() {
        XCTAssertEqual(BrowserPageDragging.pasteboardType("text/plain"), .string)
        XCTAssertEqual(BrowserPageDragging.pasteboardType("text/html"), .html)
        XCTAssertEqual(BrowserPageDragging.pasteboardType("text/uri-list"), .URL)
        var identifiers = Set<String>()
        for format in ["application/json", "text/x-blueice", "Custom Type"] {
            let type = BrowserPageDragging.pasteboardType(format)
            XCTAssertTrue(type.rawValue.hasPrefix("cc.bravotek.blueice.drag.custom."))
            XCTAssertTrue(identifiers.insert(type.rawValue).inserted)
            XCTAssertEqual(type, BrowserPageDragging.pasteboardType(format.lowercased()))
            let item = NSPasteboardItem()
            XCTAssertTrue(item.setString("Owned custom data",forType: type))
            XCTAssertEqual(item.string(forType: type), "Owned custom data")
            XCTAssertTrue(NativeDragItem.valid([.text(format: format.lowercased(),data: "Owned custom data")]))
        }
    }

    func testDragPayloadRevocationStopsFutureReadsAndReleasesAdvertisedValues() {
        let payload = NativeDragPayload(values: ["public.utf8-plain-text": Data("Owned 中文".utf8)])
        var received: Data?
        XCTAssertTrue(payload.provide("public.utf8-plain-text") { received = $0 })
        XCTAssertEqual(received, Data("Owned 中文".utf8))
        payload.revoke()
        received = nil
        XCTAssertFalse(payload.provide("public.utf8-plain-text") { received = $0 })
        XCTAssertNil(received)
        payload.revoke()
        XCTAssertFalse(payload.provide("unknown") { _ in XCTFail("Revoked data must never be published") })
    }

    func testDragPayloadSourceRefusalBlocksConcurrentPasteboardReaders() {
        let payload = NativeDragPayload(values: ["text": Data("Owned".utf8)])
        let entered = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0)
        let attemptingRead = DispatchSemaphore(value: 0), readFinished = DispatchSemaphore(value: 0), finished = DispatchGroup()
        finished.enter()
        DispatchQueue.global().async {
            XCTAssertFalse(payload.validateSource {
                entered.signal()
                _ = release.wait(timeout: .now() + 5)
                return false
            })
            finished.leave()
        }
        XCTAssertEqual(entered.wait(timeout: .now() + 5), .success)
        finished.enter()
        DispatchQueue.global().async {
            attemptingRead.signal()
            XCTAssertFalse(payload.provide("text") { _ in XCTFail("A refused source published its payload") })
            readFinished.signal()
            finished.leave()
        }
        XCTAssertEqual(attemptingRead.wait(timeout: .now() + 5), .success)
        XCTAssertEqual(readFinished.wait(timeout: .now() + 0.05), .timedOut)
        release.signal()
        XCTAssertEqual(finished.wait(timeout: .now() + 5), .success)
        XCTAssertFalse(payload.provide("text") { _ in XCTFail("Publication after revocation") })
    }

    func testNativeDragPasteboardProviderStopsUnrequestedFormatsAfterRevocation() throws {
        let payload = NativeDragPayload(values: [
            NSPasteboard.PasteboardType.string.rawValue: Data("Owned 中文".utf8),
            NSPasteboard.PasteboardType.html.rawValue: Data("<p>Owned</p>".utf8),
        ])
        let provider = NativeDragPasteboardProvider(payload: payload)
        let item = NSPasteboardItem()
        XCTAssertTrue(item.setDataProvider(provider, forTypes: [.string, .html]))
        XCTAssertEqual(item.string(forType: .string), "Owned 中文")
        payload.revoke()
        XCTAssertNil(item.data(forType: .html))
        // A receiver may retain bytes already provided before cancellation.
        // Revocation prevents further provision; it cannot recall those bytes.
        XCTAssertEqual(item.string(forType: .string), "Owned 中文")
    }

    func testNativeDropDispatchesPageDragEventsAndKeepsPayloadProtectedUntilDrop() async throws {
        try await withDropDocument(path: "/native-drop-events") { fixture in
            try await fixture.waitForName("Drop script ready")
            let node = try XCTUnwrap(fixture.model.representation?.nodes.first { $0.name == "Script drop zone" })
            let info = try fixture.drag(to: node,text: "Human dropped payload 中文",sequence: 31)
            try await fixture.waitForCopy(info)
            try await fixture.waitForName("protected:text/plain:0:0")
            XCTAssertTrue(fixture.view.prepareForDragOperation(info))
            XCTAssertTrue(fixture.view.performDragOperation(info))
            try await fixture.waitForName("dropped:Human dropped payload 中文:zone")
            fixture.view.concludeDragOperation(info)
            XCTAssertEqual(fixture.http.requests, ["/native-drop-events"])
        }
    }

    func testNativeDropWritesOnlyAfterTheExplicitDropToTheActualCoreDocument() async throws {
        try await withDropDocument { fixture in
            let node = try XCTUnwrap(fixture.model.representation?.nodes.first { $0.name == "Drop text" })
            let info = try fixture.drag(to: node, text: "Dropped 中文 😀", sequence: 41)
            let focused = fixture.model.textInputState?.focused_node
            try await fixture.waitForCopy(info)
            let before = try await fixture.readCore()
            XCTAssertEqual(before.nodes.first { $0.id == node.id }?.state.value, "")
            XCTAssertEqual(fixture.model.textInputState?.focused_node, focused)
            XCTAssertTrue(fixture.view.prepareForDragOperation(info))
            let displayed = fixture.model.generation
            let previousPixels = try XCTUnwrap(fixture.model.image?.dataProvider?.data) as Data
            XCTAssertTrue(fixture.view.performDragOperation(info))
            try await fixture.waitForValue("Dropped 中文 😀", node: node.id)
            let deadline = Date().addingTimeInterval(5)
            while Date() < deadline && (fixture.model.generation <= displayed
                || fixture.model.textInputState?.frame_generation != fixture.model.generation
                || fixture.model.textInputState?.focused?.text != "Dropped 中文 😀") {
                try await Task.sleep(for: .milliseconds(20))
            }
            XCTAssertGreaterThan(fixture.model.generation, displayed,
                "An accepted native drop must publish a new rendered frame to the GUI")
            XCTAssertEqual(fixture.model.displayState?.frameGeneration, fixture.model.generation)
            XCTAssertEqual(fixture.model.textInputState?.focused?.text, "Dropped 中文 😀")
            XCTAssertNotEqual(try XCTUnwrap(fixture.model.image?.dataProvider?.data) as Data, previousPixels,
                "The displayed Core pixels must change after the dropped text is committed")
            fixture.view.concludeDragOperation(info)
            XCTAssertFalse(fixture.view.performDragOperation(info), "An ended drag cannot disclose or apply its payload again")
            XCTAssertEqual(fixture.http.requests, ["/native-drop"])
        }
    }

    func testNativeDropRefusesDisabledInertAndReadOnlyControlsWithoutChangingCoreValues() async throws {
        try await withDropDocument { fixture in
            let original = try await fixture.readCore()
            for (index, name) in ["Disabled drop", "Inert drop", "Read only drop"].enumerated() {
                let node = try XCTUnwrap(original.nodes.first { $0.name == name })
                let info = try fixture.drag(to: node, text: "Must not be inserted", sequence: 50 + index)
                XCTAssertEqual(fixture.view.draggingEntered(info), [])
                for _ in 0..<5 {
                    try await Task.sleep(for: .milliseconds(40))
                    XCTAssertEqual(fixture.view.draggingUpdated(info), [])
                }
                XCTAssertFalse(fixture.view.prepareForDragOperation(info))
                XCTAssertFalse(fixture.view.performDragOperation(info))
                fixture.view.draggingExited(info)
                let actual = try await fixture.readCore()
                XCTAssertEqual(actual.nodes.first { $0.id == node.id }?.state.value, node.state.value)
            }
            XCTAssertEqual(fixture.http.requests, ["/native-drop"])
        }
    }

    func testNativeDropDoesNotRetargetAnOldDragToANewDocumentAtTheSameURL() async throws {
        try await withDropDocument { fixture in
            let original = try XCTUnwrap(fixture.model.representation)
            let node = try XCTUnwrap(original.nodes.first { $0.name == "Drop text" })
            let info = try fixture.drag(to: node, text: "Old document payload", sequence: 61)
            try await fixture.waitForCopy(info)
            let document = fixture.model.textInputState?.document_generation
            fixture.model.address = fixture.http.origin + "/native-drop"
            fixture.model.navigateAddress()
            try await fixture.waitUntil {
                fixture.model.representation?.url == fixture.http.origin + "/native-drop"
                    && fixture.model.textInputState?.document_generation != document
                    && fixture.model.representation?.generation == fixture.model.textInputState?.frame_generation
                    && fixture.model.displayState?.frameGeneration == fixture.model.generation
                    && !fixture.model.textInputBusy
            }
            XCTAssertFalse(fixture.view.prepareForDragOperation(info))
            XCTAssertFalse(fixture.view.performDragOperation(info))
            let actual = try await fixture.readCore()
            XCTAssertEqual(actual.nodes.first { $0.name == "Drop text" }?.state.value, "")
            XCTAssertEqual(fixture.http.requests, ["/native-drop", "/native-drop"])
        }
    }

    private func withDropDocument(path: String = "/native-drop", _ body: (DropDocumentFixture) async throws -> Void) async throws {
        let http = try HTTPFixture(); defer { http.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent()
            .appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(); await workspace.start(launcher: launcher)
        defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1])
        let view = CorePageView(frame: NSRect(x: 0,y: 0,width: 1000,height: 600)); view.model = model
        let window = DropDocumentWindow(contentRect: view.bounds,styleMask: [.borderless],backing: .buffered,defer: false)
        window.isReleasedWhenClosed = false; window.contentView = view; window.orderFront(nil)
        defer { window.close() }
        let fixture = DropDocumentFixture(http: http,workspace: workspace,model: model,view: view,window: window)
        model.address = http.origin + path; model.navigateAddress()
        try await fixture.waitUntil {
            model.representation?.url == http.origin + path
                && model.representation?.generation == model.textInputState?.frame_generation
                && model.displayState?.frameGeneration == model.generation && !model.textInputBusy
        }
        try await body(fixture)
        await workspace.stop()
    }
}

@MainActor
private struct DropDocumentFixture {
    let http: HTTPFixture
    let workspace: BrowserWorkspace
    let model: BrowserModel
    let view: CorePageView
    let window: NSWindow

    func changeCoreTextBeforeGUIReceivesItsFrame(_ text: String) throws -> TextInputState {
        let tab = try XCTUnwrap(model.selected)
        let connection = try BrowserConnection(socketPath: workspace.session.runtimeDirectory.appendingPathComponent("browser.sock").path)
        defer { connection.interrupt(); connection.close() }
        var request: UInt64 = 400
        var writeTimeout = timeval(tv_sec: 2,tv_usec: 0)
        guard setsockopt(connection.output.fileDescriptor,SOL_SOCKET,SO_SNDTIMEO,&writeTimeout,
                         socklen_t(MemoryLayout.size(ofValue: writeTimeout))) == 0 else { throw POSIXError(.EIO) }
        func exchange(_ command: BrowserCommand,scoped: Bool = true) throws -> BrowserMessage {
            request += 1
            let wrapped = scoped ? BrowserCommand.browserContext(.command(model.contextID,.window(.command(model.windowID,command)))) : command
            try connection.output.write(contentsOf: BrowserWire.encode(wrapped,tab: scoped ? tab : nil,request: request))
            let deadline = Date().addingTimeInterval(2)
            for _ in 0..<64 {
                let reply = try BrowserWire.read { count in
                    let remaining = deadline.timeIntervalSinceNow
                    guard remaining > 0 else { throw POSIXError(.ETIMEDOUT) }
                    var waiting = pollfd(fd: connection.input.fileDescriptor,events: Int16(POLLIN),revents: 0)
                    guard Darwin.poll(&waiting,1,Int32(max(1,min(2000,remaining * 1000)))) > 0 else { throw POSIXError(.ETIMEDOUT) }
                    var bytes = [UInt8](repeating: 0,count: count)
                    let size = Darwin.read(connection.input.fileDescriptor,&bytes,count)
                    guard size > 0 else { throw POSIXError(.EIO) }
                    return Data(bytes.prefix(size))
                }
                if reply.requestID != request { continue }
                guard !scoped || reply.tabID == tab else { throw POSIXError(.EIO) }
                switch reply.message {
                case .frame, .viewportState, .displayPreferences: continue
                default: break
                }
                if case .error(let message) = reply.message { throw BrowserFailure.invalid(message) }
                return reply.message
            }
            throw POSIXError(.ETIMEDOUT)
        }
        guard case .hello(BrowserWire.version) = try exchange(.values("Hello",["protocol_version":.unsigned(UInt64(BrowserWire.version))]),scoped: false) else {
            throw BrowserFailure.invalid("The owned Core peer did not receive Hello.")
        }
        guard case .textInputState(let before) = try exchange(.unit("GetTextInputState")) else {
            throw BrowserFailure.invalid("The owned Core peer did not receive its initial text input state.")
        }
        guard case .textInputState(let focused) = try exchange(.textInput(before.context,.key(.tab,false))) else {
            throw BrowserFailure.invalid("The owned Core peer did not receive the Tab text input ACK.")
        }
        guard case .textInputState(let changed) = try exchange(.textInput(focused.context,.replace(text,nil))) else {
            throw BrowserFailure.invalid("The owned Core peer did not receive the replacement text input ACK.")
        }
        try changed.validate()
        return changed
    }

    func drag(to node: PageNode, text: String, sequence: Int) throws -> DropDocumentInfo {
        _ = try XCTUnwrap(
            view.registeredDraggedTypes.contains(.string) ? true : nil,
            "The actual page view must register public text drops with AppKit"
        )
        let snapshot = try XCTUnwrap(model.representation), size = try XCTUnwrap(model.cssViewportSize)
        let rect = snapshot.viewRect(for: node,viewport: view.bounds.size,image: size)
        XCTAssertGreaterThan(rect.width, 0); XCTAssertGreaterThan(rect.height, 0)
        let location = view.convert(NSPoint(x: rect.minX + min(8, rect.width / 2),y: rect.midY),to: nil)
        let pasteboard = NSPasteboard.withUniqueName()
        XCTAssertTrue(pasteboard.setString(text,forType: .string))
        return DropDocumentInfo(window: window,location: location,pasteboard: pasteboard,sequence: sequence)
    }

    func waitForCopy(_ info: DropDocumentInfo) async throws {
        var operation = view.draggingEntered(info)
        try await waitUntil {
            if operation == .copy { return true }
            operation = view.draggingUpdated(info)
            return operation == .copy
        }
    }

    func waitForValue(_ value: String, node: UInt64) async throws {
        let deadline = Date().addingTimeInterval(5)
        while try await readCore().nodes.first(where: { $0.id == node })?.state.value != value {
            guard Date() < deadline else { throw NSError(domain: "BlueIceDropTest",code: 2,userInfo: [NSLocalizedDescriptionKey: "The actual core document did not receive the dropped text"]) }
            try await Task.sleep(for: .milliseconds(20))
        }
    }

    func waitForName(_ name: String) async throws {
        let deadline = Date().addingTimeInterval(10)
        while true {
            let page = try await readCore()
            if page.nodes.contains(where: { $0.name == name }) { return }
            guard Date() < deadline else {
                let observed = page.nodes.compactMap(\.name).filter {
                    $0.hasPrefix("dropped:") || $0.hasPrefix("protected:") || $0 == "No dropped payload"
                }
                throw NSError(domain: "BlueIceDropTest",code: 3,userInfo: [NSLocalizedDescriptionKey:
                    "The page drag listener did not produce the expected core text: " + name
                        + "; observed fixture status: " + observed.joined(separator: " | ")])
            }
            try await Task.sleep(for: .milliseconds(20))
        }
    }

    func readCore() async throws -> PageRepresentation {
        let runtime = workspace.session.runtimeDirectory, context = model.contextID, owner = model.windowID
        let tab = try XCTUnwrap(model.selected)
        let data = try await Task.detached {
            try readOwnedCoreRepresentation(runtime: runtime,context: context,window: owner,tab: tab)
        }.value
        return try JSONDecoder().decode(PageRepresentation.self,from: data)
    }

    func waitUntil(_ condition: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !condition() {
            guard Date() < deadline else { throw NSError(domain: "BlueIceDropTest",code: 1,userInfo: [NSLocalizedDescriptionKey: "The native drop document or operation did not become ready"]) }
            try await Task.sleep(for: .milliseconds(20))
        }
    }
}

@MainActor
private final class DropDocumentWindow: NSWindow {
    // Direct AppKit dispatch is distinct from the required actual OS drag UI test.
    override var isKeyWindow: Bool { isVisible }
}

private final class HoverFrameReplyGate: @unchecked Sendable {
    private let lock = NSLock()
    private var replies: [Result<IncomingEnvelope,BrowserFailure>] = []
    private var messages: [String] = []
    private var holding = false
    private var observed = false
    func startHolding() { lock.withLock { holding = true } }
    func observeState() -> Bool {
        lock.withLock { guard !observed else { return false }; observed = true; return true }
    }
    func holdIfNeeded(_ reply: Result<IncomingEnvelope,BrowserFailure>) -> Bool {
        lock.withLock { guard holding else { return false }; replies.append(reply); return true }
    }
    func recordError(_ message: String) -> Int {
        lock.withLock { messages.append(message); return messages.count }
    }
    func errors() -> [String] { lock.withLock { messages } }
    func take() -> [Result<IncomingEnvelope,BrowserFailure>] {
        lock.withLock { defer { replies.removeAll() }; return replies }
    }
}

@MainActor
private final class DropDocumentInfo: NSObject, NSDraggingInfo {
    let draggingDestinationWindow: NSWindow?
    let draggingLocation: NSPoint
    let draggingPasteboard: NSPasteboard
    let draggingSequenceNumber: Int
    var draggingSourceOperationMask: NSDragOperation { .copy }
    var draggingSource: Any? { nil }
    var draggedImageLocation: NSPoint { draggingLocation }
    nonisolated var draggedImage: NSImage? { nil }
    var draggingFormation: NSDraggingFormation = .default
    var animatesToDestination = false
    var numberOfValidItemsForDrop = 1
    var springLoadingHighlight: NSSpringLoadingHighlight { .none }

    init(window: NSWindow, location: NSPoint, pasteboard: NSPasteboard, sequence: Int) {
        draggingDestinationWindow = window; draggingLocation = location
        draggingPasteboard = pasteboard; draggingSequenceNumber = sequence
        super.init()
    }
    deinit { draggingPasteboard.releaseGlobally() }
    func slideDraggedImage(to screenPoint: NSPoint) {}
    nonisolated override func namesOfPromisedFilesDropped(atDestination dropDestination: URL) -> [String]? { nil }
    func resetSpringLoading() {}
    func enumerateDraggingItems(options: NSDraggingItemEnumerationOptions, for view: NSView?,
        classes classArray: [AnyClass], searchOptions: [NSPasteboard.ReadingOptionKey: Any],
        using block: (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void) {}
}
