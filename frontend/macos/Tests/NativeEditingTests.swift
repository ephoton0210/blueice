// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import XCTest

@MainActor
final class NativeEditingTests: XCTestCase {
    func testUTF16WireReplacementCompositionAndMovement() throws {
        let context = TextInputContext(version: 1, frame_source: 19, document_generation: 4, focus_generation: 3)
        let action = TextInputAction.compose("中文", TextRange(NSRange(location: 2, length: 0)), TextRange(NSRange(location: 1, length: 2)))
        let bytes = try BrowserWire.encode(.textInput(context, action), tab: 2, request: 7)
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes.dropFirst(4)) as? [String: Any])
        let message = try XCTUnwrap(root["message"] as? [String: Any])
        let input = try XCTUnwrap(message["TextInput"] as? [String: Any])
        let encodedContext = try XCTUnwrap(input["context"] as? [String: Any])
        XCTAssertEqual(encodedContext["focus_generation"] as? Int, 3)
        let encodedAction = try XCTUnwrap(input["action"] as? [String: Any])
        let compose = try XCTUnwrap(encodedAction["Compose"] as? [String: Any])
        XCTAssertEqual(compose["text"] as? String, "中文")
        XCTAssertEqual((compose["replacement"] as? [String: Int])?["length"], 2)
        XCTAssertNil(TextRange.replacement(NSRange(location: NSNotFound, length: 0)))
        XCTAssertNil(TextRange.replacement(NSRange(location: 65_536, length: 1)))
    }

    func testMalformedTextStateFailsSoftAndRejectsProtectedPlaintext() throws {
        func decode(_ range: Int, protected: Bool = false) throws -> BrowserMessage {
            let bounds: [String: Int] = ["x": 2, "y": 3, "width": 100, "height": 20]
            let state: [String: Any] = ["version": 1, "frame_source": 19, "document_generation": 4, "focus_generation": 3,
                                       "frame_generation": 7, "tab_id": 2, "scroll_y": 0,
                                       "focused": ["node_id": 5, "text": "A😀B", "text_length": 4, "protected": protected,
                                                   "writable": true, "multiline": false, "selection": ["location": range, "length": 0],
                                                   "marked": NSNull(), "bounds": bounds, "caret": bounds, "carets": [], "selection_rects": []]]
            let data = try JSONSerialization.data(withJSONObject: ["message": ["TextInputState": state]])
            return try JSONDecoder().decode(IncomingEnvelope.self, from: data).message
        }
        if case .textInputState = try decode(3) {} else { XCTFail("Valid UTF-16 state must decode") }
        for state in [try decode(2), try decode(9), try decode(0, protected: true)] {
            if case .textInputUnavailable = state {} else { XCTFail("Malformed or leaking state must fail soft") }
        }
    }

    private func wait(_ predicate: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
        let deadline = Date().addingTimeInterval(15)
        while !predicate() && Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Timed out waiting for real core text state", file: file, line: line)
    }

    private func start(path: String = "/editing") async throws -> (BrowserModel, CorePageView, HTTPFixture) {
        let fixture = try HTTPFixture()
        let model = BrowserModel()
        let view = CorePageView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        view.model = model
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await model.start(launcher: launcher)
        await wait { model.representation != nil }
        model.address = fixture.origin + path; model.navigateAddress()
        await wait { model.representation?.url == fixture.origin + path && model.textInputState != nil }
        return (model, view, fixture)
    }

    func testClipboardCommandsWaitForCoreSelectionAndPreservePrivacy() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        let saved = NSPasteboard.general.pasteboardItems?.map { item -> NSPasteboardItem in
            let copy = NSPasteboardItem()
            for type in item.types { if let data = item.data(forType: type) { copy.setData(data, forType: type) } }
            return copy
        } ?? []
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        try await focus("Editor", model: model, view: view)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("clipboard-sentinel", forType: .string)
        view.selectAll(nil)
        XCTAssertTrue(model.textInputBusy, "Exercise Copy before selection acknowledgement")
        view.copy(nil)
        await wait { !model.textInputBusy && NSPasteboard.general.string(forType: .string) == "A😀B" }
        guard NSPasteboard.general.string(forType: .string) == "A😀B" else { return }
        view.selectAll(nil); view.cut(nil)
        await wait { !model.textInputBusy && model.textInputState?.focused?.text == "" }
        view.paste(nil)
        await wait { !model.textInputBusy && model.textInputState?.focused?.text == "A😀B" }
        // Paste reads only when its turn is reached, after the queued Copy.
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("clipboard-sentinel", forType: .string)
        view.selectAll(nil); view.copy(nil); view.insertText("new", replacementRange: NSRange(location: NSNotFound, length: 0)); view.paste(nil)
        await wait { !model.textInputBusy && model.textInputState?.focused?.text == "newA😀B" }
        try await focus("Secret", model: model, view: view)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("clipboard-sentinel", forType: .string)
        view.selectAll(nil); view.copy(nil); view.cut(nil)
        await wait { !model.textInputBusy }
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "clipboard-sentinel")
        XCTAssertEqual(model.textInputState?.focused?.text_length, 22)
        try await focus("Readonly", model: model, view: view)
        view.selectAll(nil); view.cut(nil)
        await wait { !model.textInputBusy }
        XCTAssertEqual(model.textInputState?.focused?.text, "locked")
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "clipboard-sentinel")
        view.copy(nil)
        await wait { NSPasteboard.general.string(forType: .string) == "locked" }
        model.action(.values("OpenTab", ["url": .null]))
        await wait { model.selected == 2 && model.representation?.url == "about:credits" }
        model.select(1)
        await wait { model.representation?.url == fixture.origin + "/editing" }
        try await focus("Editor", model: model, view: view)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("clipboard-sentinel", forType: .string)
        view.selectAll(nil); view.copy(nil); model.select(2)
        await wait { !model.textInputBusy }
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "clipboard-sentinel", "A queued Copy cannot write after switching tabs")
    }

    func testFindThroughRealCorePreservesTabStateAndTracksLiveEdits() async throws {
        let (model, view, fixture) = try await start(path: "/find")
        defer { fixture.stop(); Task { await model.stop() } }
        model.showFind(); model.setFindQuery("frost")
        await wait { model.findResult?.matchCount == 4 }
        XCTAssertEqual(model.findResult?.activeMatch, 1)
        model.findNext(backwards: true)
        await wait { model.findResult?.activeMatch == 4 && (model.representation?.scrollY ?? 0) > 800 }
        XCTAssertTrue(model.findResult?.wrapped == true)
        model.findNext()
        await wait { model.findResult?.activeMatch == 1 && model.representation?.scrollY == 0 }
        model.setFindCaseSensitive(true)
        await wait { model.findResult?.matchCount == 3 }
        model.setFindQuery("private-find-secret")
        await wait { model.findResult?.matchCount == 0 }
        model.setFindQuery("hidden-find-secret")
        await wait { model.findResult?.matchCount == 0 }
        model.setFindQuery("public-editor")
        await wait { model.findResult?.matchCount == 1 }
        try await focus("Find public editor", model: model, view: view)
        view.selectAll(nil); view.insertText("updated-editor", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { !model.textInputBusy && model.findResult?.matchCount == 0 }
        model.setFindQuery("updated-editor")
        await wait { model.findResult?.matchCount == 1 }
        model.action(.values("OpenTab", ["url": .null]))
        await wait { model.selected == 2 && model.representation?.url == "about:credits" }
        XCTAssertFalse(model.findVisible)
        model.showFind(); model.setFindQuery("missing-second-tab")
        await wait { model.findResult?.matchCount == 0 }
        model.select(1)
        await wait { model.findVisible && model.findQuery == "updated-editor" && model.findResult?.matchCount == 1 }
        model.closeFind()
        XCTAssertFalse(model.findVisible)
        model.address = "about:credits"; model.navigateAddress()
        await wait { model.representation?.url == "about:credits" }
        model.showFind()
        XCTAssertEqual(model.findQuery, "")
        XCTAssertEqual(fixture.requests.filter { $0 == "/find" }.count, 1, "Finding and stepping never fetches")
    }

    private func focus(_ label: String, model: BrowserModel, view: CorePageView) async throws {
        await wait { model.representation != nil && !model.textInputBusy }
        let snapshot = try XCTUnwrap(model.representation)
        let node = try XCTUnwrap(snapshot.nodes.first { $0.name == label })
        model.focusPage(x: node.bounds.x + node.bounds.width - 8, y: node.bounds.y - snapshot.scrollY + node.bounds.height / 2)
        await wait { model.textInputState?.focused?.node_id == node.id && !model.textInputBusy }
        view.image = model.image; view.refreshTextInput()
    }

    func testAppKitCompositionReplacesSelectionAndCancelsThroughRealCore() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        try await focus("Editor", model: model, view: view)
        let window = NSWindow(contentRect: CGRect(x: 200, y: 200, width: 500, height: 300), styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = view
        model.textInput(.select(TextRange(NSRange(location: 1, length: 2))))
        await wait { model.textInputState?.focused?.selection.nsRange == NSRange(location: 1, length: 2) && !model.textInputBusy }
        let unknown = NSRange(location: NSNotFound, length: 0)
        view.setMarkedText("中", selectedRange: NSRange(location: 1, length: 0), replacementRange: unknown)
        XCTAssertEqual(view.markedRange(), NSRange(location: 1, length: 1))
        view.setMarkedText("中文中文中文", selectedRange: NSRange(location: 6, length: 0), replacementRange: unknown)
        XCTAssertGreaterThan(view.firstRect(forCharacterRange: view.selectedRange(), actualRange: nil).height, 0,
                             "Pending IME text must keep a candidate anchor at the core caret")
        view.setMarkedText("中文", selectedRange: NSRange(location: 2, length: 0), replacementRange: unknown)
        XCTAssertEqual(view.selectedRange(), NSRange(location: 3, length: 0))
        await wait { model.textInputState?.focused?.text == "A中文B" && !model.textInputBusy }
        XCTAssertEqual(view.markedRange(), NSRange(location: 1, length: 2))
        view.doCommand(by: NSSelectorFromString("cancelOperation:"))
        await wait { model.textInputState?.focused?.text == "A😀B" && !model.textInputBusy }
        XCTAssertFalse(view.hasMarkedText())
        XCTAssertEqual(view.selectedRange(), NSRange(location: 1, length: 2))
        view.setMarkedText("שלום", selectedRange: NSRange(location: 4, length: 0), replacementRange: unknown)
        view.insertText("中文", replacementRange: unknown)
        await wait { model.textInputState?.focused?.text == "A中文B" && !model.textInputBusy }
        XCTAssertFalse(view.hasMarkedText())
        view.setMarkedText("abc", selectedRange: NSRange(location: 3, length: 0), replacementRange: unknown)
        await wait { model.textInputState?.focused?.marked != nil && !model.textInputBusy }
        view.unmarkText()
        XCTAssertFalse(view.hasMarkedText(), "unmarkText must immediately end the AppKit marked range")
        await wait { model.textInputState?.focused?.marked == nil && !model.textInputBusy }
        await model.stop()
    }

    func testMultilineCaretScreenGeometrySubstringAndGraphemeDeletion() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        let window = NSWindow(contentRect: CGRect(x: 200, y: 200, width: 500, height: 300), styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = view
        try await focus("Notes", model: model, view: view)
        view.selectAll(nil)
        view.insertText("中文\nשלום\n👩‍👩‍👧‍👦", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.textInputState?.focused?.text == "中文\nשלום\n👩‍👩‍👧‍👦" && !model.textInputBusy }
        view.image = model.image
        var actual = NSRange(location: NSNotFound, length: 0)
        let first = view.firstRect(forCharacterRange: NSRange(location: 0, length: 0), actualRange: &actual)
        let last = view.firstRect(forCharacterRange: view.selectedRange(), actualRange: &actual)
        XCTAssertGreaterThan(first.minY, last.minY, "Flipped view must place a later line lower on screen")
        XCTAssertGreaterThan(last.height, 0)
        XCTAssertEqual(view.characterIndex(for: NSPoint(x: first.midX, y: first.midY)), 0)
        XCTAssertEqual(view.attributedSubstring(forProposedRange: NSRange(location: 0, length: 2), actualRange: &actual)?.string, "中文")
        view.doCommand(by: NSSelectorFromString("deleteBackward:"))
        await wait { model.textInputState?.focused?.text == "中文\nשלום\n" && !model.textInputBusy }
        await model.stop()
    }

    func testPasswordPrivacyReadonlyAndDocumentFencesWithRealServices() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        try await focus("Secret", model: model, view: view)
        view.selectAll(nil); view.insertText("new-secret", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.textInputState?.focused?.text_length == 10 && !model.textInputBusy }
        XCTAssertNil(model.textInputState?.focused?.text)
        XCTAssertNil(view.attributedSubstring(forProposedRange: NSRange(location: 0, length: 2), actualRange: nil))
        await wait { model.representation?.nodes.first(where: { $0.name == "Secret" })?.state.focused == true }
        XCTAssertNil(model.representation?.nodes.first(where: { $0.name == "Secret" })?.state.value)
        try await focus("Readonly", model: model, view: view)
        view.insertText("bad", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { !model.textInputBusy }
        XCTAssertEqual(model.textInputState?.focused?.text, "locked")
        try await focus("Editor", model: model, view: view)
        let old = try XCTUnwrap(model.textInputState?.context)
        let oldGeneration = model.generation
        model.reload()
        await wait { model.generation > oldGeneration && model.textInputState?.focused == nil }
        try await focus("Editor", model: model, view: view)
        model.action(.textInput(old, .replace("stale", nil)))
        await wait { model.status.contains("Stale") }
        XCTAssertEqual(model.textInputState?.focused?.text, "A😀B")
        await model.stop()
    }

    func testQueuedKeysResolveTextAndControlDefaultsAfterFocusAcknowledgements() async throws {
        let (model, view, fixture) = try await start(path: "/keyboard")
        defer { fixture.stop(); Task { await model.stop() } }
        func key(_ code: UInt16, _ characters: String) throws {
            view.keyDown(with: try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                timestamp: 0, windowNumber: 0, context: nil, characters: characters,
                charactersIgnoringModifiers: characters, isARepeat: false, keyCode: code)))
        }
        // Issue callbacks without awaiting a state refresh between Tab/Space.
        try key(48, "\t"); try key(49, " ")
        try key(48, "\t"); try key(48, "\t"); try key(49, " ")
        try key(48, "\t"); try key(124, "")
        try key(48, "\t"); try key(125, "")
        try key(48, "\t"); try key(124, "")
        await wait { model.textInputState?.focused_node == model.representation?.nodes.first(where: { $0.name == "Level" })?.id && !model.textInputBusy }
        await wait { model.representation?.nodes.first(where: { $0.name == "Name" })?.state.value == " " }
        let nodes = try XCTUnwrap(model.representation).nodes
        XCTAssertEqual(nodes.first(where: { $0.name == "Readonly" })?.state.value, "locked")
        XCTAssertEqual(nodes.first(where: { $0.name == "Remember" })?.state.checked, true)
        XCTAssertEqual(nodes.first(where: { $0.name == "Express" })?.state.checked, true)
        XCTAssertEqual(nodes.first(where: { $0.name == "Region" })?.state.value, "b")
        XCTAssertEqual(nodes.first(where: { $0.name == "Level" })?.state.value, "0.5")
        await model.stop()
    }

    func testInputSourceMenuTracksNativeResponderAndClearsOnFocusLoss() throws {
        let view = CorePageView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        let menu = NativeEditingMenu()
        let context = try XCTUnwrap(view.inputContext)
        let previous = context.selectedKeyboardInputSource
        defer { context.selectedKeyboardInputSource = previous }
        menu.focus(view)
        XCTAssertFalse(menu.sources.isEmpty)
        if let source = menu.sources.first { menu.choose(source.id); XCTAssertEqual(context.selectedKeyboardInputSource, source.id) }
        menu.focus(nil)
        XCTAssertFalse(menu.sources.isEmpty, "System keyboard sources also remain available for the native address field")
        XCTAssertNil(menu.selected)
    }

    func testNativeFormResetRestoresMarkedTextAndInvalidatesOldEditsWithRealServices() async throws {
        let (model, view, fixture) = try await start(path: "/reset")
        defer { fixture.stop(); Task { await model.stop() } }
        try await focus("Readonly", model: model, view: view)
        view.insertText("bad", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.status.contains("read-only") && !model.textInputBusy }
        try await focus("Name", model: model, view: view)
        view.selectAll(nil); view.insertText("edited", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.textInputState?.focused?.text == "edited" && !model.textInputBusy }
        try await focus("Notes", model: model, view: view)
        view.selectAll(nil)
        view.setMarkedText("中文", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.textInputState?.focused?.marked != nil && !model.textInputBusy }
        for _ in 0..<6 { model.textInput(.key(.tab, false)) }
        await wait { model.textInputState?.focused_node == model.representation?.nodes.first(where: { $0.name == "Reset form" })?.id && !model.textInputBusy }
        let context = try XCTUnwrap(model.textInputState?.context)
        let generation = model.generation
        model.textInput(.key(.enter, false))
        await wait { model.generation > generation && model.textInputState?.focus_generation != context.focus_generation && !model.textInputBusy }
        XCTAssertEqual(model.textInputState?.document_generation, context.document_generation)
        await wait { model.representation?.nodes.first(where: { $0.name == "Name" })?.state.value == "A😀B" }
        XCTAssertEqual(model.representation?.nodes.first(where: { $0.name == "Notes" })?.state.value, "first\nsecond")
        XCTAssertEqual(model.status, "Ready", "A successful reset must not report a previous native editing failure")
        model.action(.textInput(context, .key(.tab, false)))
        await wait { model.status.contains("Stale") }
        try await focus("Notes", model: model, view: view)
        view.refreshTextInput()
        XCTAssertFalse(view.hasMarkedText())
        XCTAssertEqual(model.textInputState?.focused?.text, "first\nsecond")
        view.doCommand(by: NSSelectorFromString("cancelOperation:"))
        await wait { !model.textInputBusy }
        XCTAssertEqual(model.textInputState?.focused?.text, "first\nsecond")
        XCTAssertEqual(fixture.requests, ["/reset"], "Reset is a local default action and must not fetch")
        try await focus("Readonly", model: model, view: view)
        view.insertText("bad", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.status.contains("read-only") && !model.textInputBusy }
        model.address = fixture.origin + "/blocked"; model.navigateAddress()
        await wait { model.status.contains("Navigation blocked") }
        let snapshot = try XCTUnwrap(model.representation)
        let reset = try XCTUnwrap(snapshot.nodes.first { $0.name == "Reset form" })
        let beforeReset = model.generation
        XCTAssertTrue(model.accessibilityAction(snapshot, epoch: model.accessibilityEpoch, node: reset))
        await wait { model.generation > beforeReset && !model.textInputBusy }
        XCTAssertTrue(model.status.contains("Navigation blocked"), "Input recovery must preserve the mandatory policy denial")
        XCTAssertEqual(fixture.requests, ["/reset", "/blocked"])
        await model.stop()
    }

    func testNativeGetPostRedirectValidationAndResubmissionThroughRealServices() async throws {
        let (model, view, fixture) = try await start(path: "/forms")
        defer { fixture.stop(); Task { await model.stop() } }
        func activate(_ name: String) async throws {
            await wait { model.representation != nil && !model.textInputBusy }
            let snapshot = try XCTUnwrap(model.representation)
            let node = try XCTUnwrap(snapshot.nodes.first { $0.name == name })
            XCTAssertTrue(model.accessibilityAction(snapshot, epoch: model.accessibilityEpoch, node: node))
        }
        try await focus("Query", model: model, view: view)
        model.textInput(.select(TextRange(NSRange(location: 0, length: 5))))
        await wait { !model.textInputBusy }
        model.textInput(.replace("", nil))
        await wait { model.textInputState?.focused?.text == "" && !model.textInputBusy }
        try await activate("Send POST")
        await wait { model.status.contains("required") }
        XCTAssertEqual(fixture.requests, ["/forms"])
        try await focus("Query", model: model, view: view)
        view.insertText("A & 冰", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.textInputState?.focused?.text == "A & 冰" && !model.textInputBusy }
        try await activate("Send POST")
        await wait { model.representation?.url == fixture.origin + "/posted?kept=1" }
        let post = try XCTUnwrap(fixture.records.last)
        XCTAssertEqual(post.method, "POST")
        XCTAssertEqual(String(data: post.body, encoding: .utf8), "q=A+%26+%E5%86%B0&accepted=on&region=a&notes=one%0D%0Atwo&level=50&mode=post")
        model.reload(); await wait { model.resubmission != nil }
        model.resolveResubmission(try XCTUnwrap(model.resubmission).confirmationID, accept: false)
        await wait { model.status == "Ready" }
        XCTAssertEqual(fixture.requests.count, 2)
        model.reload(); await wait { model.resubmission != nil }
        model.resolveResubmission(try XCTUnwrap(model.resubmission).confirmationID, accept: true)
        await wait { fixture.requests.count == 3 && model.status == "Ready" }
        XCTAssertEqual(fixture.records.last?.body, post.body)
        model.action(.unit("GoBack")); await wait { model.representation?.url == fixture.origin + "/forms" }
        try await activate("Send GET")
        await wait { model.representation?.url?.contains("/received?q=") == true }
        XCTAssertEqual(fixture.records.last?.method, "GET")
        XCTAssertTrue(fixture.records.last?.body.isEmpty == true)
        model.address = fixture.origin + "/forms"; model.navigateAddress()
        await wait { model.representation?.url == fixture.origin + "/forms" }
        try await activate("Send redirect")
        await wait { model.representation?.url == fixture.origin + "/received" }
        XCTAssertEqual(fixture.records.suffix(2).map(\.method), ["POST", "GET"])
        model.reload(); await wait { fixture.requests.last == "/received" && model.status == "Ready" }
        XCTAssertNil(model.resubmission)
        model.address = fixture.origin + "/protected-form"; model.navigateAddress()
        await wait { model.representation?.url == fixture.origin + "/protected-form" }
        let before = fixture.requests.count
        try await activate("Send protected")
        await wait { model.status.contains("Navigation blocked") }
        XCTAssertEqual(fixture.requests.count, before)
        XCTAssertFalse(model.status.contains("private-fixture-secret"))
        await model.stop()
    }

}
