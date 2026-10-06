// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine
import SwiftUI
import XCTest

@MainActor
final class NativeEditingTests: XCTestCase {
    func testChromeFindPreservesNativeMarkedTextAcrossRepaintAndCommitsOnce() async throws {
        let (model, _, fixture) = try await start(path: "/editing")
        defer { fixture.stop(); Task { await model.stop() } }
        let window = BrowserWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 700),
                                   styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = NSHostingView(rootView: BrowserView(model: model).environmentObject(NativeEditingMenu()))
        defer { window.contentView = nil; window.close() }
        model.showFind()
        await wait { window.chromeFocus?.findField != nil }
        let field = try XCTUnwrap(window.chromeFocus?.findField)
        XCTAssertTrue(window.makeFirstResponder(field))
        let editor = try XCTUnwrap(field.currentEditor() as? NSTextView)
        editor.setMarkedText("ㄓ", selectedRange: NSRange(location: 1, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertTrue(editor.hasMarkedText()); XCTAssertEqual(model.findQuery, "")
        model.setZoom(1.25); model.setFindCaseSensitive(true)
        await wait { model.zoomPercent == 125 && model.findResult != nil }
        XCTAssertTrue(editor.hasMarkedText()); XCTAssertEqual(editor.string, "ㄓ")
        XCTAssertEqual(model.findQuery, "", "An AppKit marked sequence must not be published as a committed find query")
        editor.insertText("中", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.findQuery == "中" }
        XCTAssertFalse(editor.hasMarkedText()); XCTAssertEqual(model.findQuery, "中")
    }

    func testChromeFocusTargetsFollowRenderedAvailabilityAndCollapsedGroups() async throws {
        let (model, _, fixture) = try await start(path: "/editing")
        defer { fixture.stop(); Task { await model.stop() } }
        let window = BrowserWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 700),
                                   styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = NSHostingView(rootView: BrowserView(model: model).environmentObject(NativeEditingMenu()))
        window.makeKeyAndOrderFront(nil)
        defer { window.contentView = nil; window.close() }
        await wait { window.chromeFocus?.targets.contains("go") == true }
        let navigation = try XCTUnwrap(window.chromeFocus)
        XCTAssertTrue(navigation.targets.contains("page")); XCTAssertFalse(navigation.targets.contains("forward"))
        XCTAssertFalse(navigation.targets.contains("find-query")); XCTAssertFalse(navigation.targets.contains("profile-menu"))
        model.showFind(); model.setFindQuery("no-such-visible-match")
        await wait { model.findResult?.matchCount == 0 && navigation.targets.contains("find-close") }
        XCTAssertTrue(navigation.targets.contains("find-query")); XCTAssertTrue(navigation.targets.contains("find-case"))
        XCTAssertFalse(navigation.targets.contains("find-previous")); XCTAssertFalse(navigation.targets.contains("find-next"))
        model.closeFind(); await wait { !navigation.targets.contains("find-query") }
        let tab = try XCTUnwrap(model.selected)
        let created = await model.createTabGroup(name: "Keyboard group", color: "#4477cc", tab: tab)
        let group = try XCTUnwrap(created)
        let collapsed = await model.setTabGroupCollapsed(group, collapsed: true)
        XCTAssertTrue(collapsed)
        await wait { navigation.targets.contains("tab-group-\(group)") && !navigation.targets.contains("tab-\(tab)") }
        XCTAssertFalse(navigation.targets.contains("close-tab-\(tab)"))
        let expanded = await model.setTabGroupCollapsed(group, collapsed: false)
        XCTAssertTrue(expanded)
        await wait { navigation.targets.contains("tab-\(tab)") }
        XCTAssertTrue(navigation.targets.contains("close-tab-\(tab)"))
        model.toggleAssistant()
        await wait { navigation.targets.contains("assistant-instruction") }
        model.toggleAssistant()
        await wait { !navigation.targets.contains(where: { $0.hasPrefix("assistant-") }) }
        XCTAssertTrue(navigation.targets.contains("page-zoom"))
        await model.stop(); await wait { navigation.targets.isEmpty }
    }

    func testDocumentTextWireSupportsLongReadonlyTextAndKeepsControlBounds() throws {
        let text = String(repeating: "a", count: 70_000)
        var state: [String: Any] = ["document": true, "text": text, "text_length": text.utf16.count, "protected": false, "writable": false,
            "multiline": false, "focused": false, "selection": NSNull(), "marked": NSNull(), "visible_range": ["location": 0, "length": text.utf16.count],
            "insertion_line": NSNull(), "line_count": 1, "style": ["font_size_px": 16, "bold": false, "italic": false, "color": [0, 0, 0, 255]]]
        func decode(_ value: [String: Any]) throws -> AccessibilityTextState {
            try JSONDecoder().decode(AccessibilityTextState.self, from: JSONSerialization.data(withJSONObject: value))
        }
        XCTAssertTrue(try decode(state).valid)
        state["document"] = false; XCTAssertFalse(try decode(state).valid)
        state["document"] = true; state["writable"] = true; XCTAssertFalse(try decode(state).valid)
        state["writable"] = false; state["protected"] = true; XCTAssertFalse(try decode(state).valid)
    }

