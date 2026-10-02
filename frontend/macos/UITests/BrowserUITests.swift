// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import XCTest

@MainActor
final class BrowserUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
    }

    override func tearDownWithError() throws { app.terminate() }

    private func launch() {
        app.launch()
        XCTAssertTrue(app.textFields["address"].waitForExistence(timeout: 15))
        waitValue(app.textFields["address"], "about:credits")
        XCTAssertTrue(app.groups["page"].waitForExistence(timeout: 15), app.debugDescription)
    }

    private func waitValue(_ element: XCUIElement, _ value: String) {
        let predicate = NSPredicate(format: "value == %@", value)
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: element)], timeout: 15), .completed)
    }

    private func enter(_ text: String, submit: Bool = true) {
        let address = app.textFields["address"]
        address.click()
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
        enter("https://example.invalid")
        let denied = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS[c] 'blocked' OR label CONTAINS[c] 'blocked'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [denied], timeout: 15), .completed, app.debugDescription)
        app.buttons["reload"].click()
        waitValue(app.textFields["address"], "about:credits")
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
