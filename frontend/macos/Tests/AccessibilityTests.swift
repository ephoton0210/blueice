// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import XCTest

@MainActor
final class AccessibilityTests: XCTestCase {
    private func representation(_ mutate: (inout [String: Any]) -> Void = { _ in }) throws -> PageRepresentation {
        func node(_ id: Int, _ parent: Any, _ children: [Int], _ role: Any, _ name: String) -> [String: Any] {
            ["id": id, "parent": parent, "children": children, "role": role, "name": name,
             "state": ["value": "hello", "checked": false, "disabled": false, "required": true,
                       "selected": false, "hovered": false, "focused": false],
             "bounds": ["x": 20, "y": 120, "width": 160, "height": 40], "opacity": 1,
             "occluded": false, "occluded_fraction": 0]
        }
        var json: [String: Any] = ["frame_source": 19, "generation": 7, "tab_id": 2, "url": "about:credits",
                                  "scroll_y": 100, "nodes": [node(1, NSNull(), [2], ["Heading": ["level": 2]], "Title"),
                                                            node(2, 1, [], "Link", "Next")]]
        mutate(&json)
        return try JSONDecoder().decode(PageRepresentation.self, from: JSONSerialization.data(withJSONObject: json))
    }

    func testRepresentationDecodesRustRolesAndEnvelope() throws {
        let snapshot = try representation()
        XCTAssertEqual(snapshot.nodes.first?.role, .heading(2))
        XCTAssertEqual(snapshot.nodes.last?.role, .link)
        let payload = Data(#"{"request_id":8,"tab_id":2,"message":{"Representation":{"frame_source":19,"generation":7,"tab_id":2,"url":"about:credits","scroll_y":0,"nodes":[]}}}"#.utf8)
        let envelope = try JSONDecoder().decode(IncomingEnvelope.self, from: payload)
        if case .representation(let value) = envelope.message { XCTAssertEqual(value.tabID, 2) }
        else { XCTFail("Representation must reach the native bridge") }
    }

    func testLiveAnnouncementsBaselineDeduplicateAndDropInactiveOrForeignDocumentUpdates() throws {
        var spoken: [String] = []
        var active = true
        let view = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        let tree = PageAccessibilityTree(view: view, isActive: { active }, announce: { text, _ in spoken.append(text) }) { _, _, _, _ in true }
        func snapshot(_ revision: Int, _ document: Int = 1, _ tab: Int = 2) throws -> PageRepresentation {
            try representation { value in
                value["tab_id"] = tab
                value["generation"] = revision + 7
                value["accessibility"] = ["document_generation": document, "revision": revision,
                    "announcements": revision == 0 ? [] : [["sequence": revision, "region_id": 1, "text": "Update \(revision)", "politeness": "Polite"]]]
            }
        }
        tree.update(try snapshot(1), epoch: 1, imageSize: view.bounds.size)
        XCTAssertTrue(spoken.isEmpty, "Existing document content establishes a baseline")
        tree.update(try snapshot(2), epoch: 1, imageSize: view.bounds.size)
        tree.update(try snapshot(2), epoch: 1, imageSize: view.bounds.size)
        XCTAssertEqual(spoken, ["Update 2"])
        tree.update(nil, epoch: 1, imageSize: view.bounds.size, tab: 2)
        tree.update(try snapshot(2), epoch: 1, imageSize: view.bounds.size)
        XCTAssertEqual(spoken.count, 1)
        active = false
        tree.update(try snapshot(3), epoch: 1, imageSize: view.bounds.size)
        active = true
        tree.update(try snapshot(3), epoch: 1, imageSize: view.bounds.size)
        XCTAssertEqual(spoken.count, 1, "Returning to the app must not replay background updates")
        tree.update(try snapshot(4, 2), epoch: 2, imageSize: view.bounds.size)
        tree.update(try snapshot(5, 2, 3), epoch: 2, imageSize: view.bounds.size)
        tree.update(try snapshot(6, 2), epoch: 2, imageSize: view.bounds.size)
        XCTAssertEqual(spoken.count, 1, "Reloads and tab selections must baseline their own streams")
        tree.update(try snapshot(7, 2), epoch: 2, imageSize: view.bounds.size)
        XCTAssertEqual(spoken.last, "Update 7")
    }

    func testRotorSearchUsesDocumentOrderFilterDirectionAndRejectsOldOrForeignItems() throws {
        let view = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        var actions: [PageAccessibilityAction] = []
        let tree = PageAccessibilityTree(view: view) { _, _, _, action in actions.append(action); return true }
        let snapshot = try representation { value in
            var nodes = value["nodes"] as! [[String: Any]]
            nodes[0]["children"] = []
            nodes[1]["parent"] = NSNull()
            nodes[1]["bounds"] = ["x": 20, "y": 1500, "width": 160, "height": 40]
            value["nodes"] = nodes
        }
        tree.update(snapshot, epoch: 1, imageSize: view.bounds.size)
        let rotor = try XCTUnwrap(tree.rotors.first { $0.type == .link })
        let parameters = NSAccessibilityCustomRotor.SearchParameters()
        parameters.searchDirection = .next; parameters.filterString = "nÉ"
        let result = try XCTUnwrap(tree.rotor(rotor, resultFor: parameters))
        let link = try XCTUnwrap(result.targetElement as? PageAccessibilityElement)
        XCTAssertEqual(link.accessibilityLabel(), "Next")
        XCTAssertTrue(actions.isEmpty, "Searching alone must not scroll or activate a link")
        link.setAccessibilityFocused(true)
        XCTAssertEqual(actions, [.reveal])
        parameters.currentItem = result
        XCTAssertNil(tree.rotor(rotor, resultFor: parameters), "Search must not wrap")
        parameters.searchDirection = .previous
        XCTAssertNil(tree.rotor(rotor, resultFor: parameters))
        parameters.currentItem = nil
        XCTAssertNotNil(tree.rotor(rotor, resultFor: parameters))
        let foreignView = NSView()
        let foreign = PageAccessibilityTree(view: foreignView) { _, _, _, _ in true }
        foreign.update(snapshot, epoch: 1, imageSize: view.bounds.size)
        parameters.currentItem = NSAccessibilityCustomRotor.ItemResult(targetElement: try XCTUnwrap(foreign.elements[2]))
        XCTAssertNil(tree.rotor(rotor, resultFor: parameters))
        tree.update(snapshot, epoch: 2, imageSize: view.bounds.size)
        parameters.currentItem = nil
        XCTAssertNil(tree.rotor(rotor, resultFor: parameters), "A rotor from a replaced document must fail closed")
        link.setAccessibilityFocused(true)
        XCTAssertEqual(actions.count, 1)
    }

    func testNativeTreeAndRotorsExcludeCoreHiddenNodesAndValidateAnnouncements() throws {
        let view = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        let tree = PageAccessibilityTree(view: view) { _, _, _, _ in true }
        let hidden = try representation { value in
            value["accessibility"] = ["document_generation": 1, "revision": 0, "announcements": [], "hidden_nodes": [2]]
        }
        tree.update(hidden, epoch: 1, imageSize: view.bounds.size)
        XCTAssertNil(tree.elements[2])
        let parameters = NSAccessibilityCustomRotor.SearchParameters()
        parameters.searchDirection = .next; parameters.filterString = ""
        let rotor = try XCTUnwrap(tree.rotors.first { $0.type == .link })
        XCTAssertNil(tree.rotor(rotor, resultFor: parameters))
        for event in [["sequence": 2, "region_id": 1, "text": "future", "politeness": "Polite"],
                      ["sequence": 1, "region_id": 1, "text": "", "politeness": "Polite"],
                      ["sequence": 1, "region_id": 1, "text": "bad", "politeness": "Off"]] {
            XCTAssertThrowsError(try representation { value in
                value["accessibility"] = ["document_generation": 1, "revision": 1, "announcements": [event]]
            })
        }
    }

    func testRotorMovesBothDirectionsInStableDocumentOrder() throws {
        let view = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        let tree = PageAccessibilityTree(view: view) { _, _, _, _ in true }
        let snapshot = try representation { value in
            let original = (value["nodes"] as! [[String: Any]])[1]
            value["nodes"] = ["Àlpha", "Beta", "Gamma"].enumerated().map { index, label in
                var node = original; node["id"] = index + 1; node["parent"] = NSNull(); node["children"] = []
                node["name"] = label; return node
            }
        }
        tree.update(snapshot, epoch: 1, imageSize: view.bounds.size)
        let rotor = try XCTUnwrap(tree.rotors.first { $0.type == .link })
        let parameters = NSAccessibilityCustomRotor.SearchParameters(); parameters.searchDirection = .next; parameters.filterString = ""
        func next() throws -> NSAccessibilityCustomRotor.ItemResult { try XCTUnwrap(tree.rotor(rotor, resultFor: parameters)) }
        let first = try next(); XCTAssertEqual((first.targetElement as? PageAccessibilityElement)?.accessibilityLabel(), "Àlpha")
        parameters.currentItem = first
        let second = try next(); XCTAssertEqual((second.targetElement as? PageAccessibilityElement)?.accessibilityLabel(), "Beta")
        parameters.currentItem = second
        let last = try next(); XCTAssertEqual((last.targetElement as? PageAccessibilityElement)?.accessibilityLabel(), "Gamma")
        parameters.currentItem = last; XCTAssertNil(tree.rotor(rotor, resultFor: parameters))
        parameters.searchDirection = .previous
        XCTAssertEqual((try next().targetElement as? PageAccessibilityElement)?.accessibilityLabel(), "Beta")
        parameters.filterString = "ALPHA"
        XCTAssertEqual((try next().targetElement as? PageAccessibilityElement)?.accessibilityLabel(), "Àlpha")
        tree.update(snapshot, epoch: 1, imageSize: view.bounds.size)
        parameters.currentItem = first; parameters.searchDirection = .next; parameters.filterString = ""
        XCTAssertEqual((try next().targetElement as? PageAccessibilityElement)?.accessibilityLabel(), "Beta")
    }

    func testCoreDocumentGenerationInvalidatesRotorsWhenFrontendEpochRepeats() throws {
        let view = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        var actions = 0
        let tree = PageAccessibilityTree(view: view) { _, _, _, _ in actions += 1; return true }
        func snapshot(_ document: Int) throws -> PageRepresentation {
            try representation { value in
                value["generation"] = document + 7
                value["accessibility"] = ["document_generation": document, "revision": 0, "announcements": []]
            }
        }
        tree.update(try snapshot(1), epoch: 1, imageSize: view.bounds.size)
        let old = try XCTUnwrap(tree.elements[1])
        let rotor = try XCTUnwrap(tree.rotors.first { $0.type == .heading })
        tree.update(try snapshot(2), epoch: 1, imageSize: view.bounds.size)
        XCTAssertFalse(tree.elements[1] === old)
        XCTAssertFalse(old.isAccessibilityEnabled())
        old.setAccessibilityFocused(true); XCTAssertEqual(actions, 0)
        let parameters = NSAccessibilityCustomRotor.SearchParameters(); parameters.searchDirection = .next; parameters.filterString = ""
        XCTAssertNil(tree.rotor(rotor, resultFor: parameters))
    }

    func testSnapshotRequiresMatchingTabFrameSourceGenerationAndURL() throws {
        let snapshot = try representation()
        XCTAssertTrue(snapshot.matches(tab: 2, generation: 7, source: 19, url: "about:credits"))
        XCTAssertFalse(snapshot.matches(tab: 3, generation: 7, source: 19, url: "about:credits"))
        XCTAssertFalse(snapshot.matches(tab: 2, generation: 8, source: 19, url: "about:credits"))
        XCTAssertFalse(snapshot.matches(tab: 2, generation: 7, source: 20, url: "about:credits"))
        XCTAssertFalse(snapshot.matches(tab: 2, generation: 7, source: 19, url: "about:settings"))
    }

    func testMalformedSemanticTreesAreRejected() throws {
        for mutation in 0..<5 {
            XCTAssertThrowsError(try representation { json in
                var nodes = json["nodes"] as! [[String: Any]]
                switch mutation {
                case 0: nodes[1]["id"] = 1
                case 1: nodes[1]["parent"] = 999
                case 2: nodes[0]["children"] = []
                case 3: nodes[0]["parent"] = 2; nodes[1]["children"] = [1]
                default: nodes[0]["bounds"] = ["x": 0, "y": 0, "width": -1, "height": 20]
                }
                json["nodes"] = nodes
            })
        }
    }

    func testGeometryConvertsDocumentScrollAndRetinaScale() throws {
        let snapshot = try representation()
        let rect = snapshot.viewRect(for: snapshot.nodes[0], viewport: CGSize(width: 500, height: 300),
                                     image: CGSize(width: 1000, height: 600))
        XCTAssertEqual(rect, CGRect(x: 10, y: 10, width: 80, height: 20))
    }

    func testNativeTreePreservesHierarchyRolesAndScreenBounds() throws {
        let view = NSView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        let window = NSWindow(contentRect: CGRect(x: 100, y: 200, width: 500, height: 300),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = view
        let tree = PageAccessibilityTree(view: view) { _, _, _, _ in true }
        let snapshot = try representation()
        tree.update(snapshot, epoch: 1, imageSize: CGSize(width: 1000, height: 600))
        let heading = try XCTUnwrap(tree.elements[1])
        let link = try XCTUnwrap(tree.elements[2])
        XCTAssertEqual(heading.accessibilityLabel(), "Title")
        XCTAssertEqual(link.accessibilityRole(), .link)
        XCTAssertTrue(link.accessibilityParent() as? PageAccessibilityElement === heading)
        XCTAssertEqual(heading.accessibilityChildren()?.count, 1)
        XCTAssertEqual(link.accessibilityFrame(), window.convertToScreen(view.convert(CGRect(x: 10, y: 10, width: 80, height: 20), to: nil)))
        XCTAssertTrue(link.accessibilityPerformPress())
    }

    func testOldElementsCannotActAfterSameURLReloadOrTabSwitch() throws {
        let view = NSView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        var actions = 0
        let tree = PageAccessibilityTree(view: view) { _, _, _, _ in actions += 1; return true }
        let snapshot = try representation()
        tree.update(snapshot, epoch: 1, imageSize: CGSize(width: 1000, height: 600))
        let old = try XCTUnwrap(tree.elements[2])
        XCTAssertTrue(old.accessibilityPerformPress())
        tree.update(snapshot, epoch: 2, imageSize: CGSize(width: 1000, height: 600))
        XCTAssertFalse(old.accessibilityPerformPress())
        let reloaded = try XCTUnwrap(tree.elements[2])
        tree.update(nil, epoch: 2, imageSize: .zero)
        XCTAssertFalse(reloaded.accessibilityPerformPress())
        XCTAssertEqual(actions, 1)
        XCTAssertTrue(tree.elements.isEmpty)
    }

    func testDisabledAndOffscreenElementsDoNotAdvertisePress() throws {
        let view = NSView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        var actions = 0
        let tree = PageAccessibilityTree(view: view) { _, _, _, _ in actions += 1; return true }
        for disabled in [true, false] {
            let snapshot = try representation { json in
                var nodes = json["nodes"] as! [[String: Any]]
                if disabled { var state = nodes[1]["state"] as! [String: Any]; state["disabled"] = true; nodes[1]["state"] = state }
                else { nodes[1]["bounds"] = ["x": 20, "y": 900, "width": 160, "height": 40] }
                json["nodes"] = nodes
            }
            tree.update(snapshot, epoch: 1, imageSize: CGSize(width: 1000, height: 600))
            XCTAssertFalse(try XCTUnwrap(tree.elements[2]).accessibilityPerformPress())
        }
        XCTAssertEqual(actions, 0)
    }

    func testNativeActionsUseRealCoreAndReviewedNavigation() async throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent()
            .appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let model = BrowserModel()
        let view = CorePageView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        view.model = model
        defer { Task { await model.stop() } }
        await model.start(launcher: launcher)
        await wait { model.representation != nil }
        model.address = fixture.origin + "/accessibility"
        model.navigateAddress()
        await wait { model.representation?.url == fixture.origin + "/accessibility" }
        func refresh() {
            view.accessibilityTree.update(model.representation, epoch: model.accessibilityEpoch,
                                          imageSize: model.image.map { CGSize(width: $0.width, height: $0.height) } ?? .zero)
        }
        refresh()
        let field = try XCTUnwrap(view.accessibilityTree.elements.values.first { $0.accessibilityLabel() == "Name" })
        field.setAccessibilityFocused(true)
        await wait { model.representation?.nodes.contains(where: { $0.name == "Name" && $0.state.focused }) == true }
        model.action(.values("InsertText", ["text": .string(" world")]))
        await wait { model.representation?.nodes.contains(where: { $0.name == "Name" && $0.state.value == "hello world" }) == true }
        let secret = try XCTUnwrap(model.representation?.nodes.first { $0.name == "Secret" })
        XCTAssertNil(secret.state.value)
        XCTAssertTrue(secret.state.protected)
        XCTAssertTrue(secret.state.nativeTextInput)
        let previous = try XCTUnwrap(model.representation)
        let previousEpoch = model.accessibilityEpoch
        model.reload()
        await wait { model.representation?.generation ?? 0 > previous.generation }
        XCTAssertFalse(model.accessibilityAction(previous, epoch: previousEpoch, node: previous.nodes[0]))
        refresh()
        let link = try XCTUnwrap(view.accessibilityTree.elements.values.first { $0.accessibilityLabel() == "Open destination" })
        XCTAssertTrue(link.accessibilityPerformPress())
        await wait { model.representation?.url == fixture.origin + "/destination" }
        refresh()
        XCTAssertFalse(link.accessibilityPerformPress())
        XCTAssertEqual(fixture.requests, ["/accessibility", "/accessibility", "/destination"])
        await model.stop()
        XCTAssertNil(model.representation)
    }

    func testActualCoreLiveAnnouncementsAndOffscreenNativeRotorReveal() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent()
            .appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let model = BrowserModel()
        defer { Task { await model.stop() } }
        let view = CorePageView(frame: CGRect(x: 0, y: 0, width: 500, height: 300)); view.model = model
        model.viewportChanged(view.bounds.size)
        let window = NSWindow(contentRect: NSRect(x: 40, y: 40, width: 500, height: 300), styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.contentView = view
        defer { window.close() }
        var spoken: [String] = []
        let tree = PageAccessibilityTree(view: view, isActive: { true }, announce: { text, _ in spoken.append(text) }) { snapshot, epoch, node, action in
            if action == .reveal { return model.accessibilityReveal(snapshot, epoch: epoch, node: node) }
            return model.accessibilityAction(snapshot, epoch: epoch, node: node, focusOnly: action == .focus)
        }
        view.accessibilityTree = tree
        window.makeFirstResponder(view)
        await model.start(launcher: launcher)
        await wait { model.representation != nil }
        model.address = fixture.origin + "/accessibility-live"; model.navigateAddress()
        await wait { model.representation?.url == fixture.origin + "/accessibility-live" && model.textInputState?.frame_generation == model.representation?.generation }
        func refresh() { tree.update(model.representation, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero) }
        refresh()
        let editor = try XCTUnwrap(tree.elements.values.first { $0.accessibilityLabel() == "Status editor" })
        editor.setAccessibilityFocused(true)
        await wait { model.representation?.nodes.contains(where: { $0.name == "Status editor" && $0.state.focused }) == true }
        model.action(.values("InsertText", ["text": .string("😀")]))
        await wait { model.representation?.nodes.contains(where: { $0.name == "Status editor" && $0.state.value == "1😀" }) == true && model.textInputState?.frame_generation == model.representation?.generation }
        refresh(); refresh()
        XCTAssertEqual(spoken, ["Progress 1😀"])
        XCTAssertFalse(spoken.joined().contains("secret")); XCTAssertFalse(spoken.joined().contains("quiet content"))
        let rotor = try XCTUnwrap(tree.rotors.first { $0.type == .headingLevel2 })
        XCTAssertTrue(view.accessibilityCustomRotors().contains { $0 === rotor })
        let parameters = NSAccessibilityCustomRotor.SearchParameters(); parameters.searchDirection = .next; parameters.filterString = "lower"
        let target = try XCTUnwrap(tree.rotor(rotor, resultFor: parameters)?.targetElement as? PageAccessibilityElement)
        XCTAssertGreaterThan(try XCTUnwrap(model.representation?.nodes.first { $0.name == "Lower rotor heading" }).bounds.y, 1000)
        let previous = try XCTUnwrap(model.representation)
        target.setAccessibilityFocused(true)
        await wait { model.representation?.generation ?? 0 > previous.generation && model.representation?.scrollY ?? 0 > 900 }
        refresh()
        XCTAssertEqual(model.representation?.url, fixture.origin + "/accessibility-live")
        XCTAssertTrue(tree.focusedElement() === target)
        tree.update(nil, epoch: model.accessibilityEpoch, imageSize: model.cssViewportSize ?? .zero, tab: model.selected)
        XCTAssertNil(tree.focusedElement(), "A transient frame gap must not expose a stale reading target")
        refresh(); XCTAssertTrue(tree.focusedElement() === target)
        target.setAccessibilityFocused(false)
        XCTAssertFalse(target.isAccessibilityFocused(), "Leaving a rotor item clears reading focus without blurring the editor")
        XCTAssertTrue(model.representation?.nodes.contains(where: { $0.name == "Status editor" && $0.state.focused && $0.state.value == "1😀" }) == true)
        let linkRotor = try XCTUnwrap(tree.rotors.first { $0.type == .link })
        parameters.currentItem = nil; parameters.filterString = "destination"
        let link = try XCTUnwrap(tree.rotor(linkRotor, resultFor: parameters)?.targetElement as? PageAccessibilityElement)
        XCTAssertEqual(link.accessibilityLabel(), "Rotor destination")
        model.reload()
        await wait { model.representation?.generation ?? 0 > previous.generation && model.representation?.nodes.contains(where: { $0.name == "Status editor" && $0.state.value == "1" }) == true }
        refresh(); target.setAccessibilityFocused(true)
        XCTAssertNil(tree.rotor(rotor, resultFor: parameters))
        XCTAssertEqual(fixture.requests, ["/accessibility-live", "/accessibility-live"])
        await model.stop()
    }

    private func wait(_ predicate: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
        let deadline = Date().addingTimeInterval(15)
        while !predicate() && Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Timed out waiting for real core accessibility state", file: file, line: line)
    }

    func testProtectedTextboxAllowsOrdinaryFocusWithoutValueDisclosureOrDirectValueWrites() throws {
        let snapshot = try representation { json in
            var nodes = json["nodes"] as! [[String: Any]]
            nodes[1]["role"] = "TextBox"
            var state = nodes[1]["state"] as! [String: Any]
            state["protected"] = true; state["native_text_input"] = true; state["value"] = "secret"
            nodes[1]["state"] = state; json["nodes"] = nodes
        }
        let view = NSView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        var focusActions = 0
        let tree = PageAccessibilityTree(view: view) { _, _, _, _ in focusActions += 1; return true }
        tree.update(snapshot, epoch: 1, imageSize: CGSize(width: 1000, height: 600))
        let field = try XCTUnwrap(tree.elements[2])
        XCTAssertEqual(field.accessibilitySubrole(), .secureTextField)
        XCTAssertNil(field.accessibilityValue())
        XCTAssertTrue(field.accessibilityPerformPress())
        XCTAssertEqual(focusActions, 1)
        XCTAssertFalse(field.isAccessibilitySelectorAllowed(#selector(field.setAccessibilityValue(_:))))
    }

    func testFrameRefreshSuspendsActionsAndPreservesNativeIdentity() throws {
        let snapshot = try representation()
        let view = NSView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        let tree = PageAccessibilityTree(view: view) { _, _, _, _ in true }
        tree.update(snapshot, epoch: 1, imageSize: CGSize(width: 1000, height: 600))
        let link = try XCTUnwrap(tree.elements[2])
        tree.update(nil, epoch: 1, imageSize: CGSize(width: 1000, height: 600), tab: 2)
        XCTAssertFalse(link.accessibilityPerformPress())
        XCTAssertEqual(view.accessibilityChildren()?.count, 0)
        tree.update(try representation { $0["generation"] = 8 }, epoch: 1, imageSize: CGSize(width: 1000, height: 600))
        XCTAssertTrue(tree.elements[2] === link)
        XCTAssertTrue(link.accessibilityPerformPress())
    }

    func testInvalidRepresentationDoesNotTearDownPixelConnection() throws {
        let payload = Data(#"{"request_id":8,"tab_id":2,"message":{"Representation":{"frame_source":19,"generation":7,"tab_id":2,"scroll_y":0,"nodes":[{}]}}}"#.utf8)
        let envelope = try JSONDecoder().decode(IncomingEnvelope.self, from: payload)
        if case .representationUnavailable = envelope.message {} else { XCTFail("Invalid semantics must fail soft") }
    }

    func testFlippedNativeViewportUsesBottomLeftScreenCoordinatesAndHitTesting() throws {
        let view = CorePageView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
        let window = NSWindow(contentRect: CGRect(x: 100, y: 200, width: 500, height: 300),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = view
        view.accessibilityTree.update(try representation(), epoch: 1, imageSize: CGSize(width: 1000, height: 600))
        let link = try XCTUnwrap(view.accessibilityTree.elements[2])
        let origin = window.convertToScreen(.zero).origin
        XCTAssertEqual(link.accessibilityFrame(), CGRect(x: origin.x + 10, y: origin.y + 270, width: 80, height: 20))
        let frame = link.accessibilityFrame()
        XCTAssertTrue(view.accessibilityHitTest(NSPoint(x: frame.midX, y: frame.midY)) as? PageAccessibilityElement === link)
    }

    func testReadOnlyControlStatesDoNotPromiseUnsupportedActions() throws {
        for role in ["CheckBox", "Slider", "ComboBox", "TextBox"] {
            let snapshot = try representation { json in
                var nodes = json["nodes"] as! [[String: Any]]; nodes[1]["role"] = role; json["nodes"] = nodes
            }
            let view = NSView(frame: CGRect(x: 0, y: 0, width: 500, height: 300))
            let tree = PageAccessibilityTree(view: view) { _, _, _, _ in XCTFail("Unsupported control cannot act"); return true }
            tree.update(snapshot, epoch: 1, imageSize: CGSize(width: 1000, height: 600))
            let control = try XCTUnwrap(tree.elements[2])
            XCTAssertFalse(control.accessibilityPerformPress())
            XCTAssertFalse(control.isAccessibilitySelectorAllowed(#selector(control.setAccessibilityFocused(_:))))
            XCTAssertTrue(control.isAccessibilityRequired())
        }
    }

    func testFrameSourceHashMatchesRustFNV() {
        XCTAssertEqual(PageRepresentation.frameSource(directory: ""), 0xcbf29ce484222325)
        XCTAssertEqual(PageRepresentation.frameSource(directory: "a"), 0xaf63dc4c8601ec8c)
    }
}
