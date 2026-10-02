// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Carbon
import ApplicationServices
import XCTest

@MainActor
final class BrowserUITests: XCTestCase {
    private var app: XCUIApplication!
    private var originalInputSource: TISInputSource?

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        originalInputSource = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
        let filter = [kTISPropertyInputSourceID as String: "com.apple.keylayout.ABC"] as CFDictionary
        let sources = TISCreateInputSourceList(filter, false).takeRetainedValue() as! [TISInputSource]
        if let source = sources.first { XCTAssertEqual(TISSelectInputSource(source), noErr) }
    }

    override func tearDownWithError() throws {
        defer { if let originalInputSource { XCTAssertEqual(TISSelectInputSource(originalInputSource), noErr) } }
        if app.state != .notRunning {
            if app.windows.firstMatch.exists { app.windows.firstMatch.buttons[XCUIIdentifierCloseWindow].click() }
            XCTAssertTrue(app.wait(for: .notRunning, timeout: 10), "Normal close must stop the owned services")
            app.terminate()
        }
    }

    private func launch() {
        app.launch()
        XCTAssertTrue(app.textFields["address"].waitForExistence(timeout: 15))
        waitValue(app.textFields["address"], "about:credits")
        XCTAssertTrue(app.groups["page"].waitForExistence(timeout: 15), app.debugDescription)
    }

    private func waitValue(_ element: XCUIElement, _ value: String) {
        let predicate = NSPredicate(format: "value == %@", value)
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: element)], timeout: 15), .completed,
                       "Expected \(value), received \(String(describing: element.value))")
    }

    private func enter(_ text: String, submit: Bool = true) {
        let address = app.textFields["address"]
        address.click()
        app.menuBars.menuBarItems["Edit"].click()
        app.menuItems["Input Source"].hover()
        app.menuItems["ABC"].click()
        address.typeKey("a", modifierFlags: .command)
        address.typeText(text)
        if submit { address.typeKey(.return, modifierFlags: []) }
    }

    func testRetinaCSSSizeZoomShortcutsAndPerTabRetention() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard()
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/context-menu")
        let field = app.groups["page"].textFields["Context editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        waitValue(app.buttons["page-zoom"], "100%")
        XCTAssertEqual(field.frame.width, 280, accuracy: 3, "CSS width must be in native points at 100%, independent of Retina density")
        let page = app.groups["page"]
        let bitmap = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(page.screenshot().image.tiffRepresentation)))
        XCTAssertGreaterThan(bitmap.pixelsWide, Int(page.frame.width), "This acceptance host must exercise high-density output")
        let rendered = page.value as? String ?? ""
        XCTAssertTrue(rendered.hasPrefix("Rendered \(bitmap.pixelsWide) ×"), rendered)
        app.typeKey("=", modifierFlags: .command)
        waitValue(app.buttons["page-zoom"], "110%")
        XCTAssertEqual(field.frame.width, 308, accuracy: 3)
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Page Zoom"].hover(); app.menuItems["150%"].click()
        waitValue(app.buttons["page-zoom"], "150%")
        XCTAssertEqual(field.frame.width, 420, accuracy: 3)
        field.click(); field.typeKey("a", modifierFlags: .command)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("Scaled 中文", forType: .string)
        field.typeKey("v", modifierFlags: .command)
        waitValue(field, "Scaled 中文")
        field.rightClick(); chooseContext("Select All")
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        waitValue(app.buttons["page-zoom"], "100%")
        app.buttons["tab-1"].click(); waitValue(app.buttons["page-zoom"], "150%")
        waitValue(field, "Scaled 中文")
        app.buttons["reload"].click(); waitValue(field, "hello"); waitValue(app.buttons["page-zoom"], "150%")
        app.typeKey("-", modifierFlags: .command); waitValue(app.buttons["page-zoom"], "125%")
        app.typeKey("0", modifierFlags: .command); waitValue(app.buttons["page-zoom"], "100%")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-retina-viewport"; attachment.lifetime = .keepAlways; add(attachment)
        XCTAssertEqual(fixture.requests, ["/context-menu", "/context-menu"])
    }

    func testFullScreenUpdatesViewportAndRestoresNativeWindow() {
        launch()
        let window = app.windows["browser-window"], before = window.frame
        app.typeKey("f", modifierFlags: [.command, .control])
        let expanded = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in window.frame.width > before.width + 100 }, object: window)
        XCTAssertEqual(XCTWaiter.wait(for: [expanded], timeout: 15), .completed)
        XCTAssertTrue((app.groups["page"].value as? String ?? "").hasPrefix("Rendered"))
        app.typeKey("f", modifierFlags: [.command, .control])
        let restored = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in abs(window.frame.width - before.width) < 3 }, object: window)
        XCTAssertEqual(XCTWaiter.wait(for: [restored], timeout: 15), .completed)
        XCTAssertTrue(window.buttons[XCUIIdentifierCloseWindow].isHittable)
    }

    private func saveClipboard() -> [NSPasteboardItem] {
        NSPasteboard.general.pasteboardItems?.map { item in
            let copy = NSPasteboardItem()
            for type in item.types { if let data = item.data(forType: type) { copy.setData(data, forType: type) } }
            return copy
        } ?? []
    }

    private func chooseContext(_ title: String) {
        let item = app.groups["page"].menuItems[title]
        XCTAssertTrue(item.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(item.isEnabled, title)
        item.click()
    }

    func testContextLinkCopyNewTabHistoryAndPolicyDenial() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard()
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/context-menu")
        waitValue(app.textFields["address"], fixture.origin + "/context-menu")
        let link = app.groups["page"].links["Destination link"]
        XCTAssertTrue(link.waitForExistence(timeout: 15))
        link.rightClick()
        XCTAssertTrue(app.groups["page"].menuItems["Copy Link Address"].waitForExistence(timeout: 10))
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "macos-context-menu"; attachment.lifetime = .keepAlways; add(attachment)
        chooseContext("Copy Link Address")
        let copied = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            NSPasteboard.general.string(forType: .string) == fixture.origin + "/destination"
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [copied], timeout: 10), .completed)
        XCTAssertEqual(fixture.requests, ["/context-menu"])
        link.rightClick(); chooseContext("Open Link in New Tab")
        waitValue(app.textFields["address"], fixture.origin + "/destination")
        XCTAssertTrue(app.buttons["tab-2"].waitForExistence(timeout: 10))
        XCTAssertEqual(fixture.requests, ["/context-menu", "/destination"])
        app.buttons["tab-1"].click(); waitValue(app.textFields["address"], fixture.origin + "/context-menu")
        link.rightClick(); chooseContext("Open Link")
        waitValue(app.textFields["address"], fixture.origin + "/destination")
        app.groups["page"].coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.8)).rightClick()
        chooseContext("Back"); waitValue(app.textFields["address"], fixture.origin + "/context-menu")
        let blocked = app.groups["page"].links["Blocked link"]
        XCTAssertTrue(blocked.waitForExistence(timeout: 10))
        blocked.rightClick(); chooseContext("Open Link in New Tab")
        let newDenied = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS 'Navigation blocked' OR label CONTAINS 'Navigation blocked'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [newDenied], timeout: 15), .completed)
        XCTAssertEqual(app.textFields["address"].value as? String, fixture.origin + "/context-menu")
        XCTAssertFalse(app.buttons["tab-3"].exists, "A denied destination must not leave a phantom native tab")
        blocked.rightClick(); chooseContext("Open Link")
        let denied = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS 'Navigation blocked' OR label CONTAINS 'Navigation blocked'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [denied], timeout: 15), .completed)
        XCTAssertEqual(app.textFields["address"].value as? String, fixture.origin + "/context-menu")
        let finished = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in fixture.requests.count == 6 }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [finished], timeout: 10), .completed)
        XCTAssertEqual(fixture.requests, ["/context-menu", "/destination", "/destination", "/context-menu", "/blocked", "/blocked"])
    }

    func testContextEditingReadonlyPasswordAndKeyboardMenu() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard()
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/context-menu")
        let page = app.groups["page"], field = page.textFields["Context editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        field.rightClick(); chooseContext("Select All")
        field.rightClick(); chooseContext("Cut"); waitValue(field, "")
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "hello")
        field.rightClick(); chooseContext("Paste"); waitValue(field, "hello")
        field.typeKey(.F10, modifierFlags: .shift)
        XCTAssertTrue(page.menuItems["Copy"].waitForExistence(timeout: 10), "Shift-F10 opens the focused editor's native menu")
        app.typeKey(.escape, modifierFlags: [])
        XCUIElement.perform(withKeyModifiers: .control) { field.click() }
        XCTAssertTrue(page.menuItems["Select All"].waitForExistence(timeout: 10), "Control-click opens the native context menu")
        app.typeKey(.escape, modifierFlags: [])
        let readonly = page.textFields["Context readonly"]
        readonly.rightClick(); chooseContext("Select All")
        readonly.rightClick()
        XCTAssertFalse(page.menuItems["Cut"].isEnabled)
        XCTAssertFalse(page.menuItems["Paste"].isEnabled)
        chooseContext("Copy")
        let copied = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            NSPasteboard.general.string(forType: .string) == "read only"
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [copied], timeout: 10), .completed)
        let secret = page.secureTextFields["Context secret"]
        secret.rightClick(); chooseContext("Select All")
        secret.rightClick()
        XCTAssertFalse(page.menuItems["Copy"].isEnabled)
        XCTAssertFalse(page.menuItems["Cut"].isEnabled)
        XCTAssertTrue(page.menuItems["Paste"].isEnabled)
        XCTAssertFalse(app.debugDescription.contains("private-menu-secret"))
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "read only")
        page.textFields["Context disabled"].rightClick()
        XCTAssertTrue(page.menuItems["Find in Page…"].waitForExistence(timeout: 10))
        XCTAssertFalse(page.menuItems["Paste"].exists)
        chooseContext("Find in Page…")
        XCTAssertTrue(app.searchFields["find-query"].waitForExistence(timeout: 10))
        XCTAssertEqual(fixture.requests, ["/context-menu"])
    }

    func testFindKeyboardWrapScrollCaseAndClose() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch(); enter(fixture.origin + "/find")
        waitValue(app.textFields["address"], fixture.origin + "/find")
        app.typeKey("f", modifierFlags: .command)
        let query = app.searchFields["find-query"]
        XCTAssertTrue(query.waitForExistence(timeout: 10))
        query.typeText("frost")
        waitFind("1 of 4")
        app.typeKey("g", modifierFlags: [.command, .shift])
        waitFind("4 of 4 · Wrapped")
        let lower = app.groups["page"].descendants(matching: .any).matching(NSPredicate(format: "label == 'Lower frost' OR value == 'Lower frost'")).firstMatch
        XCTAssertTrue(lower.waitForExistence(timeout: 10), "Find Previous must scroll to the last result")
        XCTAssertTrue(lower.frame.intersects(app.groups["page"].frame))
        let bitmap = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(app.groups["page"].screenshot().image.tiffRepresentation)))
        var orange = 0
        for y in stride(from: 0, to: bitmap.pixelsHigh, by: 2) {
            for x in stride(from: 0, to: bitmap.pixelsWide, by: 2) {
                if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB), color.redComponent > 0.85,
                   (0.35...0.75).contains(color.greenComponent), color.blueComponent < 0.3 { orange += 1 }
            }
        }
        XCTAssertGreaterThan(orange, 10, "Current match must have visible core-rendered orange pixels")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-find-in-page"; attachment.lifetime = .keepAlways; add(attachment)
        app.typeKey("g", modifierFlags: .command)
        waitFind("1 of 4 · Wrapped")
        app.checkBoxes["find-case"].click()
        waitFind("1 of 3")
        app.buttons["find-next"].click(); waitFind("2 of 3")
        app.buttons["find-previous"].click(); waitFind("1 of 3")
        query.click(); query.typeKey(.return, modifierFlags: []); waitFind("2 of 3")
        query.typeKey(.return, modifierFlags: .shift); waitFind("1 of 3")
        app.typeKey("f", modifierFlags: .command)
        query.typeText("snow crystal")
        waitValue(query, "snow crystal"); waitFind("1 of 1")
        query.typeKey(.escape, modifierFlags: [])
        XCTAssertFalse(query.exists)
        XCTAssertEqual(fixture.requests.filter { $0 == "/find" }.count, 1)
    }

    func testFindUnicodePrivacyTabsAndNavigationInvalidation() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch(); enter(fixture.origin + "/find")
        waitValue(app.textFields["address"], fixture.origin + "/find")
        app.typeKey("f", modifierFlags: .command)
        let query = app.searchFields["find-query"]
        XCTAssertTrue(query.waitForExistence(timeout: 10))
        for (text, expected) in [("snow crystal", "1 of 1"), ("café", "1 of 2"), ("σ", "1 of 3"), ("[a.*]", "1 of 1"), ("private-find-secret", "No matches"), ("hidden-find-secret", "No matches")] {
            query.click(); query.typeKey("a", modifierFlags: .command); query.typeText(text); waitFind(expected)
        }
        app.buttons["add-tab"].click()
        waitValue(app.textFields["address"], "about:credits")
        XCTAssertFalse(query.exists)
        app.typeKey("f", modifierFlags: .command)
        XCTAssertTrue(query.waitForExistence(timeout: 10)); query.typeText("missing-other-tab"); waitFind("No matches")
        app.buttons["tab-1"].click()
        waitValue(query, "hidden-find-secret"); waitFind("No matches")
        app.buttons["find-close"].click()
        XCTAssertFalse(query.exists)
        app.buttons["settings"].click(); waitValue(app.textFields["address"], "about:settings")
        app.typeKey("f", modifierFlags: .command)
        XCTAssertTrue(query.waitForExistence(timeout: 10)); waitValue(query, "")
    }

    private func waitFind(_ summary: String) {
        let result = app.staticTexts["find-results"]
        XCTAssertTrue(result.waitForExistence(timeout: 10))
        let expected = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@ OR value == %@", summary, summary), object: result)
        XCTAssertEqual(XCTWaiter.wait(for: [expected], timeout: 15), .completed, app.debugDescription)
    }

    func testVisibleCorePixelsAndNativeWindow() throws {
        launch()
        let page = app.groups["page"]
        let drawn = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value BEGINSWITH 'Rendered'"), object: page)
        XCTAssertEqual(XCTWaiter.wait(for: [drawn], timeout: 15), .completed)
        let shot = page.screenshot()
        let bitmap = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(shot.image.tiffRepresentation)))
        var dark = 0
        for y in stride(from: 0, to: bitmap.pixelsHigh, by: 3) {
            for x in stride(from: 0, to: bitmap.pixelsWide, by: 3) {
                if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB),
                   color.redComponent < 0.7, color.greenComponent < 0.7, color.blueComponent < 0.7 { dark += 1 }
            }
        }
        XCTAssertGreaterThan(dark, 100, "The viewport must contain visible core-rendered content")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-swiftui-appkit"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testSettingsBackAndForward() {
        launch()
        app.buttons["settings"].click()
        waitValue(app.textFields["address"], "about:settings")
        XCTAssertTrue(app.buttons["back"].isEnabled)
        app.buttons["back"].click()
        waitValue(app.textFields["address"], "about:credits")
        app.buttons["forward"].click()
        waitValue(app.textFields["address"], "about:settings")
    }

    func testResizePublishesNewCoreFrame() {
        launch()
        let page = app.groups["page"]
        let rendered = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value BEGINSWITH 'Rendered'"), object: page)
        XCTAssertEqual(XCTWaiter.wait(for: [rendered], timeout: 15), .completed)
        let before = (page.value as? String)?.components(separatedBy: " ×").first
        let oldWidth = page.frame.width
        let window = app.windows.firstMatch
        let corner = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
            .withOffset(CGVector(dx: -2, dy: -2))
        corner.click(forDuration: 0.2, thenDragTo: corner.withOffset(CGVector(dx: -120, dy: -100)))
        let changed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let value = page.value as? String else { return false }
            return value.hasPrefix("Rendered") && value.components(separatedBy: " ×").first != before
                && page.frame.width < oldWidth - 60
        }, object: page)
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 15), .completed)
    }

    func testReloadUsesCommittedURL() {
        launch()
        enter("about:settings", submit: false)
        app.buttons["reload"].click()
        waitValue(app.textFields["address"], "about:credits")
    }

    func testTabsRetainIndependentNavigationAndCloseLastTab() {
        launch()
        app.buttons["settings"].click()
        waitValue(app.textFields["address"], "about:settings")
        app.buttons["add-tab"].click()
        XCTAssertTrue(app.buttons["tab-2"].waitForExistence(timeout: 15))
        waitValue(app.textFields["address"], "about:credits")
        app.buttons["tab-1"].click()
        waitValue(app.textFields["address"], "about:settings")
        app.buttons["close-tab-2"].click()
        XCTAssertFalse(app.buttons["tab-2"].exists)
        app.buttons["close-tab-1"].click()
        XCTAssertTrue(app.staticTexts["empty-page"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.buttons["reload"].isEnabled)
        app.buttons["add-tab"].click()
        waitValue(app.textFields["address"], "about:credits")
    }

    func testExternalNavigationFailsClosedVisibly() {
        launch()
        enter("http://malware.test/")
        let denied = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS[c] 'blocked' OR label CONTAINS[c] 'blocked'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [denied], timeout: 15), .completed, app.debugDescription)
        app.buttons["reload"].click()
        waitValue(app.textFields["address"], "about:credits")
    }

    func testReviewedHTTPContentHistoryReloadAndDenial() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch()
        let url = fixture.origin + "/safe"
        enter(url)
        waitValue(app.textFields["address"], url)
        let page = app.groups["page"]
        let pixels = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let bitmap = NSBitmapImageRep(data: page.screenshot().image.tiffRepresentation ?? Data()) else { return false }
            for y in stride(from: 10, to: bitmap.pixelsHigh, by: 40) {
                for x in stride(from: 10, to: bitmap.pixelsWide, by: 40) {
                    if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB),
                       color.redComponent < 0.25, color.greenComponent > 0.35, color.greenComponent < 0.65,
                       color.blueComponent < 0.35 { return true }
                }
            }
            return false
        }, object: page)
        XCTAssertEqual(XCTWaiter.wait(for: [pixels], timeout: 15), .completed)
        app.buttons["back"].click()
        waitValue(app.textFields["address"], "about:credits")
        app.buttons["forward"].click()
        waitValue(app.textFields["address"], url)
        enter("about:settings", submit: false)
        let beforeReload = fixture.requests.count
        app.buttons["reload"].click()
        waitValue(app.textFields["address"], url)
        XCTAssertGreaterThan(fixture.requests.count, beforeReload)
        enter(fixture.origin + "/blocked")
        let denied = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS[c] 'blocked' OR label CONTAINS[c] 'blocked'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [denied], timeout: 15), .completed)
        app.buttons["reload"].click()
        waitValue(app.textFields["address"], url)
        XCTAssertEqual(fixture.requests.filter { $0 == "/blocked" }.count, 1)
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-reviewed-http"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testNativePageSemanticsTypingPrivacyAndTabIsolation() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch()
        enter(fixture.origin + "/accessibility")
        let page = app.groups["page"]
        let heading = page.descendants(matching: .any).matching(NSPredicate(format: "label == 'Accessibility fixture' OR value == 'Accessibility fixture'")).firstMatch
        XCTAssertTrue(heading.waitForExistence(timeout: 15), app.debugDescription)
        XCTAssertTrue(page.staticTexts["Readable page text"].exists)
        XCTAssertTrue(page.links["Open destination"].exists)
        XCTAssertTrue(page.images["Fixture logo"].exists)
        XCTAssertTrue(page.checkBoxes["Remember"].exists)
        XCTAssertFalse(page.textFields["Locked"].isEnabled)
        XCTAssertFalse(page.descendants(matching: .any).matching(NSPredicate(format: "label == 'Hidden fixture heading'")).firstMatch.exists)
        let secret = page.secureTextFields["Secret"]
        XCTAssertTrue(secret.exists, app.debugDescription)
        XCTAssertFalse(String(describing: secret.value).contains("private-fixture-secret"))
        XCTAssertFalse(app.debugDescription.contains("private-fixture-secret"))
        let name = page.textFields["Name"]
        waitValue(name, "hello")
        name.click()
        name.typeText(" world")
        waitValue(page.textFields["Name"], "hello world")
        app.buttons["add-tab"].click()
        waitValue(app.textFields["address"], "about:credits")
        XCTAssertFalse(page.textFields["Name"].exists)
        app.buttons["tab-1"].click()
        waitValue(page.textFields["Name"], "hello world")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-page-accessibility"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testAccessibleLinkNavigationDropsOldDocumentElements() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch()
        enter(fixture.origin + "/accessibility")
        let link = app.groups["page"].links["Open destination"]
        XCTAssertTrue(link.waitForExistence(timeout: 15), app.debugDescription)
        XCTAssertGreaterThan(link.frame.width, 0)
        XCTAssertTrue(app.groups["page"].frame.intersects(link.frame))
        link.click()
        waitValue(app.textFields["address"], fixture.origin + "/destination")
        let destination = app.groups["page"].descendants(matching: .any).matching(NSPredicate(format: "label == 'Destination reached' OR value == 'Destination reached'")).firstMatch
        XCTAssertTrue(destination.waitForExistence(timeout: 15), app.debugDescription)
        XCTAssertFalse(app.groups["page"].textFields["Name"].exists)
        XCTAssertFalse(link.exists)
        XCTAssertEqual(fixture.requests, ["/accessibility", "/destination"])
    }

    func testKeyboardOnlyNativeControlsAndLinkActivation() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch()
        enter(fixture.origin + "/keyboard")
        let page = app.groups["page"]
        XCTAssertTrue(page.textFields["Name"].waitForExistence(timeout: 15))
        // After submitting the address, every page interaction is a key.
        app.typeKey(.tab, modifierFlags: [])
        app.typeText("Alice")
        XCTAssertEqual(app.textFields["address"].value as? String, fixture.origin + "/keyboard", "Page typing must leave the address unchanged")
        waitValue(page.textFields["Name"], "Alice")
        app.typeKey(.tab, modifierFlags: [])
        app.typeText("bad")
        waitValue(page.textFields["Readonly"], "locked")
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(" ", modifierFlags: [])
        let remembered = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == 1 OR value == '1'"), object: page.checkBoxes["Remember"])
        XCTAssertEqual(XCTWaiter.wait(for: [remembered], timeout: 15), .completed)
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.rightArrow, modifierFlags: [])
        let express = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == 1 OR value == '1'"), object: page.radioButtons["Express"])
        XCTAssertEqual(XCTWaiter.wait(for: [express], timeout: 15), .completed)
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.downArrow, modifierFlags: [])
        waitValue(page.comboBoxes["Region"], "b")
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.rightArrow, modifierFlags: [])
        waitValue(page.sliders["Level"], "0.5")
        XCTAssertFalse(page.textFields["Disabled"].isEnabled)
        XCTAssertFalse(page.textFields["Hidden"].exists)
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-keyboard-controls"; attachment.lifetime = .keepAlways; add(attachment)
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.return, modifierFlags: [])
        waitValue(app.textFields["address"], fixture.origin + "/destination")
        XCTAssertEqual(fixture.requests, ["/keyboard", "/destination"])
    }

    func testKeyboardOnlyFormResetRestoresAllControlsAndAssociatedValues() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch(); enter(fixture.origin + "/reset")
        let page = app.groups["page"]
        XCTAssertTrue(page.textFields["Name"].waitForExistence(timeout: 15))
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey("a", modifierFlags: .command); app.typeText("edited name")
        waitValue(page.textFields["Name"], "edited name")
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey("a", modifierFlags: .command); app.typeText("edited notes")
        waitValue(page.textFields["Notes"], "edited notes")
        app.typeKey(.tab, modifierFlags: []); app.typeKey(" ", modifierFlags: [])
        waitChecked(page.checkBoxes["Remember"], false)
        app.typeKey(.tab, modifierFlags: []); app.typeKey(.rightArrow, modifierFlags: [])
        waitChecked(page.radioButtons["Express"], true)
        app.typeKey(.tab, modifierFlags: []); app.typeKey(.downArrow, modifierFlags: [])
        waitValue(page.comboBoxes["Region"], "b")
        app.typeKey(.tab, modifierFlags: []); app.typeKey(.rightArrow, modifierFlags: [])
        waitValue(page.sliders["Level"], "26")
        app.typeKey(.tab, modifierFlags: []); app.typeText("bad")
        waitValue(page.textFields["Readonly"], "locked")
        XCTAssertFalse(page.buttons["Disabled reset"].isEnabled)
        app.typeKey(.tab, modifierFlags: []); app.typeKey(.return, modifierFlags: [])
        waitValue(page.textFields["Name"], "A😀B")
        waitValue(page.textFields["Notes"], "first\nsecond")
        waitChecked(page.checkBoxes["Remember"], true)
        waitChecked(page.radioButtons["Standard"], true)
        waitChecked(page.radioButtons["Express"], false)
        waitValue(page.comboBoxes["Region"], "a")
        waitValue(page.sliders["Level"], "25")
        let recovered = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == 'Ready' OR label == 'Ready'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [recovered], timeout: 15), .completed, "Successful reset must clear the previous readonly editing error")
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey("a", modifierFlags: .command); app.typeText("edited outside")
        waitValue(page.textFields["External"], "edited outside")
        app.typeKey(.tab, modifierFlags: []); app.typeKey(" ", modifierFlags: [])
        waitValue(page.textFields["External"], "outside")
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey("a", modifierFlags: .command); app.typeText("keep other")
        waitValue(page.textFields["Other form"], "keep other")
        app.typeKey(.tab, modifierFlags: .shift); app.typeKey(.return, modifierFlags: [])
        waitValue(page.textFields["Other form"], "keep other")
        waitValue(page.textFields["Name"], "A😀B")
        XCTAssertEqual(app.textFields["address"].value as? String, fixture.origin + "/reset")
        XCTAssertEqual(fixture.requests, ["/reset"])
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-native-form-reset"; attachment.lifetime = .keepAlways; add(attachment)
    }

    private func waitChecked(_ element: XCUIElement, _ checked: Bool) {
        let predicate = NSPredicate { _, _ in
            if let number = element.value as? NSNumber { return number.boolValue == checked }
            return (element.value as? String) == (checked ? "1" : "0")
        }
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: element)], timeout: 15), .completed)
    }

    func testPointerResetKeepsOtherTabsAndReloadDefaultsIndependent() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch(); enter(fixture.origin + "/reset")
        let page = app.groups["page"]
        func replace(_ label: String, _ value: String) {
            let field = page.textFields[label]
            XCTAssertTrue(field.waitForExistence(timeout: 15))
            field.click(); field.typeKey("a", modifierFlags: .command); field.typeText(value)
            waitValue(field, value)
        }
        replace("Name", "first tab")
        replace("External", "first outside")
        replace("Other form", "keep other")
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        enter(fixture.origin + "/reset")
        replace("Name", "second tab")
        page.buttons["Reset form"].click()
        waitValue(page.textFields["Name"], "A😀B")
        app.buttons["tab-1"].click()
        waitValue(page.textFields["Name"], "first tab")
        waitValue(page.textFields["External"], "first outside")
        page.buttons["Disabled reset"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
        waitValue(page.textFields["Name"], "first tab")
        XCTAssertTrue(page.buttons["Reset external"].exists, app.debugDescription)
        page.buttons["Reset external"].click()
        waitValue(page.textFields["Name"], "A😀B")
        waitValue(page.textFields["External"], "outside")
        waitValue(page.textFields["Other form"], "keep other")
        replace("Name", "third value")
        app.buttons["reload"].click()
        waitValue(page.textFields["Name"], "A😀B")
        replace("Name", "fourth value")
        page.buttons["Reset form"].click()
        waitValue(page.textFields["Name"], "A😀B")
        XCTAssertEqual(fixture.requests, ["/reset", "/reset", "/reset"])
    }

    func testReverseTabAndPageBoundaryReturnToNativeAddressEditing() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch()
        let page = app.groups["page"]
        for _ in 0..<3 {
            enter(fixture.origin + "/keyboard")
            XCTAssertTrue(page.textFields["Name"].waitForExistence(timeout: 15))
            waitValue(page.textFields["Name"], "")
            app.typeKey(.tab, modifierFlags: [])
            app.typeText("A")
            XCTAssertEqual(app.textFields["address"].value as? String, fixture.origin + "/keyboard", "Page typing must leave the address unchanged")
            waitValue(page.textFields["Name"], "A")
            app.typeKey(.tab, modifierFlags: [])
            app.typeKey(.tab, modifierFlags: .shift)
            app.typeText("B")
            waitValue(page.textFields["Name"], "AB")
            app.typeKey(.tab, modifierFlags: .shift)
            app.typeText("chrome-probe")
            let address = app.textFields["address"]
            let chrome = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS 'chrome-probe'"), object: address)
            XCTAssertEqual(XCTWaiter.wait(for: [chrome], timeout: 15), .completed,
                           "Reverse Tab must reach native address editing; received \(String(describing: address.value))")
        }
        app.typeKey("l", modifierFlags: .command)
        app.typeKey("a", modifierFlags: .command)
        app.typeText(fixture.origin + "/destination")
        app.typeKey(.return, modifierFlags: [])
        waitValue(app.textFields["address"], fixture.origin + "/destination")
        XCTAssertEqual(fixture.requests, ["/keyboard", "/keyboard", "/keyboard", "/destination"])
    }

    func testNativeSelectionReplacementDeletionAndMultilineClipboard() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        // Preserve every existing pasteboard representation without logging it.
        let saved = NSPasteboard.general.pasteboardItems?.map { item -> NSPasteboardItem in
            let copy = NSPasteboardItem()
            for type in item.types { if let data = item.data(forType: type) { copy.setData(data, forType: type) } }
            return copy
        } ?? []
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch()
        enter(fixture.origin + "/editing")
        let page = app.groups["page"]
        let field = page.textFields["Editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 15), app.debugDescription)
        field.click()
        field.typeKey("a", modifierFlags: .command)
        field.typeText("hello")
        waitValue(field, "hello")
        field.typeKey(.leftArrow, modifierFlags: [.shift, .command])
        field.typeText("world")
        waitValue(field, "world")
        field.typeKey(.delete, modifierFlags: [])
        waitValue(field, "worl")
        field.typeKey("a", modifierFlags: .command)
        field.typeKey("c", modifierFlags: .command)
        let copied = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            NSPasteboard.general.string(forType: .string) == "worl"
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [copied], timeout: 15), .completed, "Copy must complete after core selection acknowledgement")
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "worl")
        field.typeKey("x", modifierFlags: .command)
        waitValue(field, "")
        field.typeKey("v", modifierFlags: .command)
        waitValue(field, "worl")
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString("中文\nשלום\n👩‍👩‍👧‍👦", forType: .string)
        let notes = page.textFields["Notes"]
        XCTAssertTrue(notes.exists, app.debugDescription)
        notes.click()
        notes.typeKey("a", modifierFlags: .command)
        notes.typeKey("v", modifierFlags: .command)
        waitValue(notes, "中文\nשלום\n👩‍👩‍👧‍👦")
        notes.typeKey(.delete, modifierFlags: [])
        waitValue(notes, "中文\nשלום\n")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-native-editing"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testNativeProtectedAndReadonlyFields() throws {
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch()
        enter(fixture.origin + "/editing")
        let page = app.groups["page"]
        let secret = page.secureTextFields["Secret"]
        XCTAssertTrue(secret.waitForExistence(timeout: 15), app.debugDescription)
        secret.click()
        secret.typeKey("a", modifierFlags: .command)
        secret.typeText("new-secret")
        XCTAssertFalse(String(describing: secret.value).contains("new-secret"))
        XCTAssertFalse(app.debugDescription.contains("private-fixture-secret"))
        XCTAssertFalse(app.debugDescription.contains("new-secret"))
        let readonly = page.textFields["Readonly"]
        readonly.click()
        readonly.typeKey("a", modifierFlags: .command)
        readonly.typeText("bad")
        waitValue(readonly, "locked")
        XCTAssertFalse(page.textFields["Disabled"].isEnabled)
    }

    func testSystemZhuyinInputMethodCommitsAndCancelsComposition() throws {
        guard AXIsProcessTrusted() else {
            throw XCTSkip("Grant Accessibility to BrowserUITests-Runner.app to permit physical IME key events; XCUITest string keys cannot verify Zhuyin hardware mapping")
        }
        let fixture = try HTTPFixture()
        defer { fixture.stop() }
        launch()
        enter(fixture.origin + "/editing")
        let field = app.groups["page"].textFields["Editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        field.click()
        field.typeKey("a", modifierFlags: .command)
        field.typeKey(.delete, modifierFlags: [])
        waitValue(field, "")
        let previous = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
        let filter = [kTISPropertyInputSourceID as String: "com.apple.inputmethod.TCIM.Zhuyin"] as CFDictionary
        let sources = TISCreateInputSourceList(filter, false).takeRetainedValue() as! [TISInputSource]
        guard let source = sources.first else { throw XCTSkip("An enabled system Zhuyin input source is required") }
        defer { XCTAssertEqual(TISSelectInputSource(previous), noErr, "Restore the user's input source") }
        XCTAssertEqual(TISSelectInputSource(source), noErr)
        app.menuBars.menuBarItems["Edit"].click()
        app.menuItems["Input Source"].hover()
        // SwiftUI native command items expose their localized title through
        // AX; the English app and Traditional Chinese runner differ here.
        let englishSource = app.menuItems["Zhuyin – Traditional"]
        let sourceItem = englishSource.waitForExistence(timeout: 3) ? englishSource : app.menuItems["繁體注音"]
        XCTAssertTrue(sourceItem.waitForExistence(timeout: 5))
        sourceItem.click()
        field.click()
        func physicalKey(_ code: CGKeyCode) throws {
            let running = NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce")
            XCTAssertEqual(running.count, 1, "Physical events require one owned BlueIce test application")
            let target = try XCTUnwrap(running.count == 1 ? running.first : nil)
            XCTAssertTrue(target.isActive, "The owned test application must be frontmost")
            let source = try XCTUnwrap(CGEventSource(stateID: .hidSystemState))
            let down = try XCTUnwrap(CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true))
            let up = try XCTUnwrap(CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false))
            down.postToPid(target.processIdentifier); up.postToPid(target.processIdentifier)
        }
        // Standard Zhuyin physical keys: ㄋ, ㄧ, third tone → 你.
        try physicalKey(1)
        waitValue(field, "ㄋ")
        let active = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
        let activeID = Unmanaged<CFString>.fromOpaque(TISGetInputSourceProperty(active, kTISPropertyInputSourceID)).takeUnretainedValue() as String
        XCTAssertEqual(activeID, "com.apple.inputmethod.TCIM.Zhuyin", "XCUITest must keep the requested input source active")
        try physicalKey(32)
        waitValue(field, "ㄋㄧ")
        try physicalKey(20)
        waitValue(field, "你")
        try physicalKey(36)
        waitValue(field, "你")
        try physicalKey(1)
        waitValue(field, "你ㄋ")
        try physicalKey(32)
        try physicalKey(53)
        waitValue(field, "你")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-system-zhuyin-input"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testMissingLauncherShowsFailureAndDisabledNavigation() {
        app.launchArguments = ["--launcher-exe", "/nonexistent/blueice-launcher"]
        app.launch()
        let failed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS[c] 'launcher' OR label CONTAINS[c] 'launcher'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [failed], timeout: 10), .completed, app.debugDescription)
        XCTAssertFalse(app.buttons["reload"].isEnabled)
        XCTAssertFalse(app.buttons["add-tab"].isEnabled)
    }

    func testWindowCloseTerminatesApplication() {
        launch()
        app.windows.firstMatch.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 10))
    }

    func testMissingCoreShowsFailureAndDisabledNavigation() {
        app.launchArguments = ["--core-exe", "/nonexistent/blueice-core"]
        app.launch()
        XCTAssertTrue(app.staticTexts["status"].waitForExistence(timeout: 10))
        let failed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS[c] 'core' OR label CONTAINS[c] 'core'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [failed], timeout: 10), .completed, app.debugDescription)
        XCTAssertFalse(app.buttons["reload"].isEnabled)
        XCTAssertFalse(app.buttons["add-tab"].isEnabled)
    }

    func testKeyboardGetSubmissionValidatesRequiredAndEncodesCurrentValues() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/forms")
        let page = app.groups["page"]
        XCTAssertTrue(page.textFields["Query"].waitForExistence(timeout: 15))
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey("a", modifierFlags: .command); app.typeKey(.delete, modifierFlags: [])
        waitValue(page.textFields["Query"], "")
        app.typeKey(.return, modifierFlags: [])
        let required = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS 'required' OR label CONTAINS 'required'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [required], timeout: 15), .completed)
        XCTAssertEqual(fixture.requests, ["/forms"])
        app.typeText("ice & snow")
        waitValue(page.textFields["Query"], "ice & snow")
        app.typeKey(.return, modifierFlags: [])
        let target = "/received?q=ice+%26+snow&accepted=on&region=a&notes=one%0D%0Atwo&level=50&mode=get"
        waitValue(app.textFields["address"], fixture.origin + target)
        XCTAssertEqual(fixture.requests, ["/forms", target])
        XCTAssertEqual(fixture.records.last?.method, "GET")
        XCTAssertTrue(fixture.records.last?.body.isEmpty == true)
    }

    func testPointerPostReloadAndHistoryRequireVisibleResubmissionConfirmation() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/forms")
        let page = app.groups["page"]
        XCTAssertTrue(page.buttons["Send POST"].waitForExistence(timeout: 15))
        waitValue(page.sliders["Level"], "50")
        page.buttons["Send POST"].click()
        waitValue(app.textFields["address"], fixture.origin + "/posted?kept=1")
        let post = try XCTUnwrap(fixture.records.last)
        XCTAssertEqual(post.method, "POST")
        XCTAssertEqual(String(data: post.body, encoding: .utf8), "q=A+%26+%E5%86%B0&accepted=on&region=a&notes=one%0D%0Atwo&level=50&mode=post")
        app.buttons["reload"].click()
        XCTAssertTrue(app.windows["browser-window"].sheets.buttons["Cancel"].waitForExistence(timeout: 15))
        XCTAssertEqual(fixture.requests.count, 2)
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-form-resubmission"; attachment.lifetime = .keepAlways; add(attachment)
        app.windows["browser-window"].sheets.buttons["Cancel"].click()
        XCTAssertEqual(fixture.requests.count, 2)
        app.typeKey("r", modifierFlags: .command)
        XCTAssertTrue(app.windows["browser-window"].sheets.buttons["Resend"].waitForExistence(timeout: 15))
        app.windows["browser-window"].sheets.buttons["Resend"].click()
        let resent = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in fixture.requests.count == 3 }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [resent], timeout: 15), .completed)
        XCTAssertEqual(fixture.records.last?.body, post.body)
        app.buttons["back"].click(); waitValue(app.textFields["address"], fixture.origin + "/forms")
        app.buttons["forward"].click()
        XCTAssertTrue(app.windows["browser-window"].sheets.buttons["Cancel"].waitForExistence(timeout: 15))
        app.windows["browser-window"].sheets.buttons["Cancel"].click()
        XCTAssertEqual(app.textFields["address"].value as? String, fixture.origin + "/forms")
        XCTAssertEqual(fixture.requests.count, 4)
        app.buttons["forward"].click()
        XCTAssertTrue(app.windows["browser-window"].sheets.buttons["Resend"].waitForExistence(timeout: 15)); app.windows["browser-window"].sheets.buttons["Resend"].click()
        waitValue(app.textFields["address"], fixture.origin + "/posted?kept=1")
        XCTAssertEqual(fixture.records.last?.body, post.body)
    }

}