    func testActualDocumentTextAXSelectionCopyAndReadonlyEditorIsolation() async throws {
        let (model, view, fixture) = try await start(path: "/document-selection")
        defer { fixture.stop(); Task { await model.stop() } }
        let saved = NSPasteboard.general.pasteboardItems?.map { item -> NSPasteboardItem in
            let copy = NSPasteboardItem(); for type in item.types { if let data = item.data(forType: type) { copy.setData(data, forType: type) } }; return copy
        } ?? []
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        let window = NSWindow(contentRect: view.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.contentView = view; window.makeFirstResponder(view)
        defer { window.contentView = nil; window.close() }
        func refresh() async {
            // Attaching an NSView can queue a debounced backing-scale resize.
            // Wait for its actual core acknowledgement before an exact-frame
            // synchronous AX read, not just an earlier locally current frame.
            await wait { model.representation?.generation == model.generation
                && model.textInputState?.frame_generation == model.generation && !model.textInputBusy
                && model.cssViewportSize != nil && model.displayState?.width == view.bounds.width
                && model.displayState?.height == view.bounds.height
                && model.displayState?.backingScale == view.convertToBacking(view.bounds).width / view.bounds.width }
            view.accessibilityTree.update(model.representation, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero)
        }
        await refresh()
        let paragraph = try XCTUnwrap(view.accessibilityTree.elements.values.first { $0.accessibilityLabel() == "Alpha bold 😀 é" })
        XCTAssertEqual(paragraph.accessibilityNumberOfCharacters(), "Alpha bold 😀 é".utf16.count)
        XCTAssertEqual(paragraph.accessibilityString(for: NSRange(location: 11, length: 2)), "😀")
        XCTAssertEqual(paragraph.accessibilityRange(for: 12), NSRange(location: 11, length: 2))
        try await focus("Document editor", model: model, view: view); await refresh()
        let editor = try XCTUnwrap(model.textInputState?.focused)
        let oldFrame = model.generation
        paragraph.setAccessibilitySelectedTextRange(NSRange(location: 11, length: 2))
        await wait { model.generation > oldFrame }; await refresh()
        await wait { model.textInputState?.document?.selected_text == "😀" }; await refresh()
        XCTAssertEqual(paragraph.accessibilitySelectedText(), "😀")
        XCTAssertEqual(model.textInputState?.focused?.node_id, editor.node_id)
        XCTAssertEqual(model.textInputState?.focused?.selection, editor.selection)
        XCTAssertEqual(model.textInputState?.focused?.text, "public-control-secret")
        let status = model.status
        view.insertText("blocked", replacementRange: NSRange(location: NSNotFound, length: 0))
        view.setMarkedText("blocked", selectedRange: NSRange(location: 0, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        view.unmarkText(); view.cut(nil); view.paste(nil)
        await wait { !model.textInputBusy }; await refresh()
        XCTAssertEqual(model.status, status)
        XCTAssertEqual(model.textInputState?.focused?.text, "public-control-secret")
        XCTAssertFalse(paragraph.isAccessibilitySelectorAllowed(#selector(PageAccessibilityElement.setAccessibilityValue(_:))))
        XCTAssertGreaterThan(paragraph.accessibilityFrame(for: NSRange(location: 11, length: 2)).width, 0)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("document-copy-sentinel", forType: .string)
        view.copy(nil); await wait { !model.textInputBusy && NSPasteboard.general.string(forType: .string) == "😀" }
        let snapshot = try XCTUnwrap(model.representation), epoch = model.accessibilityEpoch
        model.action(.values("OpenTab", ["url": .null])); await wait { model.selected != snapshot.tabID && model.representation != nil }
        await refresh(); XCTAssertNil(paragraph.accessibilitySelectedText())
        model.select(snapshot.tabID); await wait { model.representation?.url == fixture.origin + "/document-selection" }; await refresh()
        XCTAssertNil(model.accessibilityText(snapshot, epoch: epoch, node: try XCTUnwrap(snapshot.nodes.first { $0.name == "Alpha bold 😀 é" }), action: .select(TextRange(NSRange(location: 0, length: 1)))))
        let retained = try XCTUnwrap(view.accessibilityTree.elements.values.first { $0.accessibilityLabel() == "Alpha bold 😀 é" })
        XCTAssertEqual(retained.accessibilitySelectedText(), "😀")
        let oldDocument = model.textInputState?.document_generation
        model.reload(); await wait { fixture.requests.count == 2 && model.textInputState?.document_generation != oldDocument
            && model.representation?.url == fixture.origin + "/document-selection" }; await refresh()
        XCTAssertNil(retained.accessibilitySelectedText())
        XCTAssertEqual(fixture.requests, ["/document-selection", "/document-selection"])
        await model.stop()
    }

    func testAccessibilityTextWireRejectsPasswordLeaksAndMalformedRanges() throws {
        let context: [String: Any] = ["version": 1, "frame_source": 9, "document_generation": 3, "frame_generation": 8, "node_id": 7]
        let range: [String: Any] = ["location": 0, "length": 4]
        let state: [String: Any] = ["text": "safe", "text_length": 4, "protected": false, "writable": true,
            "multiline": false, "focused": false, "selection": NSNull(), "marked": NSNull(), "visible_range": range,
            "insertion_line": NSNull(), "line_count": 1, "style": ["font_size_px": 16, "bold": false, "italic": false, "color": [0, 0, 0, 255]]]
        func decode(_ context: [String: Any], _ result: [String: Any]) throws -> BrowserMessage {
            let data = try JSONSerialization.data(withJSONObject: ["message": ["AccessibilityTextState": ["context": context, "result": result]]])
            return try JSONDecoder().decode(IncomingEnvelope.self, from: data).message
        }
        if case .accessibilityTextState = try decode(context, ["State": state]) {} else { XCTFail("Valid state must decode") }
        for (key, value) in [("protected", true as Any), ("text_length", 65_537 as Any), ("visible_range", ["location": 4, "length": 1] as Any),
            ("selection", range as Any), ("line_count", 0 as Any)] {
            var invalid = state; invalid[key] = value
            if case .unknown = try decode(context, ["State": invalid]) {} else { XCTFail("Malformed state must fail soft") }
        }
        var unsupported = context; unsupported["version"] = 2
        if case .unknown = try decode(unsupported, ["State": state]) {} else { XCTFail("Unsupported version must fail soft") }
        if case .unknown = try decode(context, ["Range": ["location": UInt32.max, "length": 1]]) {} else { XCTFail("Overflow range must fail soft") }
        let command = BrowserCommand.accessibilityText(AccessibilityTextContext(version: 1, frame_source: 9, document_generation: 3, frame_generation: 8, node_id: 7), .select(TextRange(NSRange(location: 1, length: 2))))
        let data = try BrowserWire.encode(command, tab: 2, request: 7)
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: data.dropFirst(4)) as? [String: Any])
        let body = try XCTUnwrap((root["message"] as? [String: Any])?["AccessibilityText"] as? [String: Any])
        XCTAssertNotNil((body["action"] as? [String: Any])?["Select"])
    }

    func testAccessibilityLongRangeScrollUsesCoreGeometryAndPreservesFocus() async throws {
        let (model, view, fixture) = try await start(path: "/accessibility-text")
        defer { fixture.stop(); Task { await model.stop() } }
        let window = NSWindow(contentRect: view.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        defer { window.contentView = nil; window.close() }
        func refresh() async {
            await wait { model.representation?.generation == model.generation && model.textInputState?.frame_generation == model.generation
                && model.displayState?.width == view.bounds.width && model.displayState?.height == view.bounds.height && !model.textInputBusy }
            view.accessibilityTree.update(model.representation, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero)
        }
        await refresh()
        let snapshot = try XCTUnwrap(model.representation)
        func element(_ name: String) throws -> PageAccessibilityElement {
            try XCTUnwrap(view.accessibilityTree.elements[try XCTUnwrap(snapshot.nodes.first { $0.name == name }).id])
        }
        let long = try element("Long editor")
        XCTAssertEqual(long.accessibilityNumberOfCharacters(), 1503)
        XCTAssertEqual(long.accessibilityRange(for: 1502), NSRange(location: 1501, length: 2))
        let range = NSRange(location: 1500, length: 3)
        XCTAssertEqual(long.accessibilityFrame(for: range), .zero)
        long.setAccessibilityVisibleCharacterRange(range)
        let generation = model.generation
        await wait { model.generation > generation }
        await refresh()
        XCTAssertNil(model.textInputState?.focused)
        XCTAssertGreaterThan(long.accessibilityVisibleCharacterRange().location, 1000)
        let rect = long.accessibilityFrame(for: range)
        XCTAssertGreaterThan(rect.width, 0)
        XCTAssertTrue(long.accessibilityFrame().contains(rect))
        XCTAssertEqual(long.accessibilityString(for: range), "尾😀")
        let notes = try element("Scrollable notes")
        notes.setAccessibilitySelectedTextRanges([NSValue(range: NSRange(location: 180, length: 3))])
        await wait { model.textInputState?.focused?.selection.nsRange == NSRange(location: 180, length: 3) }
        await refresh()
        XCTAssertEqual(notes.accessibilitySelectedText(), "row")
        XCTAssertGreaterThan(notes.accessibilityVisibleCharacterRange().location, 0)
        XCTAssertTrue(notes.accessibilityFrame().contains(notes.accessibilityFrame(for: NSRange(location: 180, length: 3))))
        let old = try XCTUnwrap(model.representation)
        model.reload()
        await wait { model.representation?.generation != old.generation && model.representation?.url == fixture.origin + "/accessibility-text" }
        long.setAccessibilityValue("stale")
        XCTAssertEqual(model.representation?.nodes.first { $0.name == "Long editor" }?.state.value, String(repeating: "x", count: 1500) + "尾😀")
        await model.stop()
    }

    func testAccessibilityTextParametersAndWritesThroughRealCore() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        let window = NSWindow(contentRect: view.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        defer { window.contentView = nil; window.close() }
        func refresh() async {
            await wait { model.representation?.generation == model.generation && model.textInputState?.frame_generation == model.generation
                && model.displayState?.width == view.bounds.width && model.displayState?.height == view.bounds.height && !model.textInputBusy }
            view.accessibilityTree.update(model.representation, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero)
        }
        await refresh()
        let snapshot = try XCTUnwrap(model.representation)
        func element(_ name: String) throws -> PageAccessibilityElement {
            try XCTUnwrap(view.accessibilityTree.elements[try XCTUnwrap(snapshot.nodes.first { $0.name == name }).id])
        }
        let editor = try element("Editor")
        XCTAssertEqual(editor.accessibilityNumberOfCharacters(), 4)
        XCTAssertEqual(editor.accessibilityString(for: NSRange(location: 1, length: 2)), "😀")
        XCTAssertNil(editor.accessibilityString(for: NSRange(location: 2, length: 1)))
        XCTAssertEqual(editor.accessibilityRange(for: 2), NSRange(location: 1, length: 2))
        XCTAssertEqual(editor.accessibilityRange(for: 4).location, NSNotFound)
        XCTAssertNil(model.textInputState?.focused, "Accessibility reads must preserve focus")
        editor.setAccessibilitySelectedTextRange(NSRange(location: 1, length: 2))
        await wait { model.textInputState?.focused?.selection.nsRange == NSRange(location: 1, length: 2) }
        await refresh()
        XCTAssertEqual(editor.accessibilitySelectedText(), "😀")
        let rect = editor.accessibilityFrame(for: NSRange(location: 1, length: 2))
        XCTAssertGreaterThan(rect.width, 0)
        XCTAssertTrue(editor.accessibilityFrame().intersects(rect))
        XCTAssertEqual(editor.accessibilityRange(for: NSPoint(x: rect.minX + 0.1, y: rect.midY)), NSRange(location: 1, length: 2))
        editor.setAccessibilitySelectedText("中文")
        await wait { model.textInputState?.focused?.text == "A中文B" }
        await refresh()
        model.textInput(.undo)
        await wait { model.textInputState?.focused?.text == "A😀B" && !model.textInputBusy }
        await refresh()
        let notes = try element("Notes")
        XCTAssertEqual(notes.accessibilityRange(forLine: 0), NSRange(location: 0, length: 6))
        XCTAssertEqual(notes.accessibilityLine(for: 7), 1)
        let readonly = try element("Readonly")
        XCTAssertFalse(readonly.isAccessibilitySelectorAllowed(#selector(NSAccessibilityElement.setAccessibilityValue(_:))))
        readonly.setAccessibilityValue("bad")
        XCTAssertEqual(readonly.accessibilityValue() as? String, "locked")
        let secret = try element("Secret")
        XCTAssertNil(secret.accessibilityValue())
        XCTAssertNil(secret.accessibilityString(for: NSRange(location: 0, length: 2)))
        XCTAssertNil(secret.accessibilityAttributedString(for: NSRange(location: 0, length: 2)))
        XCTAssertNil(secret.accessibilityRTF(for: NSRange(location: 0, length: 2)))
        secret.setAccessibilityValue("new-private-secret")
        await wait { model.textInputState?.focused?.protected == true && model.textInputState?.focused?.text_length == 18 }
        XCTAssertNil(model.textInputState?.focused?.text)
        await refresh()
        view.accessibilityTree.update(nil, epoch: model.accessibilityEpoch, imageSize: .zero)
        editor.setAccessibilityValue("stale")
        XCTAssertNil(editor.accessibilityString(for: NSRange(location: 0, length: 1)))
        await model.stop()
    }

    func testNativeSelectAccessibilitySelectionWritesScrollAndInvalidatedElements() async throws {
        let (model, view, fixture) = try await start(path: "/select")
        defer { fixture.stop(); Task { await model.stop() } }
        model.textInput(.key(.tab, false)); model.textInput(.key(.tab, false))
        await wait { model.textInputState?.select?.multiple == true && !model.textInputBusy && model.representation?.generation == model.generation }
        let snapshot = try XCTUnwrap(model.representation)
        view.accessibilityTree.update(snapshot, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero)
        let list = try XCTUnwrap(view.accessibilityTree.elements[try XCTUnwrap(model.textInputState?.select?.node_id)])
        list.setAccessibilityFocused(true)
        await wait { model.textInputState?.select?.multiple == true && !model.textInputBusy }
        XCTAssertEqual(model.textInputState?.select?.options.filter(\.selected).map(\.label), ["Topic Alpha", "Topic Gamma"], "AX focus must preserve selections")
        await wait { model.representation?.generation == model.generation }
        view.accessibilityTree.update(model.representation, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero)
        let betaNode = try XCTUnwrap(snapshot.nodes.first { $0.name == "Topic Beta" })
        let beta = try XCTUnwrap(view.accessibilityTree.elements[betaNode.id])
        XCTAssertTrue(beta.isAccessibilitySelectorAllowed(#selector(NSAccessibilityElement.setAccessibilitySelected(_:))))
        beta.setAccessibilitySelected(true)
        await wait { model.textInputState?.select?.options[1].selected == true && !model.textInputBusy && model.representation?.generation == model.generation }
        view.accessibilityTree.update(model.representation, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero)
        beta.setAccessibilitySelected(false)
        await wait { model.textInputState?.select?.options[1].selected == false && !model.textInputBusy && model.representation?.generation == model.generation }
        let current = try XCTUnwrap(model.representation)
        view.accessibilityTree.update(current, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero)
        let locked = try XCTUnwrap(view.accessibilityTree.elements[try XCTUnwrap(current.nodes.first { $0.name == "Topic Locked" }).id])
        XCTAssertFalse(locked.isAccessibilitySelectorAllowed(#selector(NSAccessibilityElement.setAccessibilitySelected(_:))))
        locked.setAccessibilitySelected(true)
        XCTAssertFalse(model.textInputState?.select?.options[2].selected ?? true)
        let bounds = try XCTUnwrap(model.textInputState?.select?.bounds)
        model.textInput(.selectScroll(bounds.x + 4, bounds.y - current.scrollY + 4, 3))
        await wait { (model.representation?.nodes.first { $0.name == "Topic Emoji 😀" }?.bounds.height ?? 0) > 0 && !model.textInputBusy }
        XCTAssertEqual(model.textInputState?.select?.options.filter(\.selected).map(\.label), ["Topic Alpha", "Topic Gamma"])
        view.accessibilityTree.update(nil, epoch: model.accessibilityEpoch, imageSize: .zero)
        beta.setAccessibilitySelected(true); XCTAssertFalse(model.textInputBusy)
        XCTAssertFalse(model.textInputState?.select?.options[1].selected ?? true)
        await model.stop()
    }

    func testNativeSelectStateRejectsMalformedOwnershipChoicesAndChecksActionEncoding() throws {
        let bounds: [String: Any] = ["x": 0, "y": 0, "width": 120, "height": 24]
        let option: [String: Any] = ["node_id": 6, "label": "中文", "group": NSNull(), "selected": true, "disabled": false]
        let select: [String: Any] = ["node_id": 5, "multiple": false, "popup": true, "limited": false,
                                   "bounds": bounds, "active_option": 6, "options": [option]]
        func decode(_ select: [String: Any]) throws -> BrowserMessage {
            let state: [String: Any] = ["version": 1, "frame_source": 19, "document_generation": 7,
                "focus_generation": 3, "frame_generation": 8, "tab_id": 2, "scroll_y": 0,
                "focused_node": 5, "focused": NSNull(), "focus_exit": NSNull(), "select": select]
            let data = try JSONSerialization.data(withJSONObject: ["message": ["TextInputState": state]])
            return try JSONDecoder().decode(IncomingEnvelope.self, from: data).message
        }
        if case .textInputState = try decode(select) {} else { XCTFail("Valid select must decode") }
        for (key, value) in [("node_id", 9 as Any), ("multiple", true as Any), ("options", [option, option] as Any), ("active_option", 99 as Any)] {
            var invalid = select; invalid[key] = value
            if case .textInputUnavailable = try decode(invalid) {} else { XCTFail("Invalid select must fail soft") }
        }
        let context = TextInputContext(version: 1, frame_source: 19, document_generation: 7, focus_generation: 3)
        for (action, name) in [(TextInputAction.selectOption(6, 8, true, false), "SelectOption"),
            (.selectPointer(1, 2, false, true), "SelectPointer"), (.selectKey(.pageDown, true, false), "SelectKey"), (.selectScroll(1, 2, 3), "SelectScroll")] {
            let bytes = try BrowserWire.encode(.textInput(context, action), tab: 2, request: 7)
            let root = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes.dropFirst(4)) as? [String: Any])
            let input = try XCTUnwrap((root["message"] as? [String: Any])?["TextInput"] as? [String: Any])
            XCTAssertNotNil((input["action"] as? [String: Any])?[name])
        }
    }

    func testNativeSelectMenuLabelsDisabledGroupsTypeaheadAndStaleItemsThroughRealCore() async throws {
        let (model, view, fixture) = try await start(path: "/select")
        defer { fixture.stop(); Task { await model.stop() } }
        model.textInput(.key(.tab, false))
        await wait { model.textInputState?.select?.popup == true && !model.textInputBusy }
        let state = try XCTUnwrap(model.textInputState)
        let menu = view.makeSelectMenu(state)
        XCTAssertFalse(try XCTUnwrap(menu.items.first { $0.title == "Locked choice" }).isEnabled)
        XCTAssertFalse(try XCTUnwrap(menu.items.first { $0.title == "Unavailable" }).isEnabled)
        let beta = try XCTUnwrap(menu.items.first { $0.title == "Beta" })
        XCTAssertEqual(beta.indentationLevel, 1)
        XCTAssertTrue(NSApp.sendAction(try XCTUnwrap(beta.action), to: beta.target, from: beta))
        await wait { model.textInputState?.select?.options.first { $0.selected }?.label == "Beta" && !model.textInputBusy }
        let alpha = try XCTUnwrap(menu.items.first { $0.title == "Alpha" })
        XCTAssertTrue(NSApp.sendAction(try XCTUnwrap(alpha.action), to: alpha.target, from: alpha))
        XCTAssertEqual(model.textInputState?.select?.options.first { $0.selected }?.label, "Beta")
        view.insertText("b", replacementRange: NSRange(location: NSNotFound, length: 0))
        view.insertText("r", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.textInputState?.select?.options.first { $0.selected }?.label == "Bravo" && !model.textInputBusy }
        model.textInput(.key(.tab, false)); model.textInput(.key(.tab, true))
        await wait { model.textInputState?.select?.popup == true && !model.textInputBusy }
        view.insertText("中文", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { model.textInputState?.select?.options.first { $0.selected }?.label == "中文" && !model.textInputBusy }
        XCTAssertNil(model.textInputState?.focused)
        await model.stop()
    }

    func testNativeMultipleSelectionCommandMovementRangeScrollResetAndPostSubmission() async throws {
        let (model, view, fixture) = try await start(path: "/select")
        defer { fixture.stop(); Task { await model.stop() } }
        model.textInput(.key(.tab, false)); model.textInput(.key(.tab, false))
        await wait { model.textInputState?.select?.multiple == true && !model.textInputBusy }
        func selected() -> [String] { model.textInputState?.select?.options.filter(\.selected).map(\.label) ?? [] }
        XCTAssertEqual(selected(), ["Topic Alpha", "Topic Gamma"])
        model.textInput(.selectKey(.down, false, true))
        await wait { model.textInputState?.select?.active_option == model.textInputState?.select?.options[1].node_id && !model.textInputBusy }
        XCTAssertEqual(selected(), ["Topic Alpha", "Topic Gamma"])
        model.textInput(.key(.space, false))
        await wait { selected() == ["Topic Alpha", "Topic Beta", "Topic Gamma"] && !model.textInputBusy }
        model.textInput(.key(.end, true))
        await wait { selected() == ["Topic Beta", "Topic Gamma", "Topic Delta", "Topic Emoji 😀"] && !model.textInputBusy }
        await wait { model.representation?.generation == model.generation && model.textInputState?.frame_generation == model.generation
            && model.representation?.nodes.first { $0.name == "Topic Emoji 😀" }?.bounds.height ?? 0 > 0 }
        let snapshot = try XCTUnwrap(model.representation)
        view.accessibilityTree.update(snapshot, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero)
        let list = try XCTUnwrap(view.accessibilityTree.elements[try XCTUnwrap(model.textInputState?.select?.node_id)])
        XCTAssertEqual(list.accessibilityRole(), .list)
        XCTAssertEqual(list.accessibilitySelectedChildren()?.count, 4)
        // Repeated native Tab and activation go through the acknowledged queue.
        model.textInput(.key(.tab, false)); model.textInput(.key(.enter, false)); model.textInput(.key(.tab, true))
        await wait { model.textInputState?.select?.multiple == true && !model.textInputBusy }
        XCTAssertEqual(selected(), ["Topic Alpha", "Topic Gamma"])
        model.textInput(.key(.down, true))
        await wait { selected() == ["Topic Alpha", "Topic Beta"] && !model.textInputBusy }
        await wait { model.representation?.generation == model.generation && model.textInputState?.frame_generation == model.generation
            && model.representation?.nodes.contains { $0.name == "Send choices POST" } == true }
        let current = try XCTUnwrap(model.representation)
        let post = try XCTUnwrap(current.nodes.first { $0.name == "Send choices POST" })
        XCTAssertTrue(model.accessibilityAction(current, epoch: model.accessibilityEpoch, node: post))
        await wait { model.representation?.url == fixture.origin + "/posted" }
        let request = try XCTUnwrap(fixture.records.last)
        XCTAssertEqual(request.method, "POST")
        XCTAssertEqual(String(data: request.body, encoding: .utf8), "region=a&topic=a&topic=b")
        await model.stop()
    }

    func testUndoManagerCommitsCompositionRestoresUTF16SelectionAndKeepsCancelledRedo() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        try await focus("Editor", model: model, view: view)
        let manager = try XCTUnwrap(view.undoManager)
        XCTAssertFalse(manager.isUndoRegistrationEnabled)
        XCTAssertFalse(manager.canUndo); XCTAssertFalse(manager.canRedo)
        model.textInput(.select(TextRange(NSRange(location: 1, length: 2))))
        await wait { model.textInputState?.focused?.selection.nsRange == NSRange(location: 1, length: 2) && !model.textInputBusy }
        let unknown = NSRange(location: NSNotFound, length: 0)
        view.setMarkedText("中", selectedRange: NSRange(location: 1, length: 0), replacementRange: unknown)
        XCTAssertFalse(manager.canUndo); XCTAssertFalse(manager.canRedo)
        view.setMarkedText("中文", selectedRange: NSRange(location: 2, length: 0), replacementRange: unknown)
        await wait { model.textInputState?.focused?.text == "A中文B" && !model.textInputBusy }
        XCTAssertFalse(manager.canUndo); XCTAssertFalse(manager.canRedo)
        view.unmarkText()
        await wait { model.textInputState?.focused?.marked == nil && manager.canUndo && !model.textInputBusy }
        manager.undo()
        await wait { model.textInputState?.focused?.text == "A😀B" && !model.textInputBusy }
        XCTAssertEqual(view.selectedRange(), NSRange(location: 1, length: 2)); XCTAssertTrue(manager.canRedo)
        view.setMarkedText("cancel", selectedRange: NSRange(location: 6, length: 0), replacementRange: unknown)
        await wait { model.textInputState?.focused?.marked != nil && !model.textInputBusy }
        view.doCommand(by: NSSelectorFromString("cancelOperation:"))
        await wait { model.textInputState?.focused?.text == "A😀B" && !model.textInputBusy }
        XCTAssertTrue(manager.canRedo)
        manager.redo()
        await wait { model.textInputState?.focused?.text == "A中文B" && !model.textInputBusy }
        XCTAssertEqual(view.selectedRange(), NSRange(location: 3, length: 0))
        await model.stop()
    }

    func testUndoPasswordPrivacyReadonlyAndQueuedCommandsThroughAppKit() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        try await focus("Secret", model: model, view: view)
        let manager = try XCTUnwrap(view.undoManager)
        view.selectAll(nil); view.insertText("temporary-secret", replacementRange: NSRange(location: NSNotFound, length: 0))
        manager.undo()
        await wait { model.textInputState?.focused?.text_length == UInt32("private-fixture-secret".utf16.count) && !model.textInputBusy && manager.canRedo }
        XCTAssertNil(model.textInputState?.focused?.text)
        XCTAssertNil(view.attributedSubstring(forProposedRange: NSRange(location: 0, length: 2), actualRange: nil))
        manager.redo()
        await wait { model.textInputState?.focused?.text_length == UInt32("temporary-secret".utf16.count) && !model.textInputBusy }
        XCTAssertNil(model.representation?.nodes.first { $0.name == "Secret" }?.state.value)
        try await focus("Readonly", model: model, view: view)
        XCTAssertFalse(manager.canUndo); XCTAssertFalse(manager.canRedo)
        manager.undo(); manager.redo(); XCTAssertEqual(model.textInputState?.focused?.text, "locked")
        await model.stop()
    }

    func testUndoHistoryFollowsWindowTransferAndClearsOnReload() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let workspace = BrowserWorkspace(); defer { Task { await workspace.stop() } }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await workspace.start(launcher: launcher)
        let first = try XCTUnwrap(workspace.models[1])
        first.address = fixture.origin + "/editing"; first.navigateAddress()
        await wait { first.representation?.url == fixture.origin + "/editing" }
        let view = CorePageView(frame: CGRect(x: 0, y: 0, width: 500, height: 300)); view.model = first
        try await focus("Editor", model: first, view: view)
        let tab = try XCTUnwrap(first.selected), old = try XCTUnwrap(first.textInputState?.context)
        view.selectAll(nil); view.insertText("Moved 中文", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { first.textInputState?.focused?.text == "Moved 中文" && !first.textInputBusy }
        let created = await workspace.createWindow(), id = try XCTUnwrap(created)
        let second = try XCTUnwrap(workspace.models[id])
        let moved = await workspace.moveTab(tab, to: id); XCTAssertTrue(moved)
        await wait { second.textInputState?.focused?.text == "Moved 中文" && !second.textInputBusy }
        second.action(.textInput(old, .undo))
        await wait { second.status.contains("Stale") }
        XCTAssertEqual(second.textInputState?.focused?.text, "Moved 中文")
        view.model = second
        try XCTUnwrap(view.undoManager).undo()
        await wait { second.textInputState?.focused?.text == "A😀B" && !second.textInputBusy }
        try XCTUnwrap(view.undoManager).redo()
        await wait { second.textInputState?.focused?.text == "Moved 中文" && !second.textInputBusy }
        let generation = second.generation
        second.reload()
        await wait { second.generation > generation && second.textInputState?.focused == nil }
        try await focus("Editor", model: second, view: view)
        XCTAssertFalse(try XCTUnwrap(view.undoManager).canUndo); XCTAssertFalse(try XCTUnwrap(view.undoManager).canRedo)
        XCTAssertEqual(fixture.requests, ["/editing", "/editing"])
        await workspace.stop()
    }

    func testContextsKeepOneCoreSeparateGroupsAndRestorePersistentKeysWithFreshRuntimeIDs() async throws {
        let domain = "cc.blueice.context-service-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain)); defer { defaults.removePersistentDomain(forName: domain) }
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let workspace = BrowserWorkspace(contextDefaults: defaults); defer { Task { await workspace.stop() } }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await workspace.start(launcher: launcher)
        let first = try XCTUnwrap(workspace.models[1]); let pid = try XCTUnwrap(workspace.processID)
        first.address = fixture.origin + "/editing"; first.navigateAddress()
        await wait { first.representation?.url == fixture.origin + "/editing" }
        let original = try XCTUnwrap(first.selected)
        let editor = try XCTUnwrap(first.representation?.nodes.first { $0.name == "Editor" })
        first.focusPage(x: editor.bounds.x + 5, y: editor.bounds.y + 5)
        await wait { first.textInputState?.focused != nil }
        first.textInput(.selectAll); first.textInput(.replace("Root 中文", nil))
        await wait { first.textInputState?.focused?.text == "Root 中文" && !first.textInputBusy }
        let document = try XCTUnwrap(first.textInputState).document_generation
        let rootGroup = await first.createTabGroup(name: "Root", color: "#4477cc", tab: original)
        XCTAssertNotNil(rootGroup)
        let temporaryResult = await workspace.createContext("Temporary"); let temporary = try XCTUnwrap(temporaryResult)
        let contextResult = await workspace.createContext("Work"); let context = try XCTUnwrap(contextResult)
        let persistentKey = try XCTUnwrap(workspace.contextPreferences.profiles.first { $0.name == "Work" }?.id)
        let createdResult = await workspace.createWindow(contextID: context); let created = try XCTUnwrap(createdResult)
        let work = try XCTUnwrap(workspace.models[created])
        await wait { work.representation?.url == "about:credits" && work.groupsAvailable }
        XCTAssertEqual(work.contextID, context); XCTAssertEqual(work.profileName,"Work"); XCTAssertTrue(work.groups.isEmpty)
        first.apply(IncomingEnvelope(requestID: nil,tabID: nil,message: .tabs(work.tabs)),pixels: nil)
        XCTAssertEqual(first.tabs.map(\.id),[original],"A foreign context's partial list cannot revoke canonical window membership")
        let workGroup = await work.createTabGroup(name: "Research",color: "#cc4477",tab: work.selected)
        XCTAssertNotNil(workGroup)
        XCTAssertEqual(first.groups.map(\.id), [try XCTUnwrap(rootGroup)])
        XCTAssertEqual(work.groups.map(\.id), [try XCTUnwrap(workGroup)])
        let moved = await workspace.moveTab(original,to: created); XCTAssertFalse(moved)
        let assigned = await first.setTabGroup(workGroup,tab: original); XCTAssertFalse(assigned)
        XCTAssertEqual(first.tabs.first?.groupID,rootGroup); XCTAssertEqual(first.textInputState?.focused?.text,"Root 中文", "\(first.status); \(workspace.notice ?? "")")
        XCTAssertEqual(first.textInputState?.document_generation,document); XCTAssertEqual(workspace.processID,pid)
        let renamed = await workspace.renameContext(context,name: "工作"); XCTAssertTrue(renamed)
        await wait { work.profileName == "工作" }
        XCTAssertEqual(workspace.contextPreferences.profiles.first { $0.name == "工作" }?.id,persistentKey)
        let removed = await workspace.closeContext(temporary); XCTAssertTrue(removed)
        let malformed = Data("{\"message\":{\"BrowserContextState\":{\"contexts\":[],\"event\":\"Snapshot\"}}}".utf8)
        workspace.session.received?(.success(try JSONDecoder().decode(IncomingEnvelope.self,from: malformed)))
        await wait { !workspace.canManageContexts && !first.ready }
        XCTAssertEqual(workspace.models.count,2)
        await workspace.refreshContexts()
        await wait { first.ready && first.textInputState?.focused?.text == "Root 中文" && work.ready }
        XCTAssertEqual(first.textInputState?.document_generation,document); XCTAssertEqual(workspace.processID,pid)
        let runtime = workspace.session.runtimeDirectory
        await workspace.stop(); XCTAssertNotEqual(kill(pid,0),0); XCTAssertFalse(FileManager.default.fileExists(atPath: runtime.path))
        let reopened = BrowserWorkspace(contextDefaults: defaults); defer { Task { await reopened.stop() } }
        await reopened.start(launcher: launcher)
        let restored = try XCTUnwrap(reopened.contexts.first { $0.name == "工作" })
        XCTAssertNotEqual(restored.id,context); XCTAssertTrue(restored.windows.isEmpty)
        XCTAssertEqual(reopened.windows.count,1,"Saved names must not allocate unseen profile pages")
        XCTAssertEqual(reopened.contextPreferences.profiles.first { $0.name == "工作" }?.id,persistentKey)
        XCTAssertEqual(fixture.requests,["/editing"])
        await reopened.stop()
    }

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
        for (action, name) in [(TextInputAction.undo, "Undo"), (.redo, "Redo")] {
            let bytes = try BrowserWire.encode(.textInput(context, action), tab: 2, request: 8)
            let root = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes.dropFirst(4)) as? [String: Any])
            let input = try XCTUnwrap((root["message"] as? [String: Any])?["TextInput"] as? [String: Any])
            XCTAssertEqual(input["action"] as? String, name)
        }
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

    func testRetinaViewportZoomPreservesCSSInputGeometryAndTabState() async throws {
        let (model, view, fixture) = try await start(path: "/context-menu")
        defer { fixture.stop(); Task { await model.stop() } }
        await wait { model.displayState != nil }
        model.viewportChanged(CGSize(width: 500, height: 300), deviceScale: 2)
        await wait { model.displayState?.pixelWidth == 1000 && model.representation != nil }
        XCTAssertEqual(model.cssViewportSize, CGSize(width: 500, height: 300))
        let editor = try XCTUnwrap(model.representation?.nodes.first { $0.name == "Context editor" })
        XCTAssertEqual(editor.bounds.width, 280)
        model.setZoom(2)
        await wait { model.zoomPercent == 200 && model.representation != nil }
        XCTAssertEqual(model.displayState?.pixelWidth, 1000)
        XCTAssertEqual(model.cssViewportSize, CGSize(width: 250, height: 150))
        let updated = try XCTUnwrap(model.representation?.nodes.first { $0.name == "Context editor" })
        let result = await model.requestContextMenu(x: updated.bounds.x + 10, y: updated.bounds.y + 10 - (model.representation?.scrollY ?? 0))
        XCTAssertNotNil(result?.input)
        await wait { model.textInputState?.focused != nil }
        model.textInput(.selectAll); model.textInput(.replace("Zoom 中文", nil))
        await wait { model.textInputState?.focused?.text == "Zoom 中文" && !model.textInputBusy }
        let state = try XCTUnwrap(model.textInputState), field = try XCTUnwrap(state.focused)
        let rect = state.viewRect(field.caret, viewport: view.bounds.size, image: try XCTUnwrap(model.cssViewportSize))
        XCTAssertEqual(rect.width, field.caret.width * 2, accuracy: 0.01)
        model.viewportChanged(CGSize(width: 500, height: 300), deviceScale: 1)
        await wait { model.displayState?.pixelWidth == 500 && model.zoomPercent == 200 }
        model.setZoom(1); await wait { model.zoomPercent == 100 }
        model.changeZoom(increase: true); model.changeZoom(increase: true); model.changeZoom(increase: true)
        await wait { model.zoomPercent == 150 }
        let first = try XCTUnwrap(model.selected)
        model.action(.values("OpenTab", ["url": .null]))
        await wait { model.tabs.count == 2 && model.selected != first && model.displayState != nil }
        XCTAssertEqual(model.zoomPercent, 100)
        model.select(first); await wait { model.zoomPercent == 150 }
        XCTAssertEqual(model.selected, first)
        XCTAssertEqual(model.displayState?.zoom, 1.5, "The selected tab must retain its confirmed core zoom")
        XCTAssertEqual(fixture.requests, ["/context-menu"])
    }

    func testViewportWireValidatesPixelAndCSSGeometry() throws {
        let bytes = try BrowserWire.encode(.viewport(500, 300, 2), tab: 2, request: 7)
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes.dropFirst(4)) as? [String: Any])
        let viewport = ((root["message"] as? [String: Any])?["SetViewport"] as? [String: Any])?["viewport"] as? [String: Double]
        XCTAssertEqual(viewport?["device_scale"], 2)
        XCTAssertNil(viewport?["backing_scale"], "Legacy callers retain the existing wire shape")
        let capped = try BrowserWire.encode(.viewport(500, 300, 1, backingScale: 2), tab: 2, request: 8)
        let cappedRoot = try XCTUnwrap(JSONSerialization.jsonObject(with: capped.dropFirst(4)) as? [String: Any])
        let cappedViewport = ((cappedRoot["message"] as? [String: Any])?["SetViewport"] as? [String: Any])?["viewport"] as? [String: Double]
        XCTAssertEqual(cappedViewport?["device_scale"], 1); XCTAssertEqual(cappedViewport?["backing_scale"], 2)
        func decode(_ mutate: (inout [String: Any]) -> Void = { _ in }) throws -> BrowserMessage {
            var state: [String: Any] = ["tab_id": 2, "frame_source": 19, "frame_generation": 7, "width": 500,
                "height": 300, "device_scale": 2, "zoom": 2, "css_width": 250, "css_height": 150,
                "pixel_width": 1000, "pixel_height": 600]
            mutate(&state)
            return try JSONDecoder().decode(IncomingEnvelope.self, from: JSONSerialization.data(withJSONObject: ["message": ["ViewportState": state]])).message
        }
        if case .viewportState = try decode() {} else { XCTFail("Valid geometry must decode") }
        if case .viewportState(let state) = try decode({ $0["backing_scale"] = 3 }) { XCTAssertEqual(state.backingScale, 3) }
        else { XCTFail("Actual backing scale must decode independently of raster density") }
        for bad in [try decode { $0["pixel_width"] = 999 }, try decode { $0["css_width"] = 500 }, try decode { $0["zoom"] = 6 }, try decode { $0["backing_scale"] = 0 }, try decode { $0["backing_scale"] = "2" }] {
            if case .viewportUnavailable = bad {} else { XCTFail("Invalid geometry must fail soft") }
        }
    }

    func testContextWireRejectsProtectedPayloadsAndPreservesOpenedURL() throws {
        let context = PageMenuContext(tabID: 2, frameSource: 19, documentGeneration: 4, frameGeneration: 7, x: 2, y: 3)
        let bytes = try BrowserWire.encode(.contextMenuLink(context, .newTab), tab: 2, request: 8)
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes.dropFirst(4)) as? [String: Any])
        let command = try XCTUnwrap((root["message"] as? [String: Any])?["ContextMenuLink"] as? [String: Any])
        XCTAssertEqual(command["action"] as? String, "OpenInNewTab")
        XCTAssertEqual((command["context"] as? [String: Any])?["frame_generation"] as? Int, 7)
        func decode(_ value: [String: Any]) throws -> BrowserMessage {
            try JSONDecoder().decode(IncomingEnvelope.self, from: JSONSerialization.data(withJSONObject: ["message": value])).message
        }
        if case .opened(let id, let url) = try decode(["TabOpened": ["tab_id": 2, "url": "https://example.test/next"]]) {
            XCTAssertEqual(id, 2); XCTAssertEqual(url, "https://example.test/next")
        } else { XCTFail("Opened URL must survive the native wire") }
        let bounds: [String: Int] = ["x": 2, "y": 3, "width": 100, "height": 20]
        let field: [String: Any] = ["node_id": 5, "text": "secret", "text_length": 6, "protected": true,
            "writable": true, "multiline": false, "selection": ["location": 0, "length": 0], "marked": NSNull(),
            "bounds": bounds, "caret": bounds, "carets": [], "selection_rects": []]
        let input: [String: Any] = ["version": 1, "frame_source": 19, "document_generation": 4, "focus_generation": 3,
            "frame_generation": 7, "tab_id": 2, "scroll_y": 0, "focused": field]
        let encodedContext = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(context)) as? [String: Any])
        for value in [
            ["ContextMenu": ["context": encodedContext, "input": input, "link_url": NSNull()]],
            ["ContextMenuLink": ["context": encodedContext, "url": "javascript:alert(1)"]]
        ] {
            if case .contextUnavailable = try decode(value) {} else { XCTFail("Malformed context metadata must fail soft") }
        }
    }

    private func wait(_ predicate: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
        let deadline = Date().addingTimeInterval(15)
        while !predicate() && Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Timed out waiting for real core text state", file: file, line: line)
    }

    private func start(path: String = "/editing", appearance: BrowserAppearance? = nil) async throws -> (BrowserModel, CorePageView, HTTPFixture) {
        let fixture = try HTTPFixture()
        let model = BrowserModel(appearance: appearance)
        let view = CorePageView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        view.model = model
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await model.start(launcher: launcher)
        await wait { model.representation != nil }
        model.address = fixture.origin + path; model.navigateAddress()
        await wait { model.representation?.url == fixture.origin + path && model.textInputState != nil }
        return (model, view, fixture)
    }

    func testSharedWindowsKeepOneCoreAndTransferEditorHistoryZoomAndGroup() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let workspace = BrowserWorkspace()
        defer { Task { await workspace.stop() } }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await workspace.start(launcher: launcher)
        let first = try XCTUnwrap(workspace.models[1])
        await wait { first.representation?.url == "about:credits" && first.groupsAvailable }
        let pid = try XCTUnwrap(workspace.processID)
        first.address = fixture.origin + "/editing"; first.navigateAddress()
        await wait { first.representation?.url == fixture.origin + "/editing" && first.displayState != nil }
        let tab = try XCTUnwrap(first.selected)
        let editor = try XCTUnwrap(first.representation?.nodes.first { $0.name == "Editor" })
        first.focusPage(x: editor.bounds.x + 5, y: editor.bounds.y + 5)
        await wait { first.textInputState?.focused != nil }
        first.textInput(.selectAll); first.textInput(.replace("Moved 中文", nil))
        first.setZoom(1.5)
        await wait { first.textInputState?.focused?.text == "Moved 中文" && first.zoomPercent == 150 && !first.textInputBusy }
        let groupID = await first.createTabGroup(name: "Shared", color: "#4477cc", tab: tab)
        XCTAssertNotNil(groupID)
        let document = try XCTUnwrap(first.textInputState).document_generation
        let created = await workspace.createWindow()
        let id = try XCTUnwrap(created), second = try XCTUnwrap(workspace.models[id])
        await wait { second.representation?.url == "about:credits" && second.groupsAvailable }
        first.viewportChanged(CGSize(width: 500, height: 300), deviceScale: 2)
        second.viewportChanged(CGSize(width: 320, height: 240), deviceScale: 1)
        await wait { first.displayState?.pixelWidth == 1000 && second.displayState?.pixelWidth == 320 }
        XCTAssertEqual(first.cssViewportSize?.width ?? 0, 500 / 1.5, accuracy: 0.01)
        XCTAssertEqual(second.cssViewportSize?.width, 320)
        first.address = "http://malware.test/"; first.navigateAddress()
        await wait { first.status.contains("Navigation blocked") }
        let denied = first.status
        second.viewportChanged(CGSize(width: 360, height: 240), deviceScale: 1)
        await wait { second.displayState?.pixelWidth == 360 }
        XCTAssertEqual(first.status, denied, "Other window metadata must preserve the denied page notice")
        let moved = await workspace.moveTab(tab, to: id); XCTAssertTrue(moved)
        await wait { first.tabs.isEmpty && second.selected == tab && second.textInputState?.focused?.text == "Moved 中文" && second.zoomPercent == 150 }
        XCTAssertEqual(second.textInputState?.document_generation, document)
        XCTAssertEqual(second.tabs.first { $0.id == tab }?.groupID, groupID)
        XCTAssertTrue(second.history.back)
        XCTAssertEqual(workspace.processID, pid)
        let originalDestinationTab = try XCTUnwrap(second.tabs.first { $0.id != tab }?.id)
        second.action(.values("OpenTab", ["url": .null]))
        await wait { second.tabs.count == 3 && second.selected != tab && second.selected != originalDestinationTab && second.representation?.url == "about:credits" }
        let appended = try XCTUnwrap(second.selected)
        // A legacy peer's global list can arrive after the window snapshot.
        second.apply(IncomingEnvelope(requestID: nil, tabID: nil, message: .tabs(second.tabs.sorted { $0.id < $1.id })), pixels: nil)
        XCTAssertEqual(second.tabs.map(\.id), [originalDestinationTab, tab, appended], "Legacy global Tab lists must preserve canonical window-local order")
        second.action(.unit("CloseTab"))
        await wait { second.tabs.count == 2 }
        second.select(tab)
        await wait { second.textInputState?.focused?.text == "Moved 中文" && second.zoomPercent == 150 }
        let closed = await workspace.closeWindow(1); XCTAssertTrue(closed)
        await wait { !first.ready && second.ready && workspace.models[1] == nil }
        XCTAssertEqual(workspace.processID, pid); XCTAssertEqual(kill(pid, 0), 0)
        second.textInput(.selectAll); second.textInput(.replace("After move", nil))
        await wait { second.textInputState?.focused?.text == "After move" && !second.textInputBusy }
        let thirdID = await workspace.createWindow(openTab: false)
        let third = try XCTUnwrap(thirdID)
        XCTAssertGreaterThan(third, id)
        let movedAgain = await workspace.moveTab(tab, to: third); XCTAssertTrue(movedAgain)
        await wait { workspace.models[third]?.textInputState?.focused?.text == "After move" }
        XCTAssertTrue(second.tabs.allSatisfy { $0.id != tab })
        let destination = try XCTUnwrap(workspace.models[third])
        destination.action(.values("OpenTab", ["url": .string("http://malware.test/")]))
        await wait { destination.tabs.count == 2 && destination.selected == tab && destination.status.contains("Navigation blocked") }
        XCTAssertEqual(second.representation?.url, "about:credits", "Denied new pages must stay in their owning window")
        let deniedTab = try XCTUnwrap(destination.tabs.first { $0.id != tab }?.id)
        destination.action(.unit("CloseTab"), tab: deniedTab)
        await wait { destination.tabs.count == 1 && destination.selected == tab && destination.textInputState?.focused?.text == "After move" }
        XCTAssertEqual(fixture.requests, ["/editing"], "Window transfer and a denied new page must not fetch the page")
        let invalid = try JSONDecoder().decode(IncomingEnvelope.self, from: Data("{\"message\":{\"WindowState\":{\"windows\":[{\"id\":0}],\"event\":\"Snapshot\"}}}".utf8))
        workspace.session.received?(.success(invalid))
        await wait { !workspace.canManageWindows && !destination.ready && !second.ready }
        XCTAssertEqual(workspace.models.count, 2, "Malformed metadata must retain both native models")
        await workspace.refreshWindows()
        await wait { destination.ready && second.ready && destination.representation?.url == fixture.origin + "/editing" && destination.textInputState?.focused?.text == "After move" }
        XCTAssertEqual(destination.textInputState?.document_generation, document)
        XCTAssertEqual(workspace.processID, pid); XCTAssertEqual(fixture.requests, ["/editing"])
        await workspace.stop()
        XCTAssertEqual(kill(pid, 0), -1); XCTAssertEqual(kill(-pid, 0), -1)
        XCTAssertFalse(FileManager.default.fileExists(atPath: workspace.session.runtimeDirectory.path))
    }

    func testNativeTabGroupsPreserveCoreDocumentEditorHistoryAndZoom() async throws {
        let (model, _, fixture) = try await start(path: "/editing")
        defer { fixture.stop(); Task { await model.stop() } }
        await wait { model.groupsAvailable && model.displayState != nil }
        let tab = try XCTUnwrap(model.selected)
        let editor = try XCTUnwrap(model.representation?.nodes.first { $0.name == "Editor" })
        model.focusPage(x: editor.bounds.x + 10, y: editor.bounds.y + 10)
        await wait { model.textInputState?.focused != nil }
        model.textInput(.selectAll); model.textInput(.replace("Grouped 中文", nil))
        await wait { model.textInputState?.focused?.text == "Grouped 中文" && !model.textInputBusy }
        model.setZoom(1.5); await wait { model.zoomPercent == 150 }
        let before = try XCTUnwrap(model.textInputState)
        let created = await model.createTabGroup(name: "  研究  ", color: "#4477CC", tab: tab)
        let id = try XCTUnwrap(created)
        await wait { model.tabs.first { $0.id == tab }?.groupID == id }
        XCTAssertEqual(model.groups.first?.name, "研究"); XCTAssertEqual(model.groups.first?.color, "#4477cc")
        let updated = await model.updateTabGroup(id, name: "Work", color: "#cc3344")
        XCTAssertTrue(updated)
        let collapsed = await model.setTabGroupCollapsed(id, collapsed: true)
        XCTAssertTrue(collapsed); XCTAssertTrue(model.groups.first?.collapsed == true)
        XCTAssertEqual(model.selected, tab); XCTAssertEqual(model.zoomPercent, 150)
        XCTAssertEqual(model.textInputState?.document_generation, before.document_generation)
        XCTAssertEqual(model.textInputState?.focused?.text, "Grouped 中文")
        XCTAssertTrue(model.history.back)
        XCTAssertEqual(fixture.requests, ["/editing"], "Organizing tabs must not navigate or fetch")
        model.action(.values("OpenTab", ["url": .null]))
        await wait { model.tabs.count == 2 && model.selected != tab && model.representation != nil }
        let second = try XCTUnwrap(model.selected)
        XCTAssertNil(model.tabs.first { $0.id == second }?.groupID)
        let assigned = await model.setTabGroup(id, tab: second)
        XCTAssertTrue(assigned)
        let removed = await model.removeTabGroup(id)
        XCTAssertTrue(removed)
        XCTAssertTrue(model.groups.isEmpty); XCTAssertTrue(model.tabs.allSatisfy { $0.groupID == nil })
        XCTAssertEqual(Set(model.tabs.map(\.id)), [tab, second])
        model.select(tab)
        await wait { model.zoomPercent == 150 && model.textInputState?.focused?.text == "Grouped 中文" }
        XCTAssertEqual(fixture.requests, ["/editing"])
        let stale = await model.updateTabGroup(id, name: "Stale", color: "#4477cc")
        XCTAssertFalse(stale); XCTAssertTrue(model.groups.isEmpty)
    }

    func testTabGroupValidationAndCoreErrorsPreserveNavigationDenial() async throws {
        let (model, _, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        await wait { model.groupsAvailable }
        model.address = "http://malware.test/"; model.navigateAddress()
        await wait { model.status.contains("Navigation blocked") }
        let status = model.status, tab = try XCTUnwrap(model.selected)
        let before = try XCTUnwrap(model.textInputState).document_generation
        let overlong = String(repeating: "e\u{301}", count: 41)
        XCTAssertEqual(overlong.count, 41); XCTAssertEqual(overlong.unicodeScalars.count, 82)
        let invalid = await model.createTabGroup(name: overlong, color: "#4477cc", tab: tab)
        XCTAssertNil(invalid); XCTAssertTrue(model.groups.isEmpty)
        XCTAssertEqual(model.status, status)
        // Exercise actual core error replies on both its global and tab-bound
        // routes, including replies that precede the send continuation.
        for action in [TabGroupAction.remove(999_999), .assign(999_999)] {
            model.dismissTabGroupError()
            let request = await model.sendTabGroupCommand(action, tab: {
                if case .assign = action { return tab }
                return nil
            }())
            XCTAssertNotNil(request)
            await wait { model.groupError != nil }
            XCTAssertEqual(model.status, status)
            XCTAssertEqual(model.textInputState?.document_generation, before)
            XCTAssertTrue(model.groups.isEmpty); XCTAssertNil(model.tabs.first?.groupID)
        }
        let created = await model.createTabGroup(name: String(repeating: "e\u{301}", count: 40), color: "#4477cc", tab: tab)
        let id = try XCTUnwrap(created)
        XCTAssertEqual(model.groups.first?.name.unicodeScalars.count, 80)
        let removed = await model.removeTabGroup(id); XCTAssertTrue(removed)
        XCTAssertEqual(model.status, status)
        XCTAssertEqual(fixture.requests, ["/editing"])
    }

    func testDisplayPreferencesUpdateSharedPixelsWithoutLosingEditedTextOrDocument() async throws {
        let domain = "cc.blueice.appearance.core." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain)); defer { defaults.removePersistentDomain(forName: domain) }
        let appearance = BrowserAppearance(defaults: defaults, systemPreferences: { DisplayPreferences() }, observeApplication: false)
        let (model, _, fixture) = try await start(path: "/appearance", appearance: appearance)
        defer { fixture.stop(); Task { await model.stop() } }
        await wait { model.displayPreferences == DisplayPreferences() && model.representation?.nodes.contains { $0.name == "Light content" } == true }
        func pixels(_ rgb: [UInt8]) throws -> Int {
            let image = try XCTUnwrap(model.image)
            let data = try XCTUnwrap(image.dataProvider?.data) as Data
            return stride(from: 0, to: data.count, by: 4).filter { offset in
                data[offset] == rgb[0] && data[offset + 1] == rgb[1] && data[offset + 2] == rgb[2]
            }.count
        }
        XCTAssertGreaterThan(try pixels([170, 187, 204]), 100)
        let editor = try XCTUnwrap(model.representation?.nodes.first { $0.name == "Appearance editor" })
        model.focusPage(x: editor.bounds.x + 10, y: editor.bounds.y + 10)
        await wait { model.textInputState?.focused != nil }
        model.textInput(.selectAll); model.textInput(.replace("Retained 中文", nil))
        await wait { model.textInputState?.focused?.text == "Retained 中文" && !model.textInputBusy }
        let before = try XCTUnwrap(model.textInputState)
        appearance.setAppearance(.dark); appearance.setContrast(.increased); appearance.setMotion(.reduced)
        let preferences = DisplayPreferences(dark: true, highContrast: true, reducedMotion: true)
        await wait { model.displayPreferences == preferences && model.representation?.nodes.contains { $0.name == "Reduced motion" } == true }
        XCTAssertEqual(model.textInputState?.document_generation, before.document_generation)
        XCTAssertEqual(model.textInputState?.focused?.text, "Retained 中文")
        XCTAssertTrue(model.representation?.nodes.contains { $0.name == "Dark content" } == true)
        XCTAssertFalse(model.representation?.nodes.contains { $0.name == "Light content" } == true)
        XCTAssertGreaterThan(try pixels([16, 32, 48]), 100)
        XCTAssertEqual(try pixels([170, 187, 204]), 0)
        appearance.setAppearance(.light); appearance.setAppearance(.dark); appearance.setAppearance(.light)
        await wait { model.displayPreferences?.dark == false && model.representation?.nodes.contains { $0.name == "Light content" } == true }
        model.viewportChanged(CGSize(width: 2300, height: 300), deviceScale: 2)
        await wait { model.displayState?.backingScale == 2 && model.representation?.nodes.contains { $0.name == "High density screen" } == true }
        XCTAssertEqual(model.image?.width, 4096)
        XCTAssertLessThan(try XCTUnwrap(model.displayState?.deviceScale), 2)
        XCTAssertEqual(model.textInputState?.focused?.text, "Retained 中文")
        XCTAssertEqual(fixture.requests, ["/appearance"], "Preference changes must not reload or fetch")
    }

    func testContextMenuUsesCoreHitTargetsAndRejectsStaleMenus() async throws {
        let (model, view, fixture) = try await start(path: "/context-menu")
        defer { fixture.stop(); Task { await model.stop() } }
        func menu(_ name: String) async throws -> PageContextMenu {
            await wait { model.representation?.nodes.contains { $0.name == name } == true && !model.textInputBusy }
            let snapshot = try XCTUnwrap(model.representation)
            let node = try XCTUnwrap(snapshot.nodes.first { $0.name == name })
            let result = await model.requestContextMenu(x: node.bounds.x + node.bounds.width / 2,
                y: node.bounds.y + node.bounds.height / 2 - snapshot.scrollY)
            return try XCTUnwrap(result)
        }
        let link = try await menu("Destination link")
        XCTAssertEqual(link.linkURL, fixture.origin + "/destination")
        XCTAssertNil(link.input)
        XCTAssertEqual(fixture.requests, ["/context-menu"])
        let field = try await menu("Context editor")
        XCTAssertEqual(field.input?.focused?.text, "hello")
        XCTAssertFalse(model.contextMenuIsCurrent(link.context), "Focus-generated pixels invalidate the old link menu")
        model.textInput(.selectAll); await wait { !model.textInputBusy }
        let selected = try await menu("Context editor")
        XCTAssertEqual(selected.input?.focused?.selection.length, 5)
        XCTAssertTrue(model.contextMenuIsCurrent(selected.context))
        let menuItems = view.makeContextMenu(selected).items
        XCTAssertTrue(try XCTUnwrap(menuItems.first { $0.title == "Copy" }).isEnabled)
        let secret = try await menu("Context secret")
        XCTAssertNil(secret.input?.focused?.text)
        let secretItems = view.makeContextMenu(secret).items
        XCTAssertFalse(try XCTUnwrap(secretItems.first { $0.title == "Copy" }).isEnabled)
        XCTAssertFalse(try XCTUnwrap(secretItems.first { $0.title == "Cut" }).isEnabled)
        XCTAssertTrue(try XCTUnwrap(secretItems.first { $0.title == "Paste" }).isEnabled)
        let readonly = try await menu("Context readonly")
        XCTAssertFalse(readonly.input?.focused?.writable ?? true)
        let readonlyItems = view.makeContextMenu(readonly).items
        XCTAssertFalse(try XCTUnwrap(readonlyItems.first { $0.title == "Paste" }).isEnabled)
        let editorBefore = model.textInputState?.focused
        let documentMenu = try await menu("Destination link")
        XCTAssertNil(documentMenu.input)
        XCTAssertNotNil(documentMenu.document)
        XCTAssertEqual(model.textInputState?.focused?.node_id, editorBefore?.node_id)
        let documentItems = view.makeContextMenu(documentMenu).items
        XCTAssertNotNil(documentItems.first { $0.title == "Select All" && $0.isEnabled })
        XCTAssertNil(documentItems.first { $0.title == "Cut" || $0.title == "Paste" })
        model.textInput(.documentSelectAll); await wait { !model.textInputBusy && model.textInputState?.document?.active == true }
        let selectedDocument = try await menu("Destination link")
        XCTAssertTrue(try XCTUnwrap(view.makeContextMenu(selectedDocument).items.first { $0.title == "Copy" }).isEnabled)
        XCTAssertEqual(model.textInputState?.focused?.text, editorBefore?.text)
        let old = try await menu("Destination link")
        model.action(.values("OpenTab", ["url": .null]))
        await wait { model.tabs.count == 2 && model.selected != old.context.tabID }
        XCTAssertFalse(model.contextMenuIsCurrent(old.context))
        XCTAssertEqual(fixture.requests, ["/context-menu"])
    }

    func testMouseClickDuringFrameGeometryGapFocusesReadonlyThroughRealCore() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        let window = NSWindow(contentRect: view.bounds, styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.contentView = view
        XCTAssertTrue(window.makeFirstResponder(view))
        try await focus("Secret", model: model, view: view)
        await wait { model.cssViewportSize != nil && model.representation != nil }
        let snapshot = try XCTUnwrap(model.representation)
        let readonly = try XCTUnwrap(snapshot.nodes.first { $0.name == "Readonly" })
        let geometry = try XCTUnwrap(model.displayState)
        let rect = snapshot.viewRect(for: readonly, viewport: view.bounds.size, image: try XCTUnwrap(model.cssViewportSize))
        let point = view.convert(NSPoint(x: rect.midX, y: rect.midY), to: nil)
        model.apply(IncomingEnvelope(requestID: nil, tabID: snapshot.tabID, message: .viewportUnavailable), pixels: nil)
        XCTAssertNil(model.cssViewportSize, "Reproduce the suspension between a frame and its viewport reply")
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            let event = try XCTUnwrap(NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: 0,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
            if type == .leftMouseDown { view.mouseDown(with: event) } else { view.mouseUp(with: event) }
        }
        let selectAll = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .command,
            timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: "a", charactersIgnoringModifiers: "a",
            isARepeat: false, keyCode: 0))
        view.keyDown(with: selectAll)
        model.apply(IncomingEnvelope(requestID: nil, tabID: snapshot.tabID, message: .viewportState(geometry)), pixels: nil)
        await wait { model.textInputState?.focused?.node_id == readonly.id && !model.textInputBusy
            && model.textInputState?.focused?.selection.length == 6 }
        XCTAssertEqual(model.textInputState?.focused?.text, "locked")
        view.selectAll(nil); view.insertText("bad", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { !model.textInputBusy }
        XCTAssertEqual(model.textInputState?.focused?.text, "locked")
        await model.stop()
    }

    func testDeferredMouseClickIsCancelledByResizeAndTabSwitch() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        let window = NSWindow(contentRect: view.bounds, styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.contentView = view
        XCTAssertTrue(window.makeFirstResponder(view))
        let tab = try XCTUnwrap(model.selected)
        model.action(.values("OpenTab", ["url": .null]))
        await wait { model.selected != tab && model.representation != nil }
        let other = try XCTUnwrap(model.selected)
        model.select(tab)
        await wait { model.selected == tab && model.representation != nil }
        try await focus("Secret", model: model, view: view)
        for resize in [true, false] {
            await wait { model.representation != nil && model.cssViewportSize != nil && !model.textInputBusy }
            let snapshot = try XCTUnwrap(model.representation), geometry = try XCTUnwrap(model.displayState)
            let readonly = try XCTUnwrap(snapshot.nodes.first { $0.name == "Readonly" })
            let previousFocus = model.textInputState?.focused?.node_id
            let previousSelection = model.textInputState?.focused?.selection
            let rect = snapshot.viewRect(for: readonly, viewport: view.bounds.size, image: try XCTUnwrap(model.cssViewportSize))
            let point = view.convert(NSPoint(x: rect.midX, y: rect.midY), to: nil)
            let unexpectedFocus = expectation(description: "Cancelled pointer must never focus readonly")
            unexpectedFocus.isInverted = true
            let observation = model.$textInputState.sink { state in
                if state?.tab_id == tab && state?.focused?.node_id == readonly.id { unexpectedFocus.fulfill() }
            }
            model.apply(IncomingEnvelope(requestID: nil, tabID: tab, message: .viewportUnavailable), pixels: nil)
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                let event = try XCTUnwrap(NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: 0,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
                if type == .leftMouseDown { view.mouseDown(with: event) } else { view.mouseUp(with: event) }
            }
            let selectAll = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .command,
                timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: "a", charactersIgnoringModifiers: "a",
                isARepeat: false, keyCode: 0))
            view.keyDown(with: selectAll)
            if resize { view.setFrameSize(CGSize(width: view.bounds.width - 1, height: view.bounds.height)) }
            else { model.select(other) }
            model.apply(IncomingEnvelope(requestID: nil, tabID: tab, message: .viewportState(geometry)), pixels: nil)
            await fulfillment(of: [unexpectedFocus], timeout: 0.25)
            observation.cancel()
            if resize { view.setFrameSize(CGSize(width: 500, height: 300)) }
            else { model.select(tab) }
            await wait { model.textInputState?.tab_id == tab && !model.textInputBusy }
            XCTAssertEqual(model.textInputState?.focused?.node_id, previousFocus)
            XCTAssertEqual(model.textInputState?.focused?.selection, previousSelection)
        }
        await model.stop()
    }

    func testRedoKeyWaitsForPointerFocusAcknowledgementThroughRealCore() async throws {
        let (model, view, fixture) = try await start()
        defer { fixture.stop(); Task { await model.stop() } }
        let window = NSWindow(contentRect: view.bounds, styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.contentView = view
        XCTAssertTrue(window.makeFirstResponder(view))
        try await focus("Editor", model: model, view: view)
        view.selectAll(nil); view.insertText("Second tab", replacementRange: NSRange(location: NSNotFound, length: 0))
        await wait { !model.textInputBusy && model.textInputState?.focused?.text == "Second tab" }
        view.undo(nil)
        await wait { !model.textInputBusy && model.textInputState?.focused?.text == "A😀B"
            && model.textInputState?.focused?.can_redo == true && model.cssViewportSize != nil && model.representation != nil }
        let snapshot = try XCTUnwrap(model.representation)
        let node = try XCTUnwrap(snapshot.nodes.first { $0.name == "Editor" })
        let rect = snapshot.viewRect(for: node, viewport: view.bounds.size, image: try XCTUnwrap(model.cssViewportSize))
        let point = view.convert(NSPoint(x: rect.midX, y: rect.midY), to: nil)
        let down = try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseDown, location: point, modifierFlags: [], timestamp: 0,
            windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1))
        view.mouseDown(with: down)
        XCTAssertNil(model.textInputState, "Pointer focus suspends native command validation before its acknowledgement")
        let redo = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.command, .shift],
            timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: "Z", charactersIgnoringModifiers: "z",
            isARepeat: false, keyCode: 6))
        view.keyDown(with: redo)
        await wait { !model.textInputBusy && model.textInputState?.focused?.text == "Second tab" }
        await model.stop()
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
        window.isReleasedWhenClosed = false
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
        window.isReleasedWhenClosed = false
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
        await wait { model.representation?.generation == model.generation
            && model.cssViewportSize != nil && !model.textInputBusy }
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
