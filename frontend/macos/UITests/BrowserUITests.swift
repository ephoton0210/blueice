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
}
