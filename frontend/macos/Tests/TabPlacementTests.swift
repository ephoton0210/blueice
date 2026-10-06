// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import XCTest

@MainActor
final class TabPlacementTests: XCTestCase {
    private let viewport = WindowViewport(width: 500, height: 300, deviceScale: 1, backingScale: 1)
    private var contexts: [BrowserContextSummary] {
        [BrowserContextSummary(id: 1, name: "Default", windows: [1, 2], groups: [BrowserTabGroup(id: 7, name: "Study", color: "#4477cc", collapsed: true)]),
         BrowserContextSummary(id: 2, name: "Other", windows: [3], groups: [])]
    }
    private var windows: [BrowserWindowSummary] {
        [BrowserWindowSummary(id: 1, viewport: viewport, tabs: [BrowserTab(id: 1, url: "about:credits"), BrowserTab(id: 2, url: nil, groupID: 7)]),
         BrowserWindowSummary(id: 2, viewport: viewport, tabs: [BrowserTab(id: 3, url: nil), BrowserTab(id: 4, url: nil, groupID: 7), BrowserTab(id: 5, url: nil, groupID: 7)]),
         BrowserWindowSummary(id: 3, viewport: viewport, tabs: [BrowserTab(id: 6, url: nil)])]
    }
    func testTargetsUseDestinationGroupAndRejectClosedOrForeignOwnership() throws {
        func resolve(_ target: BrowserTabDropTarget, after: Bool = false, destination: UInt64 = 2) -> BrowserTabPlacement? {
            BrowserTabPlacement.resolve(tab: 1, source: 1, destination: destination, target: target, after: after, windows: windows, contexts: contexts)
        }
        let before = try XCTUnwrap(resolve(.tab(4))), after = try XCTUnwrap(resolve(.tab(4), after: true))
        XCTAssertEqual(before.group, 7); XCTAssertEqual(before.before, 4); XCTAssertEqual(after.before, 5)
        XCTAssertNil(try XCTUnwrap(resolve(.tab(5), after: true)).before)
        let group = try XCTUnwrap(resolve(.group(7)))
        XCTAssertEqual(group.group, 7); XCTAssertNil(group.before)
        let end = try XCTUnwrap(resolve(.ungroupedEnd))
        XCTAssertNil(end.group); XCTAssertNil(end.before)
        XCTAssertNil(resolve(.tab(6), destination: 3)); XCTAssertNil(resolve(.group(99))); XCTAssertNil(resolve(.tab(99)))
        XCTAssertNil(resolve(.ungroupedEnd, destination: 99))
        XCTAssertFalse(before.valid(windows: Array(windows.dropFirst()), contexts: contexts))
        var changed = windows
        changed[1] = BrowserWindowSummary(id: 2, viewport: viewport, tabs: [BrowserTab(id: 3, url: nil), BrowserTab(id: 4, url: nil), BrowserTab(id: 5, url: nil, groupID: 7)])
        XCTAssertFalse(before.valid(windows: changed, contexts: contexts), "Regrouped reference invalidates a delayed drop")
        let noGroup = [BrowserContextSummary(id: 1, name: "Default", windows: [1, 2], groups: []), contexts[1]]
        XCTAssertFalse(group.valid(windows: windows, contexts: noGroup))
        var reordered = windows
        reordered[1] = BrowserWindowSummary(id: 2, viewport: viewport, tabs: [BrowserTab(id: 3, url: nil), BrowserTab(id: 5, url: nil, groupID: 7), BrowserTab(id: 4, url: nil, groupID: 7)])
        XCTAssertTrue(before.valid(windows: reordered, contexts: contexts), "Before still names the live reference itself")
        XCTAssertFalse(after.valid(windows: reordered, contexts: contexts), "After cannot retain a neighbor that moved before its reference")
    }
    func testWireCapabilityDefaultsOffAndPlacementUsesExactUnsignedIDs() throws {
        let data = try JSONEncoder().encode(WindowAction.place(UInt64.max, 2, nil, 7))
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        let command = try XCTUnwrap(root["PlaceTab"] as? [String: Any])
        XCTAssertEqual((command["source_window_id"] as? NSNumber)?.uint64Value, UInt64.max)
        XCTAssertTrue(command["before_tab_id"] is NSNull)
        XCTAssertEqual((command["group_id"] as? NSNumber)?.uint64Value, 7)
        let state: [String: Any] = ["windows": [["id": 1, "viewport": ["width": 500, "height": 300, "device_scale": 1], "tabs": [["id": 1, "url": NSNull()]]]], "event": "Snapshot"]
        func decode(_ state: [String: Any]) throws -> BrowserWindowState {
            try JSONDecoder().decode(BrowserWindowState.self, from: JSONSerialization.data(withJSONObject: state))
        }
        let old = try decode(state); XCTAssertNil(old.tabPlacement); XCTAssertTrue(old.valid)
        var placed = state; placed["event"] = ["TabPlaced": ["tab_id": 1, "from_window": 1, "to_window": 1]]
        XCTAssertFalse(try decode(placed).valid)
        placed["tab_placement_v1"] = false; XCTAssertFalse(try decode(placed).valid)
        placed["tab_placement_v1"] = true; XCTAssertTrue(try decode(placed).valid)
        placed["event"] = ["TabPlaced": ["tab_id": 1, "from_window": 99, "to_window": 1]]
        XCTAssertFalse(try decode(placed).valid)
        XCTAssertEqual(BrowserStrings.text("Move Tab Left", language: "zh-Hant"), "將分頁向左移動")
        XCTAssertEqual(BrowserStrings.text("Move Tab Right", language: "zh-Hant"), "將分頁向右移動")
    }
    func testOwnedProviderRejectsReplacementForgeryReplayAndPreservesAddressDraft() async throws {
        let workspace = try await start(); defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1])
        model.action(.values("OpenTab", ["url": .string("about:settings")]))
        await wait { model.tabs.count == 2 && model.representation?.url == "about:settings" }
        let first = try XCTUnwrap(model.tabs.first?.id), last = try XCTUnwrap(model.tabs.last?.id)
        let drag = workspace.tabDragging
        let disconnected = BrowserWorkspace()
        XCTAssertTrue(disconnected.tabDragging.provider(workspace: disconnected, tab: first, source: 1).registeredTypeIdentifiers.isEmpty)
        let old = drag.provider(workspace: workspace, tab: last, source: 1)
        XCTAssertNil(drag.placement(workspace: disconnected, window: 1, target: .tab(first), after: false))
        let current = drag.provider(workspace: workspace, tab: last, source: 1)
        let rejected = await drag.drop([old], workspace: workspace, window: 1, target: .tab(first), after: false)
        XCTAssertFalse(rejected); XCTAssertEqual(model.tabs.map(\.id), [first, last])
        let foreign = NSItemProvider()
        foreign.registerDataRepresentation(forTypeIdentifier: BrowserTabDragging.type.identifier, visibility: .ownProcess) { completion in
            completion(Data(UUID().uuidString.utf8), nil); return nil
        }
        let forged = await drag.drop([foreign], workspace: workspace, window: 1, target: .tab(first), after: false)
        XCTAssertFalse(forged); XCTAssertEqual(model.tabs.map(\.id), [first, last])
        model.address = "unsent address 😀"
        let moved = await drag.drop([current], workspace: workspace, window: 1, target: .tab(first), after: false)
        XCTAssertTrue(moved); XCTAssertEqual(model.tabs.map(\.id), [last, first]); XCTAssertEqual(model.selected, last)
        XCTAssertEqual(model.address, "unsent address 😀")
        let replay = await drag.drop([current], workspace: workspace, window: 1, target: .ungroupedEnd, after: false)
        XCTAssertFalse(replay); XCTAssertEqual(model.tabs.map(\.id), [last, first])
        let cancelled = drag.provider(workspace: workspace, tab: last, source: 1); drag.cancel()
        let cancel = await drag.drop([cancelled], workspace: workspace, window: 1, target: .ungroupedEnd, after: false)
        XCTAssertFalse(cancel)
        var now = Date()
        let expiring = BrowserTabDragging(now: { now })
        let expired = expiring.provider(workspace: workspace, tab: last, source: 1)
        now = now.addingTimeInterval(91)
        let expiration = await expiring.drop([expired], workspace: workspace, window: 1, target: .ungroupedEnd, after: false)
        XCTAssertFalse(expiration); XCTAssertEqual(model.tabs.map(\.id), [last, first])
        await workspace.stop()
        XCTAssertFalse(workspace.canPlaceTabs)
    }
    func testDelayedProviderCannotMoveTabAfterItsSourceWindowChanges() async throws {
        let workspace = try await start(); defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1]), tab = try XCTUnwrap(model.selected)
        let created = await workspace.createWindow(), destination = try XCTUnwrap(created)
        let owned = workspace.tabDragging.provider(workspace: workspace, tab: tab, source: 1)
        let bytes: Data? = try await withCheckedThrowingContinuation { continuation in
            owned.loadDataRepresentation(forTypeIdentifier: BrowserTabDragging.type.identifier) { data, error in
                if let error { continuation.resume(throwing: error) } else { continuation.resume(returning: data) }
            }
        }
        let delayed = NSItemProvider(), loading = expectation(description: "Real provider starts loading")
        var complete: ((Data?, Error?) -> Void)?
        delayed.registerDataRepresentation(forTypeIdentifier: BrowserTabDragging.type.identifier, visibility: .ownProcess) { callback in
            DispatchQueue.main.async { complete = callback; loading.fulfill() }; return nil
        }
        let pending = Task { await workspace.tabDragging.drop([delayed], workspace: workspace, window: destination, target: .ungroupedEnd, after: false) }
        await fulfillment(of: [loading], timeout: 5)
        let moved = await workspace.moveTab(tab, to: destination); XCTAssertTrue(moved)
        complete?(bytes, nil)
        let result = await pending.value; XCTAssertFalse(result)
        XCTAssertFalse(workspace.tabDragging.isDragging, "An authenticated stale drop must clear its insertion indicators")
        XCTAssertFalse(model.tabs.contains { $0.id == tab })
        XCTAssertTrue(workspace.models[destination]?.tabs.contains { $0.id == tab } == true)
        XCTAssertNil(workspace.tabDragging.placement(workspace: workspace, window: 1, target: .ungroupedEnd, after: false))
        await workspace.stop()
    }
    func testDelayedProviderRejectsReorderedReferenceWithoutMovingAnotherTab() async throws {
        let workspace = try await start(); defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1])
        model.action(.values("OpenTab", ["url": .string("about:settings")]))
        await wait { model.tabs.count == 2 && model.representation?.url == "about:settings" }
        model.action(.values("OpenTab", ["url": .string("about:credits")]))
        await wait { model.tabs.count == 3 && model.representation?.url == "about:credits" }
        let first = model.tabs[0].id, second = model.tabs[1].id, third = model.tabs[2].id
        let owned = workspace.tabDragging.provider(workspace: workspace, tab: third, source: 1)
        let loaded: Data? = try await withCheckedThrowingContinuation { continuation in
            owned.loadDataRepresentation(forTypeIdentifier: BrowserTabDragging.type.identifier) { data, error in
                if let error { continuation.resume(throwing: error) } else { continuation.resume(returning: data) }
            }
        }
        let bytes = try XCTUnwrap(loaded), delayed = NSItemProvider()
        let loading = expectation(description: "Owned drop captures the original after-reference anchor")
        var complete: ((Data?, Error?) -> Void)?
        delayed.registerDataRepresentation(forTypeIdentifier: BrowserTabDragging.type.identifier, visibility: .ownProcess) { callback in
            DispatchQueue.main.async { complete = callback; loading.fulfill() }; return nil
        }
        let pending = Task { await workspace.tabDragging.drop([delayed], workspace: workspace, window: 1, target: .tab(first), after: true) }
        await fulfillment(of: [loading], timeout: 5)
        let reordered = try XCTUnwrap(BrowserTabPlacement.resolve(tab: first, source: 1, destination: 1,
            target: .ungroupedEnd, after: false, windows: workspace.windows, contexts: workspace.contexts))
        let moved = await workspace.placeTab(reordered); XCTAssertTrue(moved)
        XCTAssertEqual(model.tabs.map(\.id), [second, third, first])
        complete?(bytes, nil)
        let result = await pending.value
        XCTAssertFalse(result, "The captured after-reference anchor is stale even though all three tabs remain live")
        XCTAssertEqual(model.tabs.map(\.id), [second, third, first], "A late provider cannot reposition a tab using the old neighbor")
        XCTAssertEqual(model.selected, third)
        XCTAssertFalse(workspace.tabDragging.isDragging)
        await workspace.stop()
    }
    private func start() async throws -> BrowserWorkspace {
        let domain = "cc.blueice.tab-placement-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain))
        addTeardownBlock { defaults.removePersistentDomain(forName: domain) }
        let workspace = BrowserWorkspace(contextDefaults: defaults)
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await workspace.start(launcher: launcher)
        await wait { workspace.canPlaceTabs && workspace.models[1]?.representation?.url == "about:credits" }
        return workspace
    }
    private func wait(_ predicate: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
        let deadline = Date().addingTimeInterval(15)
        while !predicate(), Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), "Actual core acknowledgement", file: file, line: line)
    }
}
