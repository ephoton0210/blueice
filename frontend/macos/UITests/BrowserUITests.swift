// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import CoreServices
import Security
import LocalAuthentication
import Carbon
import ApplicationServices
import XCTest
import Darwin

@MainActor
final class BrowserUITests: XCTestCase {
    private var printPanel: XCUIElement { app.sheets.containing(.menuButton,identifier: "PDF").firstMatch }
    private func positionWindowForUnobstructedSheet(_ window: XCUIElement) {
        // Keep owned sheets clear of the host's existing Local Network prompt.
        // Move only BlueIce; the system prompt and its permissions remain untouched.
        if window.frame.width > 900 {
            let corner = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
                .withOffset(CGVector(dx: -2, dy: -2))
            corner.click(forDuration: 0.2, thenDragTo: corner.withOffset(
                CGVector(dx: 900 - window.frame.width, dy: 0)))
            XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                abs(window.frame.width - 900) < 3
            }, object: window)], timeout: 10), .completed)
        }
        let titleBar = window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.02))
        titleBar.click(forDuration: 0.1, thenDragTo: titleBar.withOffset(
            CGVector(dx: 40 - window.frame.minX, dy: 40 - window.frame.minY)))
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            abs(window.frame.minX - 40) < 3 && abs(window.frame.minY - 40) < 3
        }, object: window)], timeout: 10), .completed)
    }
    private func configureAssistant(_ model: AssistantModelFixture) throws {
        let settings: [String: Any] = ["version": 1, "backend": "loopback", "idle_timeout_secs": 600,
            "nice": 10, "max_resident_mb": 2048, "candle": NSNull(),
            "loopback": ["provider": "ollama", "base_url": model.baseURL, "model": "local-model"]]
        try JSONSerialization.data(withJSONObject: settings).write(to: assistantSettingsFile)
    }
    private func openAssistant(in window: XCUIElement? = nil, ready: Bool = true) {
        let scope = window ?? app!
        scope.buttons["assistant"].click()
        XCTAssertTrue(scope.buttons["assistant-summarize"].waitForExistence(timeout: 10), app.debugDescription)
        if ready { XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: scope.buttons["assistant-summarize"])], timeout: 10), .completed, app.debugDescription) }
    }
    private func waitAssistantText(_ element: XCUIElement, _ text: String) {
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@ OR value == %@", text, text), object: element)], timeout: 15), .completed, app.debugDescription)
    }
    private func waitModelRequests(_ model: AssistantModelFixture, count: Int) {
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in model.prompts.count >= count }, object: nil)], timeout: 10), .completed)
    }
    func testNativeAssistantSummaryOrganizePlainTextAndProtectedInputPrivacy() throws {
        let origin = try HTTPFixture(); defer { origin.stop() }
        let model = try AssistantModelFixture(); defer { model.stop() }; try configureAssistant(model)
        launch(); enter(origin.origin + "/assistant")
        XCTAssertTrue(app.groups["page"].secureTextFields["Assistant secret"].waitForExistence(timeout: 15))
        openAssistant(); app.buttons["assistant-summarize"].click()
        waitAssistantText(app.staticTexts["assistant-result"], AssistantModelFixture.summary)
        let user = try XCTUnwrap(model.prompts.first).user
        XCTAssertTrue(user.contains("Hello") && user.contains("World"))
        XCTAssertFalse(user.contains("assistant-private-secret")); XCTAssertFalse(user.contains("assistant-hidden-secret"))
        waitValue(app.textFields["address"], origin.origin + "/assistant")
        let instruction = app.textFields["assistant-instruction"]
        instruction.click(); instruction.typeKey("a", modifierFlags: .command); instruction.typeText("Make a table.")
        app.buttons["assistant-organize"].click()
        waitAssistantText(app.staticTexts["assistant-result"], AssistantModelFixture.organized)
        XCTAssertTrue(model.prompts.last?.system.contains("Make a table.") == true)
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        attachment.name = "macos-native-assistant-result"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["assistant-close"].click(); app.typeKey("a", modifierFlags: [.command, .shift])
        waitAssistantText(app.staticTexts["assistant-result"], AssistantModelFixture.organized)
        XCTAssertEqual(origin.requests, ["/assistant"])
    }
    func testNativeAssistantDiscardsOldDocumentReplyAndStopsWaiting() throws {
        let origin = try HTTPFixture(); defer { origin.stop() }
        let model = try AssistantModelFixture(); defer { model.stop() }; try configureAssistant(model)
        launch(); enter(origin.origin + "/assistant"); openAssistant(); model.hold()
        app.buttons["assistant-summarize"].click(); waitModelRequests(model, count: 1)
        enter(origin.origin + "/second"); waitValue(app.textFields["address"], origin.origin + "/second")
        XCTAssertTrue(app.groups["page"].staticTexts["BlueIce external page"].waitForExistence(timeout: 15))
        model.release()
        XCTAssertFalse(app.staticTexts["assistant-result"].exists)
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: app.buttons["assistant-summarize"])], timeout: 10), .completed)
        model.hold(); app.buttons["assistant-summarize"].click(); waitModelRequests(model, count: 2)
        app.buttons["assistant-stop"].click(); model.release()
        waitAssistantText(app.staticTexts["assistant-notice"], "Stopped waiting. The local task may finish in the background.")
        XCTAssertFalse(app.staticTexts["assistant-result"].exists)
        app.buttons["assistant-summarize"].click()
        waitAssistantText(app.staticTexts["assistant-result"], AssistantModelFixture.summary)
        XCTAssertEqual(origin.requests, ["/assistant", "/second"])
    }
    func testNativeAssistantTranslationToggleAndPreferenceRelaunch() throws {
        let origin = try HTTPFixture(); defer { origin.stop() }
        let model = try AssistantModelFixture(); defer { model.stop() }; try configureAssistant(model)
        launch(); openAssistant()
        app.textFields["assistant-language"].click()
        app.typeKey(.tab, modifierFlags: []); app.typeKey(.return, modifierFlags: [])
        waitAssistantText(app.staticTexts["assistant-language-state"], "Target: zh-TW")
        enter(origin.origin + "/assistant")
        XCTAssertTrue(app.groups["page"].staticTexts["你好"].waitForExistence(timeout: 15), app.debugDescription)
        let toggle = app.checkBoxes["assistant-show-translation"]
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: toggle)], timeout: 10), .completed)
        app.textFields["assistant-language"].click()
        app.typeText("\t\t\t ")
        XCTAssertTrue(app.groups["page"].staticTexts["Hello"].waitForExistence(timeout: 10))
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: toggle)], timeout: 10), .completed)
        // While the core applies translation, this group is disabled and focus
        // recovers at Settings. Return through Refresh to the enabled checkbox.
        app.typeKey(.tab, modifierFlags: .shift); app.typeKey(.tab, modifierFlags: .shift)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(app.groups["page"].staticTexts["你好"].waitForExistence(timeout: 10))
        XCTAssertEqual(origin.requests, ["/assistant"])
        app.windows["browser-window"].buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
        launch(); enter(origin.origin + "/assistant")
        XCTAssertTrue(app.groups["page"].staticTexts["你好"].waitForExistence(timeout: 15), app.debugDescription)
        openAssistant(); app.buttons["assistant-language-off"].click()
        waitAssistantText(app.staticTexts["assistant-language-state"], "Target: Off")
        XCTAssertNil(UserDefaults(suiteName: preferenceDomain)?.string(forKey: "browser.translationLanguage"))
    }
    func testNativeAssistantFailurePreservesMandatoryNavigationDenial() throws {
        let origin = try HTTPFixture(); defer { origin.stop() }
        launch(); enter(origin.origin + "/assistant")
        XCTAssertTrue(app.groups["page"].secureTextFields["Assistant secret"].waitForExistence(timeout: 15))
        enter(origin.origin + "/blocked")
        let status = app.staticTexts["status"]
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "value BEGINSWITH 'Navigation blocked'"), object: status)], timeout: 15), .completed)
        let denial = try XCTUnwrap(status.value as? String)
        openAssistant(); app.buttons["assistant-summarize"].click()
        XCTAssertTrue(app.staticTexts["assistant-notice"].waitForExistence(timeout: 15))
        XCTAssertEqual(status.value as? String, denial)
        waitAssistantText(app.staticTexts["assistant-source"], origin.origin + "/assistant")
        XCTAssertTrue(app.groups["page"].staticTexts["Hello"].exists)
        XCTAssertEqual(origin.requests, ["/assistant", "/blocked"])
    }
    func testNativeAssistantPendingResultFollowsTabToAnotherWindow() throws {
        let origin = try HTTPFixture(); defer { origin.stop() }
        let model = try AssistantModelFixture(); defer { model.stop() }; try configureAssistant(model)
        launch(); let first = app.windows["browser-window"]
        enter(origin.origin + "/assistant", in: first); openAssistant(in: first)
        model.hold(); first.buttons["assistant-summarize"].click(); waitModelRequests(model, count: 1)
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15))
        activateWindow(1); transferTab(1, from: first, to: "Window 2")
        activateWindow(2); openAssistant(in: second, ready: false)
        XCTAssertTrue(second.buttons["assistant-stop"].waitForExistence(timeout: 10)); model.release()
        waitAssistantText(second.staticTexts["assistant-result"], AssistantModelFixture.summary)
        waitValue(second.textFields["address"], origin.origin + "/assistant")
        XCTAssertFalse(first.staticTexts["assistant-result"].exists)
        XCTAssertEqual(origin.requests, ["/assistant"])
    }
    private func openAssistantSettings() -> XCUIApplication {
        let panels = openPermissionPanel()
        let selector = panels.radioButtons["Assistant Settings"]
        XCTAssertTrue(selector.waitForExistence(timeout: 10), panels.debugDescription)
        XCTAssertTrue(selector.isEnabled); selector.click()
        XCTAssertTrue(panels.staticTexts["assistant-settings-current"].waitForExistence(timeout: 10), panels.debugDescription)
        return panels
    }
    private func persistedAssistant() throws -> [String: Any] {
        try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: assistantSettingsFile)) as? [String: Any])
    }
    private func editAssistantField(_ panels: XCUIApplication, _ identifier: String, _ text: String) {
        let field = panels.textFields[identifier]
        let editor = panels.scrollViews["assistant-settings-editor"]
        for _ in 0..<4 {
            if field.exists && field.isHittable { break }
            let delta: CGFloat = field.exists && field.frame.minY < editor.frame.minY ? 220 : -220
            editor.scroll(byDeltaX: 0,deltaY: delta)
        }
        XCTAssertTrue(field.waitForExistence(timeout: 5),panels.debugDescription)
        XCTAssertTrue(field.isHittable,panels.debugDescription); field.click()
        field.typeKey("a", modifierFlags: .command); field.typeText(text)
    }
    func testNativeAssistantSettingsReviewCancelApplyAndRelaunch() throws {
        launch()
        var panels = openAssistantSettings()
        waitPermissionValue(panels.staticTexts["assistant-settings-current"], "In force: Off · idle 600 seconds · niceness 10")
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path), "Inspection never persists a setting")
        editAssistantField(panels, "assistant-idle", "29")
        panels.buttons["assistant-settings-review"].click()
        XCTAssertTrue(panels.staticTexts["assistant-settings-notice"].waitForExistence(timeout: 5))
        XCTAssertFalse(panels.buttons["assistant-settings-confirm"].exists)
        editAssistantField(panels, "assistant-idle", "630")
        panels.buttons["assistant-settings-review"].click()
        XCTAssertTrue(panels.buttons["assistant-settings-confirm"].waitForExistence(timeout: 5))
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        panels.buttons["assistant-settings-cancel"].click()
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        panels.buttons["assistant-settings-review"].click(); panels.buttons["assistant-settings-confirm"].click()
        waitPermissionValue(panels.staticTexts["assistant-settings-notice"], "Assistant settings applied.")
        XCTAssertEqual(try persistedAssistant()["idle_timeout_secs"] as? Int, 630)
        XCTAssertEqual(try persistedAssistant()["backend"] as? String, "none")
        let attachment = XCTAttachment(screenshot: panels.windows["permissions-window"].screenshot())
        attachment.name = "macos-native-assistant-settings"; attachment.lifetime = .keepAlways; add(attachment)
        panels.windows["permissions-window"].buttons[XCUIIdentifierCloseWindow].click()
        app.activate(); waitValue(app.textFields["address"], "about:credits")
        app.windows["browser-window"].buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
        launch(); panels = openAssistantSettings()
        waitPermissionValue(panels.staticTexts["assistant-settings-current"], "In force: Off · idle 630 seconds · niceness 10")
    }
    func testNativeAssistantProposalCannotApplyWithoutExactHumanConfirmation() throws {
        launch()
        let initial: [String: Any] = ["version": 1, "backend": "none", "idle_timeout_secs": 600, "nice": 10, "loopback": NSNull(), "candle": NSNull(), "max_resident_mb": NSNull()]
        var blocked = initial; blocked["nice"] = 0
        XCTAssertNotNil(try assistantControl(["ProposeAssistantSettings": ["settings": blocked]])["AssistantProposalBlocked"])
        var proposed = initial; proposed["idle_timeout_secs"] = 660
        let accepted = try XCTUnwrap(try assistantControl(["ProposeAssistantSettings": ["settings": proposed]])["AssistantProposalAccepted"] as? [String: Any])
        let id = try XCTUnwrap(accepted["id"] as? UInt64)
        let digest = try XCTUnwrap(accepted["digest"] as? String)
        let panels = openAssistantSettings()
        waitPermissionValue(panels.staticTexts["assistant-proposal-digest"], digest)
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        // The ordinary operator protocol has no approval route.
        XCTAssertThrowsError(try assistantControl(["ApproveAssistantProposal": ["id": id, "digest": digest]]))
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        panels.buttons["assistant-proposal-review"].click()
        XCTAssertTrue(panels.buttons["assistant-settings-confirm"].waitForExistence(timeout: 5))
        panels.buttons["assistant-settings-cancel"].click()
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        panels.buttons["assistant-proposal-review"].click()
        panels.buttons["assistant-settings-refresh"].click()
        XCTAssertFalse(panels.buttons["assistant-settings-confirm"].exists, "Fresh state cancels approval")
        panels.buttons["assistant-proposal-review"].click(); panels.buttons["assistant-settings-confirm"].click()
        waitPermissionValue(panels.staticTexts["assistant-settings-notice"], "Assistant settings applied.")
        XCTAssertEqual(try persistedAssistant()["idle_timeout_secs"] as? Int, 660)
        XCTAssertEqual(try assistantControl(["AssistantProposalStatus": ["id": id]])["AssistantProposalStatus"] as? [String: String], ["status": "approved"])
        var denied = proposed; denied["idle_timeout_secs"] = 690
        _ = try assistantControl(["ProposeAssistantSettings": ["settings": denied]])
        panels.buttons["assistant-settings-refresh"].click()
        XCTAssertTrue(panels.buttons["assistant-proposal-deny"].waitForExistence(timeout: 5))
        panels.buttons["assistant-proposal-deny"].click()
        waitPermissionValue(panels.staticTexts["assistant-settings-notice"], "Proposal denied. Settings unchanged.")
        XCTAssertEqual(try persistedAssistant()["idle_timeout_secs"] as? Int, 660)
    }
    func testNativeAssistantLoopbackEditorAndClosingUnconfirmedChanges() throws {
        launch()
        let panels = openAssistantSettings()
        panels.popUpButtons["assistant-backend"].click(); panels.menuItems["Loopback server"].click()
        waitPermissionValue(panels.popUpButtons["assistant-backend"],"Loopback server")
        editAssistantField(panels, "assistant-model", "local-model")
        editAssistantField(panels, "assistant-base-url", "http://example.com:11434/v1/")
        panels.buttons["assistant-settings-review"].click()
        XCTAssertTrue(panels.staticTexts["assistant-settings-notice"].waitForExistence(timeout: 5))
        XCTAssertFalse(panels.buttons["assistant-settings-confirm"].exists)
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        editAssistantField(panels, "assistant-base-url", "http://127.0.0.1:11434/v1/")
        panels.buttons["assistant-settings-review"].click()
        XCTAssertTrue(panels.buttons["assistant-settings-confirm"].waitForExistence(timeout: 5))
        XCTAssertTrue(panels.buttons["assistant-settings-confirm"].isHittable)
        panels.windows["permissions-window"].buttons[XCUIIdentifierCloseWindow].click()
        app.activate(); _ = openPermissionPanel()
        if panels.radioButtons["Assistant Settings"].value as? Int != 1 { panels.radioButtons["Assistant Settings"].click() }
        XCTAssertTrue(panels.staticTexts["assistant-settings-current"].waitForExistence(timeout: 10))
        XCTAssertFalse(panels.buttons["assistant-settings-confirm"].exists)
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        panels.popUpButtons["assistant-backend"].click(); panels.menuItems["Loopback server"].click()
        waitPermissionValue(panels.popUpButtons["assistant-backend"],"Loopback server")
        editAssistantField(panels, "assistant-model", "local-model")
        panels.buttons["assistant-settings-review"].click(); panels.buttons["assistant-settings-confirm"].click()
        waitPermissionValue(panels.staticTexts["assistant-settings-notice"], "Assistant settings applied.")
        let persisted = try persistedAssistant()
        XCTAssertEqual(persisted["backend"] as? String, "loopback")
        let loopback = try XCTUnwrap(persisted["loopback"] as? [String: String])
        XCTAssertEqual(loopback, ["provider": "ollama", "base_url": "http://127.0.0.1:11434/v1/", "model": "local-model"])
        XCTAssertEqual(app.textFields["address"].value as? String, "about:credits")
    }
    func testSwitchingTrustedPanelsCancelsSettingsAndOneShotConfirmation() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let manifest = try permissionPackage(); defer { try? FileManager.default.removeItem(at: manifest.deletingLastPathComponent()) }
        app.launchArguments += ["--extension-manifest", manifest.path]
        launch(); enter(fixture.origin + "/input")
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.buttons["permission-review-document"].waitForExistence(timeout: 10))
        panels.buttons["permission-review-document"].click()
        XCTAssertTrue(panels.buttons["permission-confirm-one-shot"].waitForExistence(timeout: 5))
        panels.radioButtons["Assistant Settings"].click()
        XCTAssertTrue(panels.staticTexts["assistant-settings-current"].waitForExistence(timeout: 10))
        editAssistantField(panels, "assistant-nice", "11"); panels.buttons["assistant-settings-review"].click()
        XCTAssertTrue(panels.buttons["assistant-settings-confirm"].waitForExistence(timeout: 5))
        panels.radioButtons["Permissions"].click()
        XCTAssertTrue(panels.buttons["permission-review-document"].waitForExistence(timeout: 5))
        XCTAssertFalse(panels.buttons["permission-confirm-one-shot"].exists)
        waitPermissionValue(panels.staticTexts["permission-state-storage"], "Not allowed")
        panels.radioButtons["Assistant Settings"].click()
        XCTAssertTrue(panels.staticTexts["assistant-settings-current"].waitForExistence(timeout: 5))
        XCTAssertFalse(panels.buttons["assistant-settings-confirm"].exists)
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        XCTAssertEqual(fixture.requests, ["/input"])
    }
    // Test-only ordinary operator connection. It can propose/inspect but
    // cannot send a decision through the launcher's private window pipe.
    private func assistantControl(_ request: Any) throws -> [String: Any] {
        let path = Array(assistantControlSocket.path.utf8) + [0]
        var address = sockaddr_un(); address.sun_family = sa_family_t(AF_UNIX); address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        guard path.count <= MemoryLayout.size(ofValue: address.sun_path) else { throw NSError(domain: "AssistantControl", code: 1) }
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: path) }
        let descriptor = socket(AF_UNIX, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw NSError(domain: "AssistantControl", code: 2) }
        let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true); defer { try? handle.close() }
        let result = withUnsafePointer(to: &address) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) } }
        guard result == 0 else { throw NSError(domain: "AssistantControl", code: Int(errno), userInfo: [NSLocalizedDescriptionKey: "Cannot connect to the owned test socket: \(assistantControlSocket.path)"]) }
        var timeout = timeval(tv_sec: 5, tv_usec: 0)
        _ = setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        _ = fcntl(descriptor, F_SETNOSIGPIPE, 1)
        let body = try JSONSerialization.data(withJSONObject: request, options: [.fragmentsAllowed])
        var count = UInt32(body.count).littleEndian
        var frame = withUnsafeBytes(of: &count) { Data($0) }; frame.append(body); try handle.write(contentsOf: frame)
        func read(_ count: Int) throws -> Data {
            var data = Data()
            while data.count < count {
                guard let part = try handle.read(upToCount: count - data.count), !part.isEmpty else { throw NSError(domain: "AssistantControl", code: 4) }
                data.append(part)
            }
            return data
        }
        let prefix = try read(4)
        let length = prefix.enumerated().reduce(UInt32(0)) { $0 | UInt32($1.element) << ($1.offset * 8) }
        guard length > 0, length <= 128 * 1024 else { throw NSError(domain: "AssistantControl", code: 5) }
        return try XCTUnwrap(JSONSerialization.jsonObject(with: read(Int(length))) as? [String: Any])
    }
    private func permissionPackage() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bi-permissions-ui-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let manifest = root.appendingPathComponent("extension.json")
        try Data(#"{"name":"Native permission fixture","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["storage"],"runtime_ephemeral":["dom:read"]}}"#.utf8).write(to: manifest)
        var wasm = Data([0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,7,17,1,13])
        wasm.append(Data("blueice_start".utf8)); wasm.append(contentsOf: [0,0,10,4,1,2,0,11])
        try wasm.write(to: root.appendingPathComponent("extension.wasm"))
        return manifest
    }
    private func openPermissionPanel(in window: XCUIElement? = nil) -> XCUIApplication {
        (window ?? app).buttons["permissions"].click()
        let panels = XCUIApplication(bundleIdentifier: "cc.blueice.BlueIcePanels")
        XCTAssertTrue(panels.windows["permissions-window"].waitForExistence(timeout: 10), panels.debugDescription)
        return panels
    }
    private func waitPermissionValue(_ element: XCUIElement, _ label: String) {
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", label), object: element)], timeout: 10), .completed, element.debugDescription)
    }
    func testNativePermissionPanelEmptyStateAndCloseKeepBrowserAlive() throws {
        launch()
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.staticTexts["permissions-empty"].waitForExistence(timeout: 10), panels.debugDescription)
        XCTAssertFalse(panels.buttons["permission-confirm"].exists)
        panels.windows["permissions-window"].buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.windows["browser-window"].exists)
        app.activate(); enter("about:settings"); waitValue(app.textFields["address"], "about:settings")
        waitValue(app.staticTexts["status"], "Ready")
        XCTAssertTrue(app.groups["page"].exists)
        _ = openPermissionPanel()
        XCTAssertTrue(panels.staticTexts["permissions-empty"].exists)
    }
    func testNativePermissionGrantCancelRevokeAndRefreshRetainPage() throws {
        let manifest = try permissionPackage()
        defer { try? FileManager.default.removeItem(at: manifest.deletingLastPathComponent()) }
        app.launchArguments += ["--extension-manifest", manifest.path]
        launch()
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.staticTexts["permission-package-name"].waitForExistence(timeout: 10), panels.debugDescription)
        waitPermissionValue(panels.staticTexts["permission-package-name"], "Native permission fixture")
        let state = panels.staticTexts["permission-state-storage"]
        waitPermissionValue(state, "Not allowed")
        panels.buttons["permission-change-storage"].click()
        XCTAssertTrue(panels.staticTexts["permission-confirmation"].waitForExistence(timeout: 5))
        waitPermissionValue(state, "Not allowed")
        panels.buttons["permission-cancel"].click(); waitPermissionValue(state, "Not allowed")
        panels.buttons["permission-change-storage"].click(); panels.buttons["permission-confirm"].click()
        waitPermissionValue(state, "Allowed")
        panels.buttons["permissions-refresh"].click(); waitPermissionValue(state, "Allowed")
        let image = XCTAttachment(screenshot: panels.windows["permissions-window"].screenshot())
        image.name = "macos-native-permission-allowed"; image.lifetime = .keepAlways; add(image)
        panels.buttons["permission-change-storage"].click(); panels.buttons["permission-confirm"].click()
        waitPermissionValue(state, "Not allowed")
        panels.windows["permissions-window"].buttons[XCUIIdentifierCloseWindow].click()
        app.activate(); waitValue(app.textFields["address"], "about:credits")
        XCTAssertTrue(app.groups["page"].exists)
    }
    func testNativeOneShotReviewCancelAndStaleDocumentRejection() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let manifest = try permissionPackage()
        defer { try? FileManager.default.removeItem(at: manifest.deletingLastPathComponent()) }
        app.launchArguments += ["--extension-manifest", manifest.path]
        launch(); enter(fixture.origin + "/input")
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.buttons["permission-review-document"].waitForExistence(timeout: 10), panels.debugDescription)
        panels.buttons["permission-review-document"].click()
        XCTAssertTrue(panels.staticTexts["permission-reviewed-url"].waitForExistence(timeout: 10), panels.debugDescription)
        waitPermissionValue(panels.staticTexts["permission-reviewed-url"], fixture.origin + "/input")
        XCTAssertFalse(panels.staticTexts["permissions-notice"].exists)
        panels.buttons["permission-cancel"].click()
        XCTAssertFalse(panels.buttons["permission-confirm-one-shot"].exists)
        panels.buttons["permission-review-document"].click()
        XCTAssertTrue(panels.buttons["permission-confirm-one-shot"].waitForExistence(timeout: 5))
        app.activate(); enter(fixture.origin + "/second")
        panels.activate(); panels.buttons["permission-confirm-one-shot"].click()
        waitPermissionValue(panels.staticTexts["permissions-notice"], "the reviewed HTTP(S) document changed before confirmation")
        XCTAssertFalse(panels.buttons["permission-confirm-one-shot"].exists)
        panels.buttons["permissions-refresh"].click()
        XCTAssertTrue(panels.buttons["permission-review-document"].waitForExistence(timeout: 5))
        panels.buttons["permission-review-document"].click()
        XCTAssertTrue(panels.staticTexts["permission-reviewed-url"].waitForExistence(timeout: 5))
        waitPermissionValue(panels.staticTexts["permission-reviewed-url"], fixture.origin + "/second")
        panels.buttons["permission-confirm-one-shot"].click()
        waitPermissionValue(panels.staticTexts["permissions-notice"], "One DOM read allowed for the reviewed document. This is not a persistent permission.")
        waitPermissionValue(panels.staticTexts["permission-state-storage"], "Not allowed")
    }
    private func terminateOwnedPermissionChild(started: Date) throws -> pid_t {
        // This companion is launcher-owned, so XCTest's terminate/reap path
        // races the launcher's own waitpid. Kill only the uniquely identified
        // child and observe both applications through their ordinary UI.
        let ownedPanels = NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIcePanels")
        let applications = NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce")
        guard ownedPanels.count == 1, applications.count == 1,
              let application = applications.first,
              application.launchDate.map({ $0 >= started.addingTimeInterval(-2) }) == true else {
            throw NSError(domain: "BlueIceUITestOwnership", code: 1, userInfo: [NSLocalizedDescriptionKey:
                "Identify this test's application and permission child before terminating a process"])
        }
        let child = try XCTUnwrap(ownedPanels.first)
        let launcher = getpgid(child.processIdentifier)
        guard launcher > 0 else {
            throw NSError(domain: "BlueIceUITestOwnership", code: 2, userInfo: [NSLocalizedDescriptionKey:
                "The permission child must belong to an owned launcher group"])
        }
        var launcherInfo = proc_bsdinfo()
        let infoSize = Int32(MemoryLayout<proc_bsdinfo>.stride)
        guard proc_pidinfo(launcher, PROC_PIDTBSDINFO, 0, &launcherInfo, infoSize) == infoSize,
              launcherInfo.pbi_ppid == UInt32(application.processIdentifier) else {
            throw NSError(domain: "BlueIceUITestOwnership", code: 3, userInfo: [NSLocalizedDescriptionKey:
                "The child's launcher must be a direct child of this test's BlueIce application"])
        }
        XCTAssertEqual(kill(child.processIdentifier, SIGKILL), 0)
        return launcher
    }
    func testNativePermissionChildExitClosesTheOwnedServiceConnection() throws {
        let started = Date()
        app.launchArguments += ["-browser.contexts", "malformed-profile-fixture"]
        launch()
        let notice = app.staticTexts["window-notice"]
        XCTAssertTrue(notice.waitForExistence(timeout: 10))
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.staticTexts["permissions-empty"].waitForExistence(timeout: 10))
        app.activate(); app.groups["page"].click()
        _ = try terminateOwnedPermissionChild(started: started)
        XCTAssertTrue(panels.wait(for: .notRunning, timeout: 10))
        let status = app.staticTexts["status"]
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "value BEGINSWITH 'Browser service connection ended:'"), object: status)], timeout: 15), .completed, app.debugDescription)
        XCTAssertFalse(app.textFields["address"].isEnabled)
        XCTAssertFalse(app.buttons["permissions"].isEnabled)
        XCTAssertTrue(app.windows["browser-window"].exists)
        app.activate()
        XCTAssertFalse(app.buttons["reload"].isEnabled)
        XCTAssertTrue(app.buttons["retry-windows"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["restart-browser"].isEnabled)
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(" ", modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: notice)], timeout: 5), .completed,
                       "The surviving notice controls must accept Tab and Space after the core becomes unavailable")
        XCTAssertFalse(app.buttons["reload"].isEnabled)
    }

    func testBrowserServiceRestartRestoresVolatileWindowsHistoryAndZoomWithoutSaving() throws {
        let started = Date()
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        app.launchArguments += ["-browser.session.remember", "NO", "-browser.session.reopen", "NO"]
        launch()
        let root = app.windows["browser-window"]
        enter(fixture.origin + "/first"); enter(fixture.origin + "/second")
        app.buttons["back"].click(); waitValue(app.textFields["address"], fixture.origin + "/first")
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Page Zoom"].hover(); app.menuItems["150%"].click()
        waitValue(app.buttons["page-zoom"], "150%")
        app.buttons["new-tab-group"].click(); XCTAssertTrue(app.textFields["group-name"].waitForExistence(timeout: 10))
        paste("Recovery study", into: app.textFields["group-name"]); app.buttons["group-save"].click()
        XCTAssertTrue(app.buttons["tab-group-1"].waitForExistence(timeout: 10))
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        enter(fixture.origin + "/appearance")
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15)); enter(fixture.origin + "/editing", in: second)
        XCTAssertTrue(second.textFields["Editor"].waitForExistence(timeout: 15))
        paste("unsaved-recovery-page-value", into: second.textFields["Editor"])
        activateWindow(1)
        let application = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce").first)
        let originalPID = application.processIdentifier
        let panels = openPermissionPanel(in: root)
        XCTAssertTrue(panels.staticTexts["permissions-empty"].waitForExistence(timeout: 10))
        let oldLauncher = try terminateOwnedPermissionChild(started: started)
        XCTAssertTrue(panels.wait(for: .notRunning, timeout: 10)); app.activate()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "value BEGINSWITH 'Browser service connection ended:'"), object: root.staticTexts["status"])], timeout: 15), .completed)
        XCTAssertFalse(root.textFields["address"].isEnabled)
        let restart = root.buttons["restart-browser"]
        guard restart.waitForExistence(timeout: 5) else {
            XCTFail("A stopped browser must offer an owned in-app restart and restore its current session without enabling disk persistence")
            return
        }
        XCTAssertTrue(restart.isEnabled)
        waitAssistantText(root.staticTexts["recovery-details"], "Windows to restore: 2; tabs: 3. Unsaved page changes will be lost.")
        let stoppedImage = XCTAttachment(screenshot: root.screenshot()); stoppedImage.name = "macos-browser-service-stopped"; stoppedImage.lifetime = .keepAlways; add(stoppedImage)
        app.typeKey(.tab, modifierFlags: []); app.typeKey(" ", modifierFlags: [])
        let restored = app.windows["browser-window-2"], recoveredEditor = app.windows["browser-window-3"]
        XCTAssertTrue(recoveredEditor.waitForExistence(timeout: 20))
        waitValue(restored.textFields["address"], fixture.origin + "/appearance")
        waitValue(recoveredEditor.textFields["address"], fixture.origin + "/editing")
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "exists == false"), object: app.windows["browser-window"])], timeout: 15), .completed)
        XCTAssertEqual(app.windows.matching(NSPredicate(format: "identifier BEGINSWITH 'browser-window'")).count, 2)
        XCTAssertTrue(restored.textFields["address"].isEnabled)
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce")
        XCTAssertEqual(running.count, 1); XCTAssertEqual(running.first?.processIdentifier, originalPID)
        let newPanel = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIcePanels").first)
        XCTAssertNotEqual(getpgid(newPanel.processIdentifier), oldLauncher)
        activateWindow(3); waitValue(recoveredEditor.textFields["Editor"], "A😀B")
        activateWindow(2)
        let first = restored.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'tab-' AND label == %@", fixture.origin + "/first")).firstMatch
        XCTAssertTrue(first.exists); first.click()
        waitValue(restored.textFields["address"], fixture.origin + "/first")
        waitValue(restored.buttons["page-zoom"], "150%")
        XCTAssertEqual(restored.buttons["tab-group-1"].label, "Recovery study")
        XCTAssertTrue(restored.buttons["forward"].isEnabled)
        restored.buttons["forward"].click(); waitValue(restored.textFields["address"], fixture.origin + "/second")
        XCTAssertEqual(fixture.requests, ["/first", "/second", "/first", "/appearance", "/editing", "/first", "/appearance", "/editing", "/second"])
        let settings = preferenceWindow()
        XCTAssertTrue(NSPredicate(format: "value == 0 OR value == '0'").evaluate(with: settings.checkBoxes["session-remember"]))
        XCTAssertFalse(settings.checkBoxes["session-reopen"].isEnabled)
        XCTAssertNil(storedSessionArchive(), "Crash recovery must not turn opt-in persistence on")
        settings.buttons[XCUIIdentifierCloseWindow].click()
        let image = XCTAttachment(screenshot: restored.screenshot()); image.name = "macos-browser-service-recovered"; image.lifetime = .keepAlways; add(image)

        // A second service failure must remain recoverable. Closing a stopped
        // native window is an explicit choice that recovery must respect.
        activateWindow(3)
        let secondPanels = openPermissionPanel(in: recoveredEditor)
        XCTAssertTrue(secondPanels.staticTexts["permissions-empty"].waitForExistence(timeout: 10))
        let secondLauncher = try terminateOwnedPermissionChild(started: started)
        XCTAssertTrue(secondPanels.wait(for: .notRunning, timeout: 10)); app.activate()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "value BEGINSWITH 'Browser service connection ended:'"), object: recoveredEditor.staticTexts["status"])], timeout: 15), .completed)
        recoveredEditor.buttons[XCUIIdentifierCloseWindow].click()
        guard XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: recoveredEditor)], timeout: 10) == .completed else {
            XCTFail("A stopped browser must close an individual native window and omit it from the next recovery")
            return
        }
        waitAssistantText(restored.staticTexts["recovery-details"], "Windows to restore: 1; tabs: 2. Unsaved page changes will be lost.")
        app.typeKey(.tab, modifierFlags: []); app.typeKey(" ", modifierFlags: [])
        waitValue(restored.textFields["address"], fixture.origin + "/second")
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "enabled == true"), object: restored.textFields["address"])], timeout: 15), .completed,
                       "The old stopped window can share the new core's ID and URL; wait for the replacement to finish restoring")
        XCTAssertEqual(app.windows.matching(NSPredicate(format: "identifier BEGINSWITH 'browser-window'")).count, 1)
        XCTAssertFalse(recoveredEditor.exists)
        XCTAssertEqual(NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce").first?.processIdentifier, originalPID)
        let thirdPanel = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIcePanels").first)
        XCTAssertNotEqual(getpgid(thirdPanel.processIdentifier), secondLauncher)
        waitValue(restored.buttons["page-zoom"], "150%")
        XCTAssertEqual(restored.buttons["tab-group-1"].label, "Recovery study")
        XCTAssertEqual(fixture.requests, ["/first", "/second", "/first", "/appearance", "/editing", "/first", "/appearance", "/editing", "/second", "/second", "/appearance"])
        XCTAssertNil(storedSessionArchive())
    }
    func testBrowserServiceRecoveryNeverReplaysPostOrRestoresItsPrivateBody() throws {
        let started = Date()
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        app.launchArguments += ["-browser.session.remember", "NO", "-browser.session.reopen", "NO"]
        launch(); enter(fixture.origin + "/forms")
        let originalPID = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce").first).processIdentifier
        let query = app.groups["page"].textFields["Query"]
        XCTAssertTrue(query.waitForExistence(timeout: 15)); paste("private-post-recovery-value", into: query)
        app.groups["page"].buttons["Send POST"].click()
        waitValue(app.textFields["address"], fixture.origin + "/posted?kept=1")
        XCTAssertEqual(fixture.records.last?.method, "POST")
        XCTAssertTrue(String(decoding: try XCTUnwrap(fixture.records.last?.body), as: UTF8.self).contains("private-post-recovery-value"))
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.staticTexts["permissions-empty"].waitForExistence(timeout: 10))
        _ = try terminateOwnedPermissionChild(started: started)
        XCTAssertTrue(panels.wait(for: .notRunning, timeout: 10)); app.activate()
        let restart = app.buttons["restart-browser"]
        XCTAssertTrue(restart.waitForExistence(timeout: 10)); XCTAssertTrue(restart.isEnabled)
        waitAssistantText(app.staticTexts["recovery-details"], "Windows to restore: 1; tabs: 1. Unsaved page changes will be lost.")
        app.typeKey(.tab, modifierFlags: []); app.typeKey(" ", modifierFlags: [])
        let restored = app.windows["browser-window-2"]
        XCTAssertTrue(restored.waitForExistence(timeout: 20)); waitValue(restored.textFields["address"], fixture.origin + "/posted?kept=1")
        waitPageContent("POST page could not be restored")
        XCTAssertEqual(fixture.records.map(\.method), ["GET", "POST"])
        restored.buttons["reload"].click()
        waitAssistantText(restored.staticTexts["status"], "Form data has expired; submit the form again")
        XCTAssertEqual(fixture.records.count, 2)
        XCTAssertFalse(restored.sheets.buttons["Resend"].exists)
        XCTAssertEqual(NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce").first?.processIdentifier, originalPID)
        XCTAssertNil(storedSessionArchive())
    }
    func testBrowserServiceRecoveryDiscardsPendingAssistantResultAndInstruction() throws {
        let started = Date()
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let assistant = try AssistantModelFixture(); defer { assistant.stop() }; try configureAssistant(assistant)
        app.launchArguments += ["-browser.session.remember", "NO", "-browser.session.reopen", "NO"]
        launch(); enter(fixture.origin + "/assistant")
        openAssistant()
        paste("private-recovery-instruction", into: app.textFields["assistant-instruction"])
        assistant.hold(); app.buttons["assistant-organize"].click(); waitModelRequests(assistant, count: 1)
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.staticTexts["permissions-empty"].waitForExistence(timeout: 10))
        _ = try terminateOwnedPermissionChild(started: started)
        XCTAssertTrue(panels.wait(for: .notRunning, timeout: 10)); app.activate()
        XCTAssertTrue(app.buttons["restart-browser"].waitForExistence(timeout: 10))
        waitAssistantText(app.staticTexts["recovery-details"], "Windows to restore: 1; tabs: 1. Unsaved page changes will be lost.")
        app.typeKey(.tab, modifierFlags: []); app.typeKey(" ", modifierFlags: [])
        let restored = app.windows["browser-window-2"]
        XCTAssertTrue(restored.waitForExistence(timeout: 20)); waitValue(restored.textFields["address"], fixture.origin + "/assistant")
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "enabled == true"), object: restored.buttons["assistant"])], timeout: 15), .completed)
        assistant.release()
        XCTAssertFalse(restored.staticTexts["assistant-result"].exists)
        openAssistant(in: restored)
        waitValue(restored.textFields["assistant-instruction"], "Group the main points.")
        XCTAssertFalse(restored.staticTexts["assistant-result"].exists)
        XCTAssertFalse(restored.buttons["assistant-stop"].exists)
        XCTAssertEqual(assistant.prompts.count, 1, "Recovery cannot silently repeat a pending human-requested AI task")
        restored.buttons["assistant-summarize"].click()
        waitAssistantText(restored.staticTexts["assistant-result"], AssistantModelFixture.summary)
        XCTAssertEqual(assistant.prompts.count, 2)
        XCTAssertEqual(fixture.requests, ["/assistant", "/assistant"])
        XCTAssertNil(storedSessionArchive())
    }
    func testNativeOneShotLongURLKeepsConfirmationReachable() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let manifest = try permissionPackage()
        defer { try? FileManager.default.removeItem(at: manifest.deletingLastPathComponent()) }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        app.launchArguments += ["--extension-manifest", manifest.path]
        launch()
        let url = fixture.origin + "/review?context=" + String(repeating: "document/", count: 900)
        paste(url, into: app.textFields["address"]); app.typeKey(.return, modifierFlags: [])
        waitValue(app.textFields["address"], url); waitValue(app.staticTexts["status"], "Ready")
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.buttons["permission-review-document"].waitForExistence(timeout: 10))
        panels.buttons["permission-review-document"].click()
        XCTAssertTrue(panels.staticTexts["permission-reviewed-url"].waitForExistence(timeout: 10))
        waitPermissionValue(panels.staticTexts["permission-reviewed-url"], url)
        let confirm = panels.buttons["permission-confirm-one-shot"]
        XCTAssertTrue(confirm.isHittable, "A legal long URL must keep the human confirmation reachable")
        confirm.click()
        waitPermissionValue(panels.staticTexts["permissions-notice"], "One DOM read allowed for the reviewed document. This is not a persistent permission.")
        XCTAssertEqual(fixture.requests.count, 1, "Review and confirmation must not refetch the document")
    }
    func testPermissionWindowFailurePreservesTheNavigationDenial() throws {
        var directory = Bundle(for: BrowserUITests.self).bundleURL.deletingLastPathComponent()
        var core: URL?
        while directory.path != "/" {
            let candidate = directory.appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-core")
            if FileManager.default.isExecutableFile(atPath: candidate.path) { core = candidate; break }
            directory.deleteLastPathComponent()
        }
        app.launchArguments += ["--core-exe", try XCTUnwrap(core).path]
        launch(); enter("http://malware.test/")
        let status = app.staticTexts["status"]
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "value BEGINSWITH 'Navigation blocked:'"), object: status)], timeout: 15), .completed)
        let denial = try XCTUnwrap(status.value as? String)
        app.buttons["permissions"].click()
        XCTAssertEqual(status.value as? String, denial, "A chrome action must preserve the mandatory navigation denial")
        let ok = app.sheets["alert"].buttons["OK"]
        XCTAssertTrue(ok.waitForExistence(timeout: 10), app.debugDescription)
        ok.click(); waitValue(status, denial)
    }

    private func chooseFileAtPath(_ path: String, selectAll: Bool = false) throws {
        let panel = app.sheets["open-panel"]
        XCTAssertTrue(panel.waitForExistence(timeout: 10),app.debugDescription)
        panel.typeKey("g",modifierFlags: [.command,.shift])
        let folder = app.sheets["GoToWindow"].textFields["PathTextField"]
        XCTAssertTrue(folder.waitForExistence(timeout: 10),app.debugDescription)
        folder.click(); folder.typeKey("a",modifierFlags: .command)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(path,forType: .string)
        folder.typeKey("v",modifierFlags: .command); folder.typeKey(.return,modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),object: app.sheets["GoToWindow"])],timeout: 10),.completed)
        if selectAll { panel.typeKey("a",modifierFlags: .command) }
        let choose = panel.buttons["Choose"]
        XCTAssertTrue(choose.waitForExistence(timeout: 10),app.debugDescription)
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"),object: choose)],timeout: 10),.completed,panel.debugDescription)
        let image = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        image.name = "macos-native-file-picker"; image.lifetime = .keepAlways; add(image)
        choose.click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),object: panel)],timeout: 10),.completed)
    }

    func testNativeFilePickerUploadsBinaryAndRetainsOnlyBasenames() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bi-file-ui-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("chosen-中文.bin")
        let bytes = Data([0,255,13,10,7]); try bytes.write(to: file)
        let second = root.appendingPathComponent("second-👨‍👩‍👧‍👦.txt")
        let secondBytes = Data("one\ntwo\rthree".utf8); try secondBytes.write(to: second)
        app.launchArguments += ["-AppleLanguages","(en)","-AppleLocale","en_US"]
        launch(); enter(fixture.origin + "/file-input")
        let upload = app.groups["page"].buttons["Upload files"]
        XCTAssertTrue(upload.waitForExistence(timeout: 15),app.debugDescription)
        XCTAssertFalse(app.groups["page"].buttons["Disabled file"].isEnabled)
        upload.click(); try chooseFileAtPath(file.path,selectAll: true)
        waitValue(upload,file.lastPathComponent + ", " + second.lastPathComponent)
        XCTAssertFalse((upload.value as? String ?? "").contains(root.path))
        waitValue(app.groups["page"].textFields["Retained editor"],"retained 中文")
        let image = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        image.name = "macos-file-selected-page"; image.lifetime = .keepAlways; add(image)
        XCTAssertEqual(fixture.requests,["/file-input"])
        app.groups["page"].buttons["Send files"].click()
        waitPageContent("Files received")
        XCTAssertEqual(fixture.requests,["/file-input","/upload-received"])
        let request = try XCTUnwrap(fixture.records.last)
        XCTAssertEqual(request.method,"POST")
        XCTAssertNotNil(request.body.range(of: bytes))
        XCTAssertNotNil(request.body.range(of: secondBytes))
        XCTAssertNotNil(request.body.range(of: Data("filename=\"\(second.lastPathComponent)\"".utf8)))
        XCTAssertNotNil(request.body.range(of: Data("filename=\"\(file.lastPathComponent)\"".utf8)))
        XCTAssertNil(request.body.range(of: Data(root.path.utf8)))
        XCTAssertTrue(request.headers.lowercased().contains("multipart/form-data; boundary="))
    }

    func testNativeFilePickerCancelKeyboardAndResetPreserveDocument() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bi-file-cancel-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("retained.txt"); try Data("chosen content".utf8).write(to: file)
        app.launchArguments += ["-AppleLanguages","(en)","-AppleLocale","en_US"]
        launch(); enter(fixture.origin + "/file-input")
        let upload = app.groups["page"].buttons["Upload files"]
        XCTAssertTrue(upload.waitForExistence(timeout: 15))
        upload.click(); try chooseFileAtPath(file.path); waitValue(upload,file.lastPathComponent)
        // Cancelling a later picker preserves the existing file selection.
        upload.click()
        let panel = app.sheets["open-panel"]
        XCTAssertTrue(panel.buttons["Cancel"].waitForExistence(timeout: 10)); panel.buttons["Cancel"].click()
        waitValue(upload,file.lastPathComponent)
        // The page remains focused on its file button; Space is a native gesture.
        app.typeKey(" ",modifierFlags: [])
        XCTAssertTrue(panel.buttons["Cancel"].waitForExistence(timeout: 10),app.debugDescription); panel.buttons["Cancel"].click()
        app.groups["page"].buttons["Reset files"].click(); waitValue(upload,"")
        waitValue(app.groups["page"].textFields["Retained editor"],"retained 中文")
        XCTAssertEqual(fixture.requests,["/file-input"])
    }
    func testNativePrintPanelPaperOrientationScaleAndSavePDF() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        app.launchArguments += ["-AppleLanguages","(en)","-AppleLocale","en_US"]
        launch(); enter(fixture.origin + "/printing")
        XCTAssertTrue(app.groups["page"].textFields["Print editor"].waitForExistence(timeout: 15))
        waitValue(app.staticTexts["status"],"Ready")
        let requests = fixture.requests
        app.typeKey("p",modifierFlags: .command)
        let panel = printPanel
        XCTAssertTrue(panel.buttons["Cancel"].waitForExistence(timeout: 15),app.debugDescription)
        let landscape = panel.radioButtons[" Landscape"]
        XCTAssertTrue(landscape.exists,panel.debugDescription); landscape.click()
        let four = panel.radioButtons["All 4 Pages"]
        XCTAssertTrue(four.waitForExistence(timeout: 15),panel.debugDescription)
        let scaleIndex = try XCTUnwrap(panel.textFields.allElementsBoundByIndex.firstIndex { ($0.value as? String) == "100%" })
        let scale = panel.textFields.element(boundBy: scaleIndex)
        XCTAssertTrue(scale.exists); scale.click(); scale.typeKey("a",modifierFlags: .command); scale.typeText("80"); scale.typeKey(.tab,modifierFlags: [])
        XCTAssertTrue(panel.radioButtons["All 3 Pages"].waitForExistence(timeout: 15),panel.debugDescription)
        let paper = panel.popUpButtons.matching(NSPredicate(format: "value BEGINSWITH 'US Letter'")).firstMatch
        XCTAssertTrue(paper.exists); paper.click()
        let a4 = app.menuItems.matching(NSPredicate(format: "title BEGINSWITH 'A4'")).firstMatch
        XCTAssertTrue(a4.waitForExistence(timeout: 5),app.debugDescription); a4.click()
        let preview = XCTAttachment(screenshot: app.screenshot()); preview.name = "macos-native-print-settings"; preview.lifetime = .keepAlways; add(preview)
        panel.buttons["PDF"].click()
        XCTAssertTrue(app.buttons["Save"].waitForExistence(timeout: 10),app.debugDescription)
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bi-print-ui-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("BlueIce-Print.pdf")
        let save = app.sheets["save-panel"]
        XCTAssertTrue(save.buttons["Save"].isEnabled)
        XCTAssertFalse((save.textFields["saveAsNameTextField"].value as? String ?? "/").contains("/"))
        paste(file.lastPathComponent,into: save.textFields["saveAsNameTextField"])
        let expand = save.disclosureTriangles["NS_OPEN_SAVE_DISCLOSURE_TRIANGLE"]
        if (expand.value as? NSNumber)?.intValue == 0 { expand.click() }
        save.typeKey("g",modifierFlags: [.command,.shift])
        let folder = app.sheets["GoToWindow"].textFields["PathTextField"]
        XCTAssertTrue(folder.waitForExistence(timeout: 10),app.debugDescription)
        folder.click(); folder.typeKey("a",modifierFlags: .command); NSPasteboard.general.clearContents(); NSPasteboard.general.setString(root.path,forType: .string); folder.typeKey("v",modifierFlags: .command); folder.typeKey(.return,modifierFlags: [])
        save.buttons["Save"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),object: panel)],timeout: 15),.completed)
        // Saving can hide the panel before NSPrintOperation finishes. Menu
        // validation follows user events; do not hold a stale menu snapshot
        // open while waiting for the print operation's completion.
        let printableAgain = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            self.app.typeKey(.escape,modifierFlags: [])
            self.app.menuBars.menuBarItems["File"].click()
            return self.app.menuItems["Print…"].isEnabled
        },object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [printableAgain],timeout: 15),.completed,app.debugDescription)
        app.menuItems["Print…"].click()
        let secondCancel = printPanel.buttons["Cancel"]
        XCTAssertTrue(secondCancel.waitForExistence(timeout: 15),app.debugDescription)
        secondCancel.click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),object: panel)],timeout: 15),.completed)
        XCTAssertTrue(app.groups["page"].textFields["Print editor"].waitForExistence(timeout: 15))
        let document = try XCTUnwrap(CGPDFDocument(file as CFURL))
        let pdfAttachment = XCTAttachment(data: try Data(contentsOf: file),uniformTypeIdentifier: "com.adobe.pdf"); pdfAttachment.name = "macos-core-print.pdf"; pdfAttachment.lifetime = .keepAlways; add(pdfAttachment)
        print("PRINTED_PDF_PAGES=\(document.numberOfPages) BOX=\(document.page(at: 1)?.getBoxRect(.mediaBox) ?? .zero)")
        XCTAssertEqual(document.numberOfPages,3)
        let first = try XCTUnwrap(document.page(at: 1))
        let size = first.getBoxRect(.mediaBox).size
        XCTAssertEqual(size.width,842,accuracy: 1); XCTAssertEqual(size.height,595,accuracy: 1)
        XCTAssertEqual(try Data(contentsOf: file).prefix(5),Data("%PDF-".utf8))
        var bytes = [UInt8](repeating: 255,count: Int(size.width) * Int(size.height) * 4)
        let context = try XCTUnwrap(CGContext(data: &bytes,width: Int(size.width),height: Int(size.height),bitsPerComponent: 8,bytesPerRow: Int(size.width) * 4,space: CGColorSpaceCreateDeviceRGB(),bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.drawPDFPage(first)
        var red = 0; var blue = 0
        for i in stride(from: 0,to: bytes.count,by: 4) {
            if bytes[i] > 240 && bytes[i+1] < 15 && bytes[i+2] < 15 { red += 1 }
            if bytes[i] < 15 && bytes[i+1] < 15 && bytes[i+2] > 240 { blue += 1 }
        }
        XCTAssertGreaterThan(red,1000); XCTAssertEqual(blue,0)
        XCTAssertEqual(app.groups["page"].textFields["Print editor"].value as? String,"retained 中文")
        waitValue(app.textFields["address"],fixture.origin + "/printing")
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot()); attachment.name = "macos-print-restored-page"; attachment.lifetime = .keepAlways; add(attachment)
        XCTAssertEqual(fixture.requests,requests)
    }

    func testNativePrintPanelCancelPreservesEditedDocument() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        app.launchArguments += ["-AppleLanguages","(en)","-AppleLocale","en_US"]
        launch(); enter(fixture.origin + "/printing")
        let editor = app.groups["page"].textFields["Print editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 15)); paste("Print retained 中文",into: editor); waitValue(editor,"Print retained 中文")
        let requests = fixture.requests
        app.menuBars.menuBarItems["File"].click(); app.menuItems["Print…"].click()
        let cancel = printPanel.buttons["Cancel"]
        XCTAssertTrue(cancel.waitForExistence(timeout: 15),app.debugDescription)
        let attachment = XCTAttachment(screenshot: app.screenshot()); attachment.name = "macos-native-print-panel"; attachment.lifetime = .keepAlways; add(attachment)
        cancel.click()
        XCTAssertTrue(editor.waitForExistence(timeout: 15)); waitValue(editor,"Print retained 中文")
        waitValue(app.textFields["address"],fixture.origin + "/printing")
        XCTAssertEqual(fixture.requests,requests)
        // Cancelling releases the captured job; a second invocation must work.
        app.typeKey("p",modifierFlags: .command)
        XCTAssertTrue(cancel.waitForExistence(timeout: 15)); cancel.click()
    }

    func testNativeDownloadQuarantineAndFinderOriginSurviveRelaunchWithoutExposingTokens() throws {
        let fixture = try DownloadFixture(); defer { fixture.stop() }
        let root = URL(fileURLWithPath: "/private/tmp/bi-quarantine-ui-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let files = root.appendingPathComponent("files")
        app.launchArguments += ["--downloads-directory",files.path,"--downloads-data-directory",root.appendingPathComponent("data").path]
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); positionWindowForUnobstructedSheet(app.windows["browser-window"])
        app.buttons["downloads"].click()
        XCTAssertTrue(app.staticTexts["downloads-empty"].waitForExistence(timeout: 10))
        startDownload(fixture.origin + "/notes.txt?token=private#private",name: "notes.txt")
        waitDownload(1,"Completed")
        XCTAssertTrue(app.staticTexts["download-quarantine-1"].waitForExistence(timeout: 10))
        let file = files.appendingPathComponent("notes.txt")
        XCTAssertEqual(try Data(contentsOf: file),fixture.bytes)
        let quarantine = try XCTUnwrap(file.resourceValues(forKeys: [.quarantinePropertiesKey]).quarantineProperties)
        XCTAssertEqual(quarantine[kLSQuarantineAgentNameKey as String] as? String,"BlueIce")
        XCTAssertEqual(quarantine[kLSQuarantineAgentBundleIdentifierKey as String] as? String,"cc.blueice.BlueIce")
        let handle = try FileHandle(forReadingFrom: file); defer { try? handle.close() }
        var origins = Data(count: 4096)
        let length = origins.withUnsafeMutableBytes { buffer in
            fgetxattr(handle.fileDescriptor,"com.apple.metadata:kMDItemWhereFroms",buffer.baseAddress,buffer.count,0,0)
        }
        XCTAssertGreaterThan(length,0); guard length > 0 else { return }
        origins.count = length
        XCTAssertEqual(try PropertyListSerialization.propertyList(from: origins,format: nil) as? [String],[fixture.origin + "/notes.txt"])
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        attachment.name = "macos-native-download-quarantine"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["downloads-close"].click(); app.typeKey("q",modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning,timeout: 15))
        launch(); app.buttons["downloads"].click(); waitDownload(1,"Completed")
        XCTAssertTrue(app.staticTexts["download-quarantine-1"].waitForExistence(timeout: 10))
        app.buttons["download-remove-1"].click()
        XCTAssertFalse(app.buttons["download-open-1"].exists)
        XCTAssertEqual(try Data(contentsOf: file),fixture.bytes)
        XCTAssertNotNil(try file.resourceValues(forKeys: [.quarantinePropertiesKey]).quarantineProperties)
    }

    private func openDownloadCredentials() {
        app.buttons["downloads"].click()
        XCTAssertTrue(app.buttons["download-credentials"].waitForExistence(timeout: 10))
        app.buttons["download-credentials"].click()
        XCTAssertTrue(app.staticTexts["download-credentials-title"].waitForExistence(timeout: 10))
    }
    private func reviewDownloadAccount(_ url: String) {
        let field = app.textFields["download-credential-url"]
        field.click(); field.typeKey("a",modifierFlags: .command); field.typeText(url)
        app.buttons["download-credential-review"].click()
        XCTAssertTrue(app.staticTexts["download-credential-account"].waitForExistence(timeout: 10),app.debugDescription)
    }
    private func saveDownloadCredential(_ value: String, result: String = "Credential saved in macOS Keychain.") {
        let field = app.secureTextFields["download-credential-secret"]
        field.click(); field.typeText(value)
        XCTAssertFalse(app.debugDescription.contains(value),"Native accessibility must mask the credential")
        field.typeKey(.return,modifierFlags: [])
        waitAssistantText(app.staticTexts["download-credential-result"],result)
        let displayed = field.value as? String ?? ""
        XCTAssertTrue(displayed.isEmpty || displayed == field.placeholderValue)
    }
    func testNativeDownloadCredentialReviewSaveNamespacesAndRemoval() throws {
        let username = "bi-ui-" + UUID().uuidString
        credentialFixtureAccounts.append(username)
        let url = "sftp://" + username + "@credential-fixture.invalid:2222"
        launch(); openDownloadCredentials(); reviewDownloadAccount(url)
        waitAssistantText(app.staticTexts["download-credential-account"],"Account: " + username + " · credential-fixture.invalid · port 2222")
        XCTAssertEqual(app.popUpButtons["download-credential-kind"].label,"Credential type")
        XCTAssertFalse(app.buttons["download-credential-save"].isEnabled)
        saveDownloadCredential("fixture-password")
        app.secureTextFields["download-credential-secret"].click(); app.typeText("discarded-kind-fixture")
        app.popUpButtons["download-credential-kind"].click(); app.menuItems["SFTP private-key passphrase"].click()
        XCTAssertFalse(app.buttons["download-credential-save"].isEnabled,"Changing credential kind clears the draft")
        saveDownloadCredential("fixture-passphrase")
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        attachment.name = "macos-native-download-credentials"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["download-credential-remove"].click()
        waitAssistantText(app.staticTexts["download-credential-result"],"Credential removed from macOS Keychain.")
        app.popUpButtons["download-credential-kind"].click(); app.menuItems["SFTP password"].click()
        app.buttons["download-credential-remove"].click()
        waitAssistantText(app.staticTexts["download-credential-result"],"Credential removed from macOS Keychain.")
        app.secureTextFields["download-credential-secret"].click(); app.typeText("discarded-account-fixture")
        reviewDownloadAccount(url.replacingOccurrences(of: "sftp:",with: "ftps:"))
        XCTAssertFalse(app.buttons["download-credential-save"].isEnabled,"Changing account clears the draft")
        XCTAssertEqual(app.popUpButtons["download-credential-kind"].value as? String,"FTPS password")
        saveDownloadCredential("fixture-ftps-password")
        app.buttons["download-credential-remove"].click()
        waitAssistantText(app.staticTexts["download-credential-result"],"Credential removed from macOS Keychain.")
        app.buttons["download-credentials-close"].click()
        XCTAssertTrue(app.staticTexts["downloads-empty"].waitForExistence(timeout: 10),"Managing an account never starts a download")
        app.buttons["downloads-close"].click()
        waitValue(app.textFields["address"],"about:credits")
    }
    func testNativeDownloadCredentialInvalidURLAndDismissedSecrets() throws {
        launch(); openDownloadCredentials()
        for url in ["sftp://alice:credential-marker@host/file","ftp://alice@host/file","ftps://host/file"] {
            let field = app.textFields["download-credential-url"]
            field.click(); field.typeKey("a",modifierFlags: .command); field.typeText(url)
            app.buttons["download-credential-review"].click()
            XCTAssertTrue(app.staticTexts["download-credential-notice"].waitForExistence(timeout: 10))
            waitAssistantText(app.staticTexts["download-credential-notice"],"invalid_request: Enter an SFTP or FTPS URL with a username, without a password, query or fragment.")
            XCTAssertFalse(app.secureTextFields["download-credential-secret"].exists)
        }
        reviewDownloadAccount("sftp://bi-ui-" + UUID().uuidString + "@[::1]:2222")
        app.secureTextFields["download-credential-secret"].click(); app.typeText("unsaved-fixture")
        app.typeKey(.escape,modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),object: app.buttons["download-credentials-close"])],timeout: 5),.completed)
        openDownloadCredentials()
        XCTAssertFalse(app.secureTextFields["download-credential-secret"].exists)
        waitValue(app.textFields["download-credential-url"],"")
        app.buttons["download-credentials-close"].click(); app.buttons["downloads-close"].click()
    }
    func testNativeDownloadCredentialsTraditionalChineseAndNormalRelaunch() throws {
        let index = try XCTUnwrap(app.launchArguments.firstIndex(of: "--interface-language"))
        app.launchArguments[index + 1] = "zh-Hant"
        let username = "bi-ui-" + UUID().uuidString
        credentialFixtureAccounts.append(username)
        let url = "ftps://" + username + "@credential-fixture.invalid:2222"
        launch(); openDownloadCredentials(); reviewDownloadAccount(url)
        waitAssistantText(app.staticTexts["download-credentials-title"],"下載憑證")
        XCTAssertEqual(app.popUpButtons["download-credential-kind"].label,"憑證類型")
        XCTAssertEqual(app.buttons["download-credential-save"].label,"儲存至鑰匙圈")
        saveDownloadCredential("fixture-localized-password",result: "已將憑證儲存至 macOS 鑰匙圈。")
        app.buttons["download-credentials-close"].click(); app.buttons["downloads-close"].click()
        app.typeKey("q",modifierFlags: .command); XCTAssertTrue(app.wait(for: .notRunning,timeout: 15))
        XCTAssertFalse(String(describing: storedPreferences()).contains("fixture-localized-password"))
        launch(); openDownloadCredentials(); reviewDownloadAccount(url)
        XCTAssertFalse(app.buttons["download-credential-save"].isEnabled)
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        attachment.name = "macos-native-download-credentials-zh"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["download-credential-remove"].click()
        waitAssistantText(app.staticTexts["download-credential-result"],"已從 macOS 鑰匙圈移除憑證。")
        app.buttons["download-credentials-close"].click(); app.buttons["downloads-close"].click()
    }

    func testNativeDownloadsTransferControlsPersistenceAndFinderActions() throws {
        let fixture = try DownloadFixture(); defer { fixture.stop() }
        let root = URL(fileURLWithPath: "/private/tmp/bi-download-ui-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let files = root.appendingPathComponent("files")
        app.launchArguments += ["--downloads-directory",files.path,"--downloads-data-directory",root.appendingPathComponent("data").path]
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch()
        positionWindowForUnobstructedSheet(app.windows["browser-window"])
        XCTAssertFalse(FileManager.default.fileExists(atPath: files.path))
        app.menuBars.menuBarItems["File"].click(); app.menuItems["Downloads…"].click()
        XCTAssertTrue(app.staticTexts["downloads-empty"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["download-start"].isEnabled)
        paste("file:///etc/hosts",into: app.textFields["download-url"])
        XCTAssertFalse(app.buttons["download-start"].isEnabled)
        paste(fixture.origin + "/notes.txt",into: app.textFields["download-url"])
        paste("../outside.txt",into: app.textFields["download-name"])
        XCTAssertFalse(app.buttons["download-start"].isEnabled)
        let name = "BlueIce-" + UUID().uuidString + ".txt"
        paste(name,into: app.textFields["download-name"])
        app.buttons["download-start"].click(); waitDownload(1,"Completed")
        let file = files.appendingPathComponent(name)
        XCTAssertEqual(try Data(contentsOf: file),fixture.bytes)
        app.buttons["downloads-refresh"].click(); waitDownload(1,"Completed")
        app.buttons["download-reveal-1"].click()
        let finder = XCUIApplication(bundleIdentifier: "com.apple.finder")
        XCTAssertTrue(finder.descendants(matching: .any).matching(NSPredicate(format: "label == %@",name)).firstMatch.waitForExistence(timeout: 15),finder.debugDescription)
        app.activate()
        let handlerURL = try XCTUnwrap(NSWorkspace.shared.urlForApplication(toOpen: file))
        let handler = XCUIApplication(bundleIdentifier: try XCTUnwrap(Bundle(url: handlerURL)?.bundleIdentifier))
        app.buttons["download-open-1"].click()
        XCTAssertTrue(handler.wait(for: .runningForeground,timeout: 15),handler.debugDescription)
        let document = handler.windows[name]
        XCTAssertTrue(document.waitForExistence(timeout: 15),handler.debugDescription)
        if document.buttons[XCUIIdentifierCloseWindow].exists { document.buttons[XCUIIdentifierCloseWindow].click() }
        app.activate()
        startDownload(fixture.origin + "/large.txt",name: "large.txt")
        waitDownload(2,"Downloading")
        app.buttons["download-pause-2"].click(); waitDownload(2,"Paused")
        XCTAssertTrue(app.buttons["download-resume-2"].exists)
        fixture.setSlow(false)
        app.buttons["download-resume-2"].click(); waitDownload(2,"Completed")
        XCTAssertEqual(try Data(contentsOf: files.appendingPathComponent("large.txt")),fixture.large)
        app.buttons["download-remove-2"].click()
        XCTAssertTrue(FileManager.default.fileExists(atPath: files.appendingPathComponent("large.txt").path))
        startDownload(fixture.origin + "/setup.exe",name: "setup.exe"); waitDownload(3,"Blocked")
        XCTAssertTrue(app.staticTexts["download-blocked-3"].exists)
        XCTAssertFalse(app.buttons["download-open-3"].exists)
        XCTAssertFalse(FileManager.default.fileExists(atPath: files.appendingPathComponent("setup.exe").path))
        app.buttons["download-remove-3"].click()
        fixture.setSlow(true)
        startDownload(fixture.origin + "/large-cancel.txt",name: "cancel.txt"); waitDownload(4,"Downloading")
        app.buttons["download-cancel-4"].click(); waitDownload(4,"Cancelled")
        XCTAssertFalse(FileManager.default.fileExists(atPath: files.appendingPathComponent("cancel.txt.blueice-part").path))
        app.buttons["download-remove-4"].click()
        startDownload(fixture.origin + "/large-exit.txt",name: "exit.txt"); waitDownload(5,"Downloading")
        app.buttons["downloads-close"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: app.buttons["downloads-close"])], timeout: 10), .completed)
        app.windows["browser-window"].buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning,timeout: 15))
        let before = fixture.requests.count
        launch(); positionWindowForUnobstructedSheet(app.windows["browser-window"])
        app.buttons["downloads"].click(); waitDownload(5,"Paused")
        XCTAssertEqual(fixture.requests.count,before,"Reopening the catalog must not restart a transfer")
        waitDownload(1,"Completed")
        XCTAssertFalse(app.staticTexts["download-state-2"].exists)
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        attachment.name = "macos-native-downloads"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["download-cancel-5"].click(); waitDownload(5,"Cancelled")
        app.buttons["download-remove-5"].click()
        app.buttons["download-remove-1"].click()
        XCTAssertTrue(app.staticTexts["downloads-empty"].waitForExistence(timeout: 10))
        XCTAssertEqual(try Data(contentsOf: file),fixture.bytes)
        app.buttons["downloads-close"].click()
    }

    func testNativeDownloadLinkedFilePreservesSourceTabAndClipboard() throws {
        let fixture = try DownloadFixture(); defer { fixture.stop() }
        let root = URL(fileURLWithPath: "/private/tmp/bi-download-link-ui-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let files = root.appendingPathComponent("files")
        app.launchArguments += ["--downloads-directory",files.path,"--downloads-data-directory",root.appendingPathComponent("data").path]
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/links")
        let link = app.groups["page"].links["Download notes"]
        XCTAssertTrue(link.waitForExistence(timeout: 15))
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("Keep clipboard 中文",forType: .string)
        link.rightClick(); chooseContext("Download Linked File")
        waitDownload(1,"Completed")
        XCTAssertEqual(try Data(contentsOf: files.appendingPathComponent("notes.txt")),fixture.bytes)
        XCTAssertEqual(NSPasteboard.general.string(forType: .string),"Keep clipboard 中文")
        app.buttons["downloads-close"].click()
        waitValue(app.textFields["address"],fixture.origin + "/links")
        XCTAssertTrue(link.exists); XCTAssertFalse(app.buttons["tab-2"].exists)
        app.buttons["downloads"].click(); waitDownload(1,"Completed")
        startDownload(fixture.origin + "/large-forced-exit.txt",name: "forced-exit.txt"); waitDownload(2,"Downloading")
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce")
        XCTAssertEqual(running.count,1,"The forced-exit test requires one identifiable BlueIce instance")
        let process = try XCTUnwrap(running.first)
        XCTAssertEqual(kill(process.processIdentifier,SIGKILL),0,"Terminate only this test's BlueIce process")
        XCTAssertTrue(app.wait(for: .notRunning,timeout: 15))
        let catalog = root.appendingPathComponent("data/transfers.json")
        let checkpoint = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let data = try? Data(contentsOf: catalog), let text = String(data: data,encoding: .utf8) else { return false }
            return text.contains("\"paused\"")
        },object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [checkpoint],timeout: 15),.completed,"Owner EOF must checkpoint without GUI cleanup callbacks")
        let before = fixture.requests.count
        launch(); app.buttons["downloads"].click(); waitDownload(2,"Paused")
        XCTAssertEqual(fixture.requests.count,before,"Forced-exit recovery must require explicit Resume")
        waitDownload(1,"Completed")
        app.buttons["download-cancel-2"].click(); waitDownload(2,"Cancelled")
        app.buttons["downloads-close"].click()
    }

    private func startDownload(_ url: String, name: String) {
        paste(url,into: app.textFields["download-url"]); paste(name,into: app.textFields["download-name"])
        XCTAssertTrue(app.buttons["download-start"].isEnabled); app.buttons["download-start"].click()
    }
    private func waitDownload(_ id: Int, _ state: String) {
        let element = app.staticTexts["download-state-\(id)"]
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == true AND value == %@",state),object: element)],timeout: 20),.completed,app.debugDescription)
    }

    func testProfilesNativeCreateRenameScopedWindowsPersistenceAndRemoval() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch()
        let first = app.windows["browser-window"]
        enter(fixture.origin + "/editing",in: first)
        XCTAssertTrue(first.textFields["Editor"].waitForExistence(timeout: 15))
        paste("Root 中文",into: first.textFields["Editor"]); waitValue(first.textFields["Editor"],"Root 中文")
        app.menuBars.menuBarItems["Profiles"].click(); app.menuItems["New Profile…"].click()
        XCTAssertTrue(first.textFields["profile-name"].waitForExistence(timeout: 10))
        XCTAssertFalse(first.buttons["profile-save"].isEnabled)
        paste("Default",into: first.textFields["profile-name"]); XCTAssertFalse(first.buttons["profile-save"].isEnabled)
        paste("Work",into: first.textFields["profile-name"]); first.buttons["profile-save"].click()
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 10)); waitValue(second.textFields["address"],"about:credits")
        waitValue(second.descendants(matching: .any).matching(identifier: "profile-menu").firstMatch,"Work")
        second.typeKey("n",modifierFlags: .command)
        let third = app.windows["browser-window-3"]
        XCTAssertTrue(third.waitForExistence(timeout: 10)); waitValue(third.textFields["address"],"about:credits")
        waitValue(third.descendants(matching: .any).matching(identifier: "profile-menu").firstMatch,"Work")
        app.menuBars.menuBarItems["Window"].click(); app.menuItems["Window 2"].click()
        second.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(third.exists && first.exists)
        app.menuBars.menuBarItems["Window"].click(); app.menuItems["Window 3"].click()
        chooseProfileAction("Rename…", profile: "Work")
        XCTAssertTrue(third.textFields["profile-name"].waitForExistence(timeout: 10))
        paste("工作",into: third.textFields["profile-name"]); third.buttons["profile-save"].click()
        waitValue(third.descendants(matching: .any).matching(identifier: "profile-menu").firstMatch,"工作")
        waitValue(first.textFields["Editor"],"Root 中文")
        XCTAssertEqual(fixture.requests,["/editing"])
        let attachment = XCTAttachment(screenshot: third.screenshot()); attachment.name = "macos-profile-contexts"; attachment.lifetime = .keepAlways; add(attachment)
        activateWindow(1); first.buttons[XCUIIdentifierCloseWindow].click()
        activateWindow(3); third.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning,timeout: 15))
        launch()
        XCTAssertEqual(app.windows.matching(NSPredicate(format: "identifier BEGINSWITH 'browser-window'")).count,1)
        chooseProfileAction("Open Window", profile: "工作")
        let reopened = app.windows["browser-window-2"]
        XCTAssertTrue(reopened.waitForExistence(timeout: 10)); waitValue(reopened.textFields["address"],"about:credits")
        waitValue(reopened.descendants(matching: .any).matching(identifier: "profile-menu").firstMatch,"工作")
        chooseProfileAction("Remove…", profile: "工作")
        XCTAssertTrue(reopened.buttons["profile-remove"].waitForExistence(timeout: 10))
        reopened.buttons["profile-cancel"].click(); XCTAssertTrue(reopened.exists)
        chooseProfileAction("Remove…", profile: "工作")
        XCTAssertTrue(reopened.buttons["profile-remove"].waitForExistence(timeout: 10)); reopened.buttons["profile-remove"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"),object: reopened)],timeout: 10),.completed)
        XCTAssertTrue(app.windows["browser-window"].exists)
        waitValue(app.windows["browser-window"].textFields["address"],"about:credits")
        XCTAssertEqual(fixture.requests,["/editing"])
    }

    func testSessionRestoresProfilesWindowsGroupsSelectionHistoryAndZoom() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/first"); waitValue(app.textFields["address"], fixture.origin + "/first")
        enter(fixture.origin + "/second"); waitValue(app.textFields["address"], fixture.origin + "/second")
        app.buttons["back"].click(); waitValue(app.textFields["address"], fixture.origin + "/first")
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Page Zoom"].hover(); app.menuItems["150%"].click()
        waitValue(app.buttons["page-zoom"], "150%")
        app.buttons["new-tab-group"].click(); XCTAssertTrue(app.textFields["group-name"].waitForExistence(timeout: 10))
        paste("Study", into: app.textFields["group-name"]); app.buttons["group-color-red"].click(); app.buttons["group-save"].click()
        XCTAssertTrue(app.buttons["tab-group-1"].waitForExistence(timeout: 10))
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        app.buttons["tab-1"].click(); app.buttons["tab-group-1"].click()
        waitValue(app.buttons["tab-group-1"], "Collapsed, 1 tabs, contains selected tab")
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15)); enter(fixture.origin + "/editing", in: second)
        XCTAssertTrue(second.textFields["Editor"].waitForExistence(timeout: 15)); paste("unsaved-form-content", into: second.textFields["Editor"])
        positionWindowForUnobstructedSheet(second)
        app.menuBars.menuBarItems["Profiles"].click(); app.menuItems["New Profile…"].click()
        XCTAssertTrue(app.textFields["profile-name"].waitForExistence(timeout: 10)); paste("Work", into: app.textFields["profile-name"])
        app.buttons["profile-save"].click()
        let third = app.windows["browser-window-3"]
        XCTAssertTrue(third.waitForExistence(timeout: 15), app.debugDescription); enter(fixture.origin + "/appearance", in: third)
        waitValue(third.textFields["address"], fixture.origin + "/appearance")
        let settings = preferenceWindow(); try enableSessionRestoration(settings, windows: 3, tabs: 4)
        settings.buttons[XCUIIdentifierCloseWindow].click(); activateWindow(3)
        let archive = try waitSessionArchive(windows: 3, tabs: 4)
        XCTAssertFalse(String(decoding: archive, as: UTF8.self).contains("unsaved-form-content"))
        let originalFrame = app.windows["browser-window"].frame
        app.typeKey("q", modifierFlags: .command); XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
        app.launch()
        let root = app.windows["browser-window-2"]
        let work = app.windows["browser-window-4"]
        XCTAssertTrue(work.waitForExistence(timeout: 20)); waitValue(work.textFields["address"], fixture.origin + "/appearance")
        XCTAssertEqual(app.windows.matching(NSPredicate(format: "identifier BEGINSWITH 'browser-window'")).count, 3)
        waitValue(work.descendants(matching: .any).matching(identifier: "profile-menu").firstMatch, "Work")
        activateWindow(2); waitValue(root.textFields["address"], fixture.origin + "/first")
        XCTAssertEqual(root.frame.width, originalFrame.width, accuracy: 3)
        XCTAssertEqual(root.frame.height, originalFrame.height, accuracy: 3)
        XCTAssertEqual(root.frame.minX, originalFrame.minX, accuracy: 3)
        XCTAssertEqual(root.frame.minY, originalFrame.minY, accuracy: 3)
        waitValue(root.buttons["page-zoom"], "150%")
        XCTAssertEqual(root.buttons["tab-group-1"].label, "Study")
        waitValue(root.buttons["tab-group-1"], "Collapsed, 1 tabs, contains selected tab")
        XCTAssertTrue(root.buttons["forward"].isEnabled)
        activateWindow(3); waitValue(app.windows["browser-window-3"].textFields["Editor"], "A😀B")
        activateWindow(2); root.buttons["forward"].click(); waitValue(root.textFields["address"], fixture.origin + "/second")
        let attachment = XCTAttachment(screenshot: root.screenshot()); attachment.name = "macos-session-restored"; attachment.lifetime = .keepAlways; add(attachment)
        XCTAssertEqual(fixture.requests, ["/first", "/second", "/first", "/editing", "/appearance", "/first", "/editing", "/appearance", "/second"])
    }

    func testSessionPostMarkerNeverPersistsBodyOrReplaysRequest() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/forms")
        let query = app.groups["page"].textFields["Query"]
        XCTAssertTrue(query.waitForExistence(timeout: 15)); paste("private-post-session-value", into: query)
        app.groups["page"].buttons["Send POST"].click(); waitValue(app.textFields["address"], fixture.origin + "/posted?kept=1")
        XCTAssertEqual(fixture.records.last?.method, "POST")
        XCTAssertTrue(String(decoding: try XCTUnwrap(fixture.records.last?.body), as: UTF8.self).contains("private-post-session-value"))
        let settings = preferenceWindow(); try enableSessionRestoration(settings, windows: 1, tabs: 1)
        settings.buttons[XCUIIdentifierCloseWindow].click()
        let archive = try waitSessionArchive(windows: 1, tabs: 1)
        XCTAssertFalse(String(decoding: archive, as: UTF8.self).contains("private-post-session-value"))
        XCTAssertTrue(String(decoding: archive, as: UTF8.self).contains("\"was_post\":true"))
        app.typeKey("q", modifierFlags: .command); XCTAssertTrue(app.wait(for: .notRunning, timeout: 15)); app.launch()
        let restored = app.windows["browser-window-2"]
        XCTAssertTrue(restored.waitForExistence(timeout: 20)); waitValue(restored.textFields["address"], fixture.origin + "/posted?kept=1")
        waitPageContent("POST page could not be restored")
        restored.buttons["reload"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == 'Form data has expired; submit the form again' OR value == 'Form data has expired; submit the form again'"), object: restored.staticTexts["status"])], timeout: 15), .completed)
        XCTAssertEqual(fixture.records.count, 2); XCTAssertFalse(restored.sheets.buttons["Resend"].exists)
    }

    func testSessionManualRestoreForgetAndInvalidPreferenceRecovery() throws {
        launch(); enter("about:settings"); waitValue(app.textFields["address"], "about:settings")
        let settings = preferenceWindow(); settings.checkBoxes["session-remember"].click()
        settings.buttons[XCUIIdentifierCloseWindow].click(); _ = try waitSessionArchive(windows: 1, tabs: 1)
        app.typeKey("q", modifierFlags: .command); XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
        launch(); app.menuBars.menuBarItems["File"].click(); app.menuItems["Restore Last Session"].click()
        let restored = app.windows["browser-window-2"]
        XCTAssertTrue(restored.waitForExistence(timeout: 15)); waitValue(restored.textFields["address"], "about:settings")
        let panel = preferenceWindow(); panel.buttons["session-forget"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == 0 OR value == '0'"), object: panel.checkBoxes["session-remember"])], timeout: 10), .completed)
        XCTAssertFalse(panel.buttons["session-restore"].isEnabled)
        XCTAssertFalse(panel.checkBoxes["session-reopen"].isEnabled)
        panel.buttons[XCUIIdentifierCloseWindow].click(); app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
        // cfprefsd owns the plist flush; verify durability after normal exit.
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in self.storedSessionArchive() == nil }, object: nil)], timeout: 30), .completed)
        launch(); let forgotten = preferenceWindow()
        XCTAssertTrue(NSPredicate(format: "value == 0 OR value == '0'").evaluate(with: forgotten.checkBoxes["session-remember"]))
        XCTAssertFalse(forgotten.buttons["session-restore"].isEnabled)
        forgotten.buttons[XCUIIdentifierCloseWindow].click(); app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
        app.launchArguments += ["-browser.session.archive", "malformed-session-fixture"]
        launch(); let recovery = preferenceWindow()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label CONTAINS 'unavailable' OR value CONTAINS 'unavailable'"), object: recovery.staticTexts["session-status"])], timeout: 10), .completed)
        XCTAssertNil(storedSessionArchive())
        XCTAssertFalse(recovery.buttons["session-restore"].isEnabled); recovery.buttons["session-forget"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in self.storedSessionArchive() == nil }, object: nil)], timeout: 10), .completed)
    }

    func testSessionRestorationRechecksChangedContentAndKeepsDenialVisible() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/session-change"); waitPageContent("Initially reviewed page")
        let settings = preferenceWindow(); try enableSessionRestoration(settings, windows: 1, tabs: 1)
        settings.buttons[XCUIIdentifierCloseWindow].click(); _ = try waitSessionArchive(windows: 1, tabs: 1)
        app.typeKey("q", modifierFlags: .command); XCTAssertTrue(app.wait(for: .notRunning, timeout: 15)); app.launch()
        let window = app.windows["browser-window-2"]
        XCTAssertTrue(window.waitForExistence(timeout: 20))
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label CONTAINS 'Navigation blocked' OR value CONTAINS 'Navigation blocked'"), object: window.staticTexts["status"])], timeout: 15), .completed)
        XCTAssertFalse(pageContent("Initially reviewed page").exists)
        let archive = try waitSessionArchive(windows: 1, tabs: 1)
        XCTAssertTrue(String(decoding: archive, as: UTF8.self).contains("/session-change"))
        XCTAssertEqual(fixture.requests, ["/session-change", "/session-change"])
        XCTAssertTrue(window.buttons["add-tab"].isEnabled)
    }

    private func enableSessionRestoration(_ settings: XCUIElement, windows: Int, tabs: Int) throws {
        let remember = settings.checkBoxes["session-remember"], reopen = settings.checkBoxes["session-reopen"]
        remember.click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == 1 OR value == '1'"), object: remember)], timeout: 10), .completed)
        _ = try waitSessionArchive(windows: windows, tabs: tabs)
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: reopen)], timeout: 10), .completed)
        reopen.click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == 1 OR value == '1'"), object: reopen)], timeout: 10), .completed)
    }

    private var ownedPreferenceFile: URL? {
        let prefix = "cc.blueice.uitests."
        guard preferenceDomain.hasPrefix(prefix), UUID(uuidString: String(preferenceDomain.dropFirst(prefix.count))) != nil,
              let user = getpwuid(getuid()), let home = user.pointee.pw_dir else { return nil }
        return URL(fileURLWithPath: String(cString: home)).appendingPathComponent("Library/Preferences").appendingPathComponent(preferenceDomain + ".plist")
    }
    private func storedPreferences() -> [String: Any]? {
        // The generated UI runner is sandboxed. Read the app's isolated test
        // domain, rather than the runner's separate suite with the same name.
        guard let file = ownedPreferenceFile else { return nil }
        guard let data = try? Data(contentsOf: file),
              let values = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any] else { return nil }
        return values
    }
    private func storedSessionArchive() -> Data? { storedPreferences()?["browser.session.archive"] as? Data }

    private func waitSessionArchive(windows: Int, tabs: Int) throws -> Data {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let data = self.storedSessionArchive(),
                  let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let profiles = root["profiles"] as? [[String: Any]] else { return false }
            let savedWindows = profiles.flatMap { $0["windows"] as? [[String: Any]] ?? [] }
            return savedWindows.count == windows && savedWindows.reduce(0) { $0 + ($1["tabs"] as? [Any] ?? []).count } == tabs
        }, object: nil)
        let result = XCTWaiter.wait(for: [expectation], timeout: 15)
        if result != .completed { _ = preferenceWindow() }
        XCTAssertEqual(result, .completed, app.debugDescription)
        return try XCTUnwrap(storedSessionArchive())
    }

    func testUndoRedoNativeMenuKeyboardUnicodeAndContextMenu() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/editing")
        let field = app.groups["page"].textFields["Editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 15)); field.click()
        app.menuBars.menuBarItems["Edit"].click(); XCTAssertFalse(app.menuItems["Undo"].isEnabled); app.typeKey(.escape, modifierFlags: [])
        paste("中文👨‍👩‍👧‍👦", into: field)
        app.menuBars.menuBarItems["Edit"].click(); XCTAssertTrue(app.menuItems["Undo"].isEnabled); app.menuItems["Undo"].click()
        waitValue(field, "A😀B")
        app.typeKey("z", modifierFlags: [.command, .shift]); waitValue(field, "中文👨‍👩‍👧‍👦")
        field.rightClick(); chooseContext("Undo"); waitValue(field, "A😀B")
        field.rightClick(); chooseContext("Redo"); waitValue(field, "中文👨‍👩‍👧‍👦")
        field.typeKey("z", modifierFlags: .command); waitValue(field, "A😀B")
        paste("new branch", into: field)
        app.menuBars.menuBarItems["Edit"].click(); XCTAssertFalse(app.menuItems["Redo"].isEnabled); app.typeKey(.escape, modifierFlags: [])
        XCTAssertEqual(fixture.requests, ["/editing"])
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot()); attachment.name = "macos-undo-redo"; attachment.lifetime = .keepAlways; add(attachment)
    }

    func testUndoRedoClipboardTextareaPasswordReadonlyAndAddressResponder() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/editing")
        let page = app.groups["page"], notes = page.textFields["Notes"]
        XCTAssertTrue(notes.waitForExistence(timeout: 15))
        paste("中文\nשלום\n👩‍👩‍👧‍👦", into: notes)
        notes.typeKey("a", modifierFlags: .command); notes.typeKey("x", modifierFlags: .command); waitValue(notes, "")
        notes.typeKey("z", modifierFlags: .command); waitValue(notes, "中文\nשלום\n👩‍👩‍👧‍👦")
        notes.typeKey("z", modifierFlags: .command); waitValue(notes, "first\nsecond")
        notes.typeKey("z", modifierFlags: [.command, .shift]); waitValue(notes, "中文\nשלום\n👩‍👩‍👧‍👦")
        let address = app.textFields["address"]
        paste("address draft", into: address)
        address.typeKey("z", modifierFlags: .command); waitValue(address, fixture.origin + "/editing")
        waitValue(notes, "中文\nשלום\n👩‍👩‍👧‍👦")
        let secret = page.secureTextFields["Secret"]
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("temporary-secret", forType: .string)
        secret.click(); secret.typeKey("a", modifierFlags: .command); secret.typeKey("v", modifierFlags: .command)
        app.menuBars.menuBarItems["Edit"].click(); XCTAssertTrue(app.menuItems["Undo"].isEnabled); app.menuItems["Undo"].click()
        app.menuBars.menuBarItems["Edit"].click(); XCTAssertTrue(app.menuItems["Redo"].isEnabled); app.menuItems["Redo"].click()
        // AppKit's idle state does not acknowledge the core's queued Redo.
        // Revalidate the native menu until that edit has consumed its redo
        // entry before starting a new pointer focus transition.
        let redoFinished = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            self.app.typeKey(.escape,modifierFlags: [])
            self.app.menuBars.menuBarItems["Edit"].click()
            return self.app.menuItems["Undo"].isEnabled && !self.app.menuItems["Redo"].isEnabled
        },object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [redoFinished],timeout: 15),.completed,app.debugDescription)
        app.typeKey(.escape,modifierFlags: [])
        XCTAssertFalse(app.debugDescription.contains("temporary-secret")); XCTAssertFalse(app.debugDescription.contains("private-fixture-secret"))
        let readonly = page.textFields["Readonly"]; readonly.click()
        let readonlyFocused = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            readonly.debugDescription.components(separatedBy: "\n").first?.contains("Keyboard Focused") == true
        },object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [readonlyFocused],timeout: 15),.completed,app.debugDescription)
        app.menuBars.menuBarItems["Edit"].click(); XCTAssertFalse(app.menuItems["Undo"].isEnabled); XCTAssertFalse(app.menuItems["Redo"].isEnabled); app.typeKey(.escape, modifierFlags: [])
        readonly.typeKey("z", modifierFlags: .command); waitValue(readonly, "locked")
        XCTAssertEqual(fixture.requests, ["/editing"])
    }

    func testUndoRedoTabIsolationWindowTransferAndReload() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch()
        let first = app.windows["browser-window"]
        enter(fixture.origin + "/editing", in: first)
        let field = first.groups["page"].textFields["Editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 15)); paste("First tab", into: field)
        app.typeKey("t", modifierFlags: .command); enter(fixture.origin + "/editing", in: first)
        let other = first.groups["page"].textFields["Editor"]
        XCTAssertTrue(other.waitForExistence(timeout: 15)); paste("Second tab", into: other)
        other.typeKey("z", modifierFlags: .command); waitValue(other, "A😀B")
        first.buttons["tab-1"].click(); waitValue(field, "First tab")
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15)); waitValue(second.textFields["address"], "about:credits")
        activateWindow(1); transferTab(1, from: first, to: "Window 2")
        let moved = second.groups["page"].textFields["Editor"]
        XCTAssertTrue(moved.waitForExistence(timeout: 15)); activateWindow(2); moved.click()
        moved.typeKey("z", modifierFlags: .command); waitValue(moved, "A😀B")
        moved.typeKey("z", modifierFlags: [.command, .shift]); waitValue(moved, "First tab")
        second.buttons["reload"].click(); waitValue(moved, "A😀B"); moved.click()
        app.menuBars.menuBarItems["Edit"].click(); XCTAssertFalse(app.menuItems["Undo"].isEnabled); XCTAssertFalse(app.menuItems["Redo"].isEnabled); app.typeKey(.escape, modifierFlags: [])
        activateWindow(1); waitValue(first.groups["page"].textFields["Editor"], "A😀B")
        first.groups["page"].textFields["Editor"].click(); app.typeKey("z", modifierFlags: [.command, .shift]); waitValue(first.groups["page"].textFields["Editor"], "Second tab")
        XCTAssertEqual(fixture.requests, ["/editing", "/editing", "/editing"])
    }

    private var credentialFixtureAccounts: [String] = []
    private var app: XCUIApplication!
    private var originalInputSource: TISInputSource?
    private var preferenceDomain = ""
    private var assistantSettingsFile: URL!
    private var assistantControlSocket: URL!

    func testTabDragReordersWithoutChangingSelectedEditorFindOrZoom() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/editing")
        let window = app.windows["browser-window"], editor = window.groups["page"].textFields["Editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 15)); paste("Drag 😀 中文", into: editor)
        waitValue(editor, "Drag 😀 中文")
        app.typeKey("+", modifierFlags: .command); app.typeKey("+", modifierFlags: .command); app.typeKey("+", modifierFlags: .command)
        waitValue(window.buttons["page-zoom"], "150%")
        window.buttons["add-tab"].click(); enter("about:settings")
        window.buttons["add-tab"].click(); waitValue(window.textFields["address"], "about:credits")
        window.buttons["tab-1"].click(); waitValue(editor, "Drag 😀 中文")
        app.typeKey("f", modifierFlags: .command)
        let find = window.searchFields["find-query"]
        XCTAssertTrue(find.waitForExistence(timeout: 10)); find.typeText("unlikely-query")
        paste("unsent address 😀", into: window.textFields["address"])
        let first = window.buttons["tab-1"], second = window.buttons["tab-2"], third = window.buttons["tab-3"]
        XCTAssertTrue(window.frame.contains(first.frame)); XCTAssertTrue(window.frame.contains(third.frame))
        XCTAssertEqual(try tabInsertionPixels(first, in: window), 0, "No insertion line is visible before a drag")
        third.coordinate(withNormalizedOffset: CGVector(dx: 0.4, dy: 0.5)).press(forDuration: 0.6,
            thenDragTo: first.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5)))
        let order = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            third.frame.minX < first.frame.minX && first.frame.minX < second.frame.minX
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [order], timeout: 15), .completed,
                       "A real native drag must update canonical window-local order")
        XCTAssertEqual(first.value as? String, "Selected")
        waitValue(editor, "Drag 😀 中文"); waitValue(find, "unlikely-query")
        waitValue(window.buttons["page-zoom"], "150%")
        waitValue(window.textFields["address"], "unsent address 😀")
        XCTAssertEqual(fixture.requests, ["/editing"], "Organization never reloads the page")
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = "macos-tab-drag-order"; attachment.lifetime = .keepAlways; add(attachment)
        XCTAssertEqual(try tabInsertionPixels(first, in: window), 0, "The insertion line must disappear after the drop completes")
    }

    private func tabInsertionPixels(_ tab: XCUIElement, in window: XCUIElement) throws -> Int {
        // The tab title has six points of outer padding. Sample the insertion
        // line's three-point strip there, away from text and the close button.
        let frame = tab.frame, bounds = window.frame
        let area = CGRect(x: (frame.minX - bounds.minX - 6) / bounds.width,
                          y: (frame.minY - bounds.minY) / bounds.height,
                          width: 3 / bounds.width, height: frame.height / bounds.height)
        let accent = try XCTUnwrap(NSColor.controlAccentColor.usingColorSpace(.sRGB))
        return try colorCount(try XCTUnwrap(window.screenshot().image.tiffRepresentation),
            red: accent.redComponent, green: accent.greenComponent, blue: accent.blueComponent, area: area)
    }

    func testTabDragIntoCollapsedGroupAndBackToUngroupedEndPreservesPage() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/editing")
        let window = app.windows["browser-window"], editor = window.groups["page"].textFields["Editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 15)); paste("Grouped drag 中文", into: editor)
        window.buttons["new-tab-group"].click()
        XCTAssertTrue(window.textFields["group-name"].waitForExistence(timeout: 10))
        paste("Drag Study", into: window.textFields["group-name"]); window.buttons["group-save"].click()
        let group = window.buttons["tab-group-1"]
        XCTAssertTrue(group.waitForExistence(timeout: 10)); group.click()
        waitValue(group, "Collapsed, 1 tabs, contains selected tab")
        window.buttons["add-tab"].click(); waitValue(window.textFields["address"], "about:credits")
        window.buttons["tab-2"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.6,
            thenDragTo: group.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)))
        waitValue(group, "Collapsed, 2 tabs, contains selected tab")
        XCTAssertFalse(window.buttons["tab-1"].exists); XCTAssertFalse(window.buttons["tab-2"].exists)
        waitValue(window.textFields["address"], "about:credits")
        group.click(); waitValue(group, "Expanded, 2 tabs, contains selected tab")
        XCTAssertLessThan(window.buttons["tab-1"].frame.minX, window.buttons["tab-2"].frame.minX)
        window.buttons["tab-2"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.6,
            thenDragTo: window.buttons["add-tab"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)))
        waitValue(group, "Expanded, 1 tabs")
        XCTAssertLessThan(window.buttons["tab-2"].frame.minX, group.frame.minX)
        XCTAssertFalse(window.buttons["tab-3"].exists, "An end drop cannot activate the new-tab button")
        window.buttons["tab-1"].click(); waitValue(editor, "Grouped drag 中文")
        XCTAssertEqual(fixture.requests, ["/editing"])
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = "macos-tab-drag-groups"; attachment.lifetime = .keepAlways; add(attachment)
    }

    func testTabPlacementKeyboardContextMenuAndNormalRestartKeepOrder() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch()
        let window = app.windows["browser-window"]
        window.buttons["add-tab"].click(); enter("about:settings")
        window.buttons["add-tab"].click()
        paste(fixture.origin + "/first", into: window.textFields["address"])
        window.textFields["address"].typeKey(.return, modifierFlags: [])
        waitValue(window.textFields["address"], fixture.origin + "/first")
        let first = window.buttons["tab-1"], second = window.buttons["tab-2"], third = window.buttons["tab-3"]
        func order(_ a: XCUIElement, _ b: XCUIElement, _ c: XCUIElement) {
            XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                a.frame.minX < b.frame.minX && b.frame.minX < c.frame.minX
            }, object: nil)], timeout: 15), .completed)
        }
        app.typeKey(.leftArrow, modifierFlags: [.command, .control]); order(first, third, second)
        app.typeKey(.leftArrow, modifierFlags: [.command, .control]); order(third, first, second)
        third.rightClick(); chooseChromeContext("Move Tab Right"); order(first, third, second)
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Move Tab Right"].click(); order(first, second, third)
        app.typeKey(.leftArrow, modifierFlags: [.command, .control]); order(first, third, second)
        app.typeKey(.leftArrow, modifierFlags: [.command, .control]); order(third, first, second)
        app.menuBars.menuBarItems["View"].click()
        XCTAssertFalse(app.menuItems["Move Tab Left"].isEnabled); app.typeKey(.escape, modifierFlags: [])
        XCTAssertEqual(third.value as? String, "Selected")
        let settings = preferenceWindow(); try enableSessionRestoration(settings, windows: 1, tabs: 3)
        settings.buttons[XCUIIdentifierCloseWindow].click()
        let archive = try waitSessionArchive(windows: 1, tabs: 3)
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: archive) as? [String: Any])
        let profiles = try XCTUnwrap(root["profiles"] as? [[String: Any]])
        let windows = try XCTUnwrap(profiles.first?["windows"] as? [[String: Any]])
        let tabs = try XCTUnwrap(windows.first?["tabs"] as? [[String: Any]])
        let urls = try tabs.map { tab -> String in
            let history = try XCTUnwrap(tab["history"] as? [String: Any])
            let entries = try XCTUnwrap(history["entries"] as? [[String: Any]])
            let cursor = try XCTUnwrap(history["cursor"] as? Int)
            XCTAssertTrue(entries.indices.contains(cursor))
            return try XCTUnwrap(entries[cursor]["url"] as? String)
        }
        XCTAssertEqual(urls, [fixture.origin + "/first", "about:credits", "about:settings"])
        XCTAssertEqual(windows.first?["selected"] as? Int, 0)
        app.typeKey("q", modifierFlags: .command); XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
        app.launch()
        let reopened = app.windows["browser-window-2"]
        XCTAssertTrue(reopened.waitForExistence(timeout: 15))
        func tab(_ url: String) -> XCUIElement {
            reopened.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'tab-' AND label == %@", url)).firstMatch
        }
        let restored = tab(fixture.origin + "/first"), credits = tab("about:credits"), preferences = tab("about:settings")
        XCTAssertTrue(preferences.waitForExistence(timeout: 15)); order(restored, credits, preferences)
        waitValue(reopened.textFields["address"], fixture.origin + "/first"); waitValue(restored, "Selected")
        XCTAssertFalse(window.exists, "Restoration replaces the startup window with a fresh core identity")
        XCTAssertEqual(fixture.requests, ["/first", "/first"])
        let attachment = XCTAttachment(screenshot: reopened.screenshot())
        attachment.name = "macos-tab-order-restored"; attachment.lifetime = .keepAlways; add(attachment)
    }

    private func positionTabDragWindow(_ window: XCUIElement, x: CGFloat) {
        let corner = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1)).withOffset(CGVector(dx: -2, dy: -2))
        corner.click(forDuration: 0.2, thenDragTo: corner.withOffset(CGVector(dx: 760 - window.frame.width, dy: 600 - window.frame.height)))
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            abs(window.frame.width - 760) < 3 && abs(window.frame.height - 600) < 3
        }, object: window)], timeout: 10), .completed)
        let title = window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.02))
        title.click(forDuration: 0.1, thenDragTo: title.withOffset(CGVector(dx: x - window.frame.minX, dy: 40 - window.frame.minY)))
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            abs(window.frame.minX - x) < 3 && abs(window.frame.minY - 40) < 3
        }, object: window)], timeout: 10), .completed)
    }

    func testTabDragTransfersLiveWindowStateAndRefusesDifferentProfile() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/editing")
        let first = app.windows["browser-window"]
        let editor = first.groups["page"].textFields["Editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 15)); paste("Across windows 😀", into: editor)
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Page Zoom"].hover(); app.menuItems["150%"].click()
        waitValue(first.buttons["page-zoom"], "150%")
        app.typeKey("f", modifierFlags: .command)
        XCTAssertTrue(first.searchFields["find-query"].waitForExistence(timeout: 10)); first.searchFields["find-query"].typeText("transfer")
        positionTabDragWindow(first, x: 40)
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15)); waitValue(second.textFields["address"], "about:credits")
        positionTabDragWindow(second, x: 820)
        let source = first.buttons["tab-1"], target = second.buttons["tab-2"]
        XCTAssertFalse(first.frame.intersects(second.frame))
        source.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.6,
            thenDragTo: target.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5)))
        waitValue(second.textFields["address"], fixture.origin + "/editing")
        let moved = second.groups["page"].textFields["Editor"]
        XCTAssertTrue(moved.waitForExistence(timeout: 15)); waitValue(moved, "Across windows 😀")
        waitValue(second.searchFields["find-query"], "transfer"); waitValue(second.buttons["page-zoom"], "150%")
        XCTAssertLessThan(second.buttons["tab-1"].frame.minX, target.frame.minX)
        XCTAssertFalse(first.buttons["tab-1"].exists)
        activateWindow(1); first.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: first)], timeout: 10), .completed)
        activateWindow(2); paste("After source closes 中文", into: moved); waitValue(moved, "After source closes 中文")
        app.menuBars.menuBarItems["Profiles"].click(); app.menuItems["New Profile…"].click()
        XCTAssertTrue(app.textFields["profile-name"].waitForExistence(timeout: 10)); paste("Separate", into: app.textFields["profile-name"])
        app.buttons["profile-save"].click()
        let other = app.windows["browser-window-3"]
        XCTAssertTrue(other.waitForExistence(timeout: 15)); waitValue(other.textFields["address"], "about:credits")
        positionTabDragWindow(other, x: 40)
        second.buttons["tab-1"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.6,
            thenDragTo: other.buttons["tab-3"].coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5)))
        XCTAssertTrue(second.buttons["tab-1"].exists); XCTAssertFalse(other.buttons["tab-1"].exists)
        waitValue(other.textFields["address"], "about:credits")
        waitValue(moved, "After source closes 中文"); waitValue(second.buttons["page-zoom"], "150%")
        XCTAssertEqual(fixture.requests, ["/editing"], "Transfer and refused drops never fetch another document")
        let attachment = XCTAttachment(screenshot: second.screenshot())
        attachment.name = "macos-tab-drag-windows"; attachment.lifetime = .keepAlways; add(attachment)
    }

    func testTraditionalChineseChromeSettingsMenusAndMultipleWindows() throws {
        let downloads = assistantSettingsFile.deletingLastPathComponent().appendingPathComponent("localization-downloads")
        app.launchArguments += ["--downloads-directory", downloads.appendingPathComponent("files").path,
                                "--downloads-data-directory", downloads.appendingPathComponent("data").path]
        let languageIndex = try XCTUnwrap(app.launchArguments.firstIndex(of: "--interface-language"))
        app.launchArguments[languageIndex + 1] = "zh-Hant"
        launch()
        let first = app.windows["browser-window"]
        XCTAssertEqual(first.buttons["back"].label, "返回")
        XCTAssertEqual(first.buttons["reload"].label, "重新載入")
        XCTAssertEqual(first.textFields["address"].label, "網址")
        XCTAssertEqual(first.buttons["add-tab"].label, "新增分頁")
        first.buttons["downloads"].click()
        XCTAssertTrue(app.staticTexts["downloads-title"].waitForExistence(timeout: 10))
        waitAssistantText(app.staticTexts["downloads-title"], "下載項目")
        XCTAssertEqual(app.buttons["downloads-close"].label, "完成")
        app.buttons["downloads-close"].click()
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15))
        XCTAssertEqual(second.buttons["back"].label, "返回")
        XCTAssertEqual(second.buttons["assistant"].label, "本機助理")
        XCTAssertTrue(app.menuBars.menuBarItems["設定檔"].exists)
        app.typeKey(",", modifierFlags: .command)
        XCTAssertTrue(app.popUpButtons["interface-language"].waitForExistence(timeout: 10))
        XCTAssertEqual(app.popUpButtons["appearance-choice"].label, "外觀")
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "macos-traditional-chinese-interface"; attachment.lifetime = .keepAlways; add(attachment)
    }

    func testRuntimeLanguageSwitchPreservesPageEditorsPrivatePanelsAndRelaunch() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/editing")
        let first = app.windows["browser-window"], editor = first.groups["page"].textFields["Editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 15)); paste("Keep 😀 中文", into: editor)
        waitValue(editor, "Keep 😀 中文")
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.staticTexts["permissions-empty"].waitForExistence(timeout: 10))
        app.activate(); app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15))
        app.typeKey(",", modifierFlags: .command)
        let language = app.popUpButtons["interface-language"]
        XCTAssertTrue(language.waitForExistence(timeout: 10)); language.click()
        app.menuItems["繁體中文"].click()
        let localized = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "返回"), object: first.buttons["back"])
        XCTAssertEqual(XCTWaiter.wait(for: [localized], timeout: 10), .completed)
        XCTAssertEqual(second.buttons["back"].label, "返回")
        waitValue(editor, "Keep 😀 中文")
        waitValue(first.textFields["address"], fixture.origin + "/editing")
        waitAssistantText(panels.staticTexts["permissions-empty"], "尚未安裝擴充功能。")
        XCTAssertEqual(panels.buttons["permissions-refresh"].label, "重新整理")
        panels.activate(); panels.radioButtons["助理設定"].click()
        waitAssistantText(panels.staticTexts["assistant-settings-current"], "目前設定：關閉 · 閒置 600 秒 · nice 值 10")
        XCTAssertEqual(panels.buttons["assistant-settings-review"].label, "檢閱變更…")
        XCTAssertFalse(panels.buttons["assistant-settings-confirm"].exists)
        XCTAssertFalse(FileManager.default.fileExists(atPath: assistantSettingsFile.path))
        app.activate(); app.typeKey(",", modifierFlags: .command)
        language.click(); app.menuItems["English"].click()
        let english = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == 'Back'"), object: first.buttons["back"])
        XCTAssertEqual(XCTWaiter.wait(for: [english], timeout: 10), .completed)
        waitAssistantText(panels.staticTexts["assistant-settings-current"], "In force: Off · idle 600 seconds · niceness 10")
        waitValue(editor, "Keep 😀 中文")
        language.click(); app.menuItems["繁體中文"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "返回"), object: first.buttons["back"])], timeout: 10), .completed)
        let settings = app.windows.containing(.popUpButton, identifier: "interface-language").firstMatch
        settings.buttons[XCUIIdentifierCloseWindow].click()
        second.buttons[XCUIIdentifierCloseWindow].click()
        first.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 15))
        let languageIndex = try XCTUnwrap(app.launchArguments.firstIndex(of: "--interface-language"))
        app.launchArguments.removeSubrange(languageIndex...languageIndex + 1)
        launch(); XCTAssertEqual(app.buttons["back"].label, "返回")
        XCTAssertEqual(fixture.requests, ["/editing"])
        // Foundation acknowledges the shared defaults database before its
        // backing plist is updated. Check the actual owner file asynchronously,
        // as for the session archive, without relaxing the persisted value.
        let persisted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            self.storedPreferences()?["browser.interfaceLanguage"] as? String == "zh-Hant"
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [persisted], timeout: 15), .completed)
        XCTAssertEqual(storedPreferences()?["browser.interfaceLanguage"] as? String, "zh-Hant")
    }

    override func setUpWithError() throws {
        continueAfterFailure = false
        credentialFixtureAccounts = []
        app = XCUIApplication()
        preferenceDomain = "cc.blueice.uitests." + UUID().uuidString
        app.launchArguments = ["--preferences-domain", preferenceDomain, "--interface-language", "en"]
        let assistantRoot = FileManager.default.temporaryDirectory.appendingPathComponent("as-" + UUID().uuidString.prefix(12))
        try FileManager.default.createDirectory(at: assistantRoot, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        assistantSettingsFile = assistantRoot.appendingPathComponent("settings.json")
        app.launchArguments += ["--assistant-settings", assistantSettingsFile.path]
        assistantControlSocket = assistantRoot.appendingPathComponent("c.sock")
        XCTAssertLessThan(assistantControlSocket.path.utf8.count, 104, "Darwin Unix sockets require short paths")
        app.launchArguments += ["--control-socket", assistantControlSocket.path]
        originalInputSource = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
        let filter = [kTISPropertyInputSourceID as String: "com.apple.keylayout.ABC"] as CFDictionary
        let sources = TISCreateInputSourceList(filter, false).takeRetainedValue() as! [TISInputSource]
        if let source = sources.first { XCTAssertEqual(TISSelectInputSource(source), noErr) }
    }

    override func tearDownWithError() throws {
        defer {
            for username in credentialFixtureAccounts {
                guard username.hasPrefix("bi-ui-"), UUID(uuidString: String(username.dropFirst(6))) != nil else { XCTFail("Invalid fixture account"); continue }
                for service in ["sftp","sftp-key-passphrase","ftps"] {
                    let context = LAContext(); context.interactionNotAllowed = true
                    let query: [CFString: Any] = [kSecClass: kSecClassGenericPassword,
                        kSecAttrService: "org.blueice.downloads.\(service).credential-fixture.invalid:2222",
                        kSecAttrAccount: username,kSecUseAuthenticationContext: context]
                    let status = SecItemDelete(query as CFDictionary)
                    XCTAssertTrue(status == errSecSuccess || status == errSecItemNotFound,"Owned credential fixture cleanup failed: \(status)")
                }
            }
            try? FileManager.default.removeItem(at: assistantSettingsFile.deletingLastPathComponent())
            try? FileManager.default.removeItem(at: assistantControlSocket)
            UserDefaults(suiteName: preferenceDomain)?.removePersistentDomain(forName: preferenceDomain)
            if app.state == .notRunning, let file = ownedPreferenceFile { try? FileManager.default.removeItem(at: file) }
            if let originalInputSource { XCTAssertEqual(TISSelectInputSource(originalInputSource), noErr) }
        }
        if app.state != .notRunning {
            if app.sheets["open-panel"].buttons["Cancel"].exists { app.sheets["open-panel"].buttons["Cancel"].click() }
            if app.sheets["GoToWindow"].exists { app.typeKey(.escape,modifierFlags: []) }
            if app.sheets["save-panel"].buttons["CancelButton"].exists { app.sheets["save-panel"].buttons["CancelButton"].click() }
            if app.sheets["alert"].buttons["OK"].exists { app.sheets["alert"].buttons["OK"].click() }
            if printPanel.menus.firstMatch.exists { app.typeKey(.escape,modifierFlags: []) }
            if printPanel.buttons["Cancel"].exists { printPanel.buttons["Cancel"].click() }
            if app.buttons["download-credentials-close"].exists { app.buttons["download-credentials-close"].click() }
            if app.buttons["downloads-close"].exists { app.buttons["downloads-close"].click() }
            let deadline = Date().addingTimeInterval(15)
            while app.state != .notRunning, Date() < deadline {
                let candidate = app.windows.matching(NSPredicate(format: "identifier BEGINSWITH 'browser-window'")).firstMatch
                guard candidate.exists else { break }
                let window = app.windows[candidate.identifier]
                window.buttons[XCUIIdentifierCloseWindow].click()
                _ = XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: window)], timeout: 5)
            }
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

    private func enter(_ text: String, submit: Bool = true, in window: XCUIElement? = nil) {
        let address = (window ?? app).textFields["address"]
        address.click()
        app.menuBars.menuBarItems["Edit"].click()
        app.menuItems["Input Source"].hover()
        app.menuItems["ABC"].click()
        address.click()
        address.typeKey("a", modifierFlags: .command)
        address.typeText(text)
        if submit { address.typeKey(.return, modifierFlags: []) }
    }

    private func paste(_ text: String, into field: XCUIElement) {
        field.click(); field.typeKey("a", modifierFlags: .command)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
        field.typeKey("v", modifierFlags: .command)
        waitValue(field, text)
    }
    private func chooseChromeContext(_ title: String) {
        let item = app.windows["browser-window"].menuItems[title]
        XCTAssertTrue(item.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(item.isEnabled, app.debugDescription); item.click()
    }
    private func moveTab(_ id: Int, to group: String) {
        app.buttons["tab-\(id)"].rightClick()
        XCTAssertTrue(app.windows["browser-window"].menuItems["Move to Group"].waitForExistence(timeout: 10), app.debugDescription)
        app.windows["browser-window"].menuItems["Move to Group"].hover(); chooseChromeContext(group)
    }

    private func activateWindow(_ id: Int) {
        app.menuBars.menuBarItems["Window"].click()
        let item = app.menuBars.menuItems["Window \(id)"]
        XCTAssertTrue(item.waitForExistence(timeout: 10)); item.click()
    }
    private func chooseProfileAction(_ title: String, profile: String) {
        app.menuBars.menuBarItems["Profiles"].click()
        let submenu = app.menuBars.menuItems[profile]
        XCTAssertTrue(submenu.waitForExistence(timeout: 10)); submenu.hover()
        let item = submenu.menuItems[title]
        XCTAssertTrue(item.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(item.isEnabled, app.debugDescription); item.click()
    }
    private func transferTab(_ tab: Int, from window: XCUIElement, to destination: String) {
        window.buttons["tab-\(tab)"].rightClick()
        let menu = window.menuItems["Move to Window"]
        XCTAssertTrue(menu.waitForExistence(timeout: 10)); menu.hover()
        let item = window.menuItems[destination]
        XCTAssertTrue(item.waitForExistence(timeout: 10)); XCTAssertTrue(item.isEnabled); item.click()
    }

    func testNativeWindowsTransferEditedGroupedTabAndCloseOriginalWindow() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch()
        let first = app.windows["browser-window"]
        enter(fixture.origin + "/context-menu", in: first)
        let original = first.groups["page"].textFields["Context editor"]
        XCTAssertTrue(original.waitForExistence(timeout: 15)); paste("Window 中文", into: original)
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Page Zoom"].hover(); app.menuItems["150%"].click()
        waitValue(first.buttons["page-zoom"], "150%")
        first.buttons["new-tab-group"].click()
        XCTAssertTrue(first.textFields["group-name"].waitForExistence(timeout: 10))
        paste("Shared", into: first.textFields["group-name"]); first.buttons["group-save"].click()
        XCTAssertTrue(first.buttons["tab-group-1"].waitForExistence(timeout: 10))
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15)); waitValue(second.textFields["address"], "about:credits")
        waitValue(second.buttons["page-zoom"], "100%")
        app.typeKey("=", modifierFlags: .command); waitValue(second.buttons["page-zoom"], "110%")
        waitValue(first.buttons["page-zoom"], "150%")
        activateWindow(1)
        transferTab(1, from: first, to: "Window 2")
        waitValue(second.textFields["address"], fixture.origin + "/context-menu")
        let moved = second.groups["page"].textFields["Context editor"]
        XCTAssertTrue(moved.waitForExistence(timeout: 15)); waitValue(moved, "Window 中文")
        waitValue(second.buttons["page-zoom"], "150%")
        XCTAssertEqual(moved.frame.width, 420, accuracy: 3)
        XCTAssertTrue(second.buttons["tab-group-1"].exists); XCTAssertTrue(second.buttons["back"].isEnabled)
        XCTAssertFalse(first.buttons["tab-1"].exists); XCTAssertFalse(first.textFields["address"].isEnabled)
        activateWindow(1); first.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: first)], timeout: 10), .completed)
        XCTAssertNotEqual(app.state, .notRunning)
        activateWindow(2); paste("After close 中文", into: moved)
        app.typeKey("0", modifierFlags: .command); waitValue(second.buttons["page-zoom"], "100%")
        transferTab(1, from: second, to: "New Window")
        let third = app.windows["browser-window-3"]
        XCTAssertTrue(third.waitForExistence(timeout: 15)); waitValue(third.textFields["address"], fixture.origin + "/context-menu")
        waitValue(third.groups["page"].textFields["Context editor"], "After close 中文")
        XCTAssertTrue(third.buttons["tab-group-1"].exists); XCTAssertFalse(third.buttons["tab-3"].exists)
        XCTAssertTrue(second.buttons["tab-2"].exists); XCTAssertFalse(second.buttons["tab-1"].exists)
        XCTAssertEqual(fixture.requests, ["/context-menu"], "Moving the existing page must not fetch it again")
        let attachment = XCTAttachment(screenshot: third.screenshot())
        attachment.name = "macos-shared-core-windows"; attachment.lifetime = .keepAlways; add(attachment)
        activateWindow(2); second.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: second)], timeout: 10), .completed)
        third.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 10), "The last native window must stop the owned services")
    }

    func testResizingSecondWindowPreservesFirstViewportAndAddressDraft() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); let first = app.windows["browser-window"]
        enter(fixture.origin + "/context-menu", in: first)
        let page = first.groups["page"]
        XCTAssertTrue(first.groups["page"].textFields["Context editor"].waitForExistence(timeout: 15))
        let before = page.value as? String, width = page.frame.width
        enter("about:settings", submit: false, in: first)
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15)); waitValue(second.textFields["address"], "about:credits")
        let otherPage = second.groups["page"]
        let rendered = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value BEGINSWITH 'Rendered'"), object: otherPage)
        XCTAssertEqual(XCTWaiter.wait(for: [rendered], timeout: 15), .completed)
        let oldWidth = otherPage.frame.width, oldPixels = otherPage.value as? String
        let corner = second.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1)).withOffset(CGVector(dx: -2, dy: -2))
        corner.click(forDuration: 0.2, thenDragTo: corner.withOffset(CGVector(dx: -120, dy: -100)))
        let changed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            otherPage.frame.width < oldWidth - 60 && (otherPage.value as? String) != oldPixels
        }, object: otherPage)
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 15), .completed)
        XCTAssertEqual(page.frame.width, width, accuracy: 1); XCTAssertEqual(page.value as? String, before)
        waitValue(first.textFields["address"], "about:settings")
        activateWindow(1); first.buttons["reload"].click()
        waitValue(first.textFields["address"], fixture.origin + "/context-menu")
        XCTAssertEqual(fixture.requests, ["/context-menu", "/context-menu"])
    }

    func testWindowTransferKeepsFindQueryAndActiveWindowCommands() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch()
        let first = app.windows["browser-window"]
        enter(fixture.origin + "/find", in: first)
        XCTAssertTrue(first.groups["page"].descendants(matching: .any).matching(NSPredicate(format: "label == 'Find fixture' OR value == 'Find fixture'")).firstMatch.waitForExistence(timeout: 15))
        app.typeKey("f", modifierFlags: .command)
        let query = first.searchFields["find-query"]
        XCTAssertTrue(query.waitForExistence(timeout: 10)); query.typeText("frost"); waitFind("1 of 4")
        transferTab(1, from: first, to: "New Window")
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15)); waitValue(second.textFields["address"], fixture.origin + "/find")
        let transferredQuery = second.searchFields["find-query"]
        XCTAssertTrue(transferredQuery.waitForExistence(timeout: 10)); waitValue(transferredQuery, "frost")
        let summary = second.staticTexts["find-results"]
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == '1 of 4' OR value == '1 of 4'"), object: summary)], timeout: 15), .completed)
        app.typeKey("g", modifierFlags: .command)
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == '2 of 4' OR value == '2 of 4'"), object: summary)], timeout: 15), .completed)
        XCTAssertFalse(first.searchFields["find-query"].exists)
        XCTAssertTrue(second.buttons["back"].isEnabled)
        second.buttons["find-close"].click(); XCTAssertFalse(transferredQuery.exists)
        app.typeKey("f", modifierFlags: .command)
        XCTAssertTrue(transferredQuery.waitForExistence(timeout: 10)); waitValue(transferredQuery, "frost")
        XCTAssertEqual(fixture.requests, ["/find"])
    }

    func testTabGroupsNativeEditorCollapseMoveRemovePreservePagesAndZoom() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard()
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/context-menu")
        let field = app.groups["page"].textFields["Context editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 15)); paste("Grouped 中文", into: field)
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Page Zoom"].hover(); app.menuItems["150%"].click()
        waitValue(app.buttons["page-zoom"], "150%")
        app.buttons["new-tab-group"].click()
        XCTAssertTrue(app.textFields["group-name"].waitForExistence(timeout: 10))
        paste("  Research  ", into: app.textFields["group-name"])
        app.buttons["group-color-red"].click(); waitValue(app.textFields["group-color"], "#cc3344")
        app.buttons["group-save"].click()
        let group = app.buttons["tab-group-1"]
        XCTAssertTrue(group.waitForExistence(timeout: 10))
        XCTAssertEqual(group.label, "Research"); waitValue(group, "Expanded, 1 tabs, contains selected tab")
        XCTAssertGreaterThan(try colorCount(try XCTUnwrap(group.screenshot().image.tiffRepresentation), red: 204/255, green: 51/255, blue: 68/255), 5)
        waitValue(field, "Grouped 中文"); waitValue(app.buttons["page-zoom"], "150%")
        XCTAssertTrue(app.buttons["back"].isEnabled)
        group.click(); waitValue(group, "Collapsed, 1 tabs, contains selected tab")
        XCTAssertFalse(app.buttons["tab-1"].exists, "Collapsed members are hidden while their selected page remains live")
        waitValue(field, "Grouped 中文")
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        waitValue(app.buttons["page-zoom"], "100%")
        XCTAssertTrue(app.buttons["tab-2"].waitForExistence(timeout: 10))
        moveTab(2, to: "Research"); waitValue(group, "Collapsed, 2 tabs, contains selected tab")
        XCTAssertFalse(app.buttons["tab-2"].exists)
        waitValue(app.textFields["address"], "about:credits")
        group.click(); waitValue(group, "Expanded, 2 tabs, contains selected tab")
        app.buttons["tab-1"].click(); waitValue(field, "Grouped 中文"); waitValue(app.buttons["page-zoom"], "150%")
        group.rightClick(); chooseChromeContext("Edit Group…")
        XCTAssertTrue(app.textFields["group-name"].waitForExistence(timeout: 10))
        paste("Work", into: app.textFields["group-name"])
        paste("#228844", into: app.textFields["group-color"])
        app.buttons["group-save"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == 'Work'"), object: group)], timeout: 10), .completed)
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: group)], timeout: 10), .completed)
        XCTAssertGreaterThan(try colorCount(try XCTUnwrap(group.screenshot().image.tiffRepresentation), red: 34/255, green: 136/255, blue: 68/255), 5)
        moveTab(2, to: "No Group"); waitValue(group, "Expanded, 1 tabs, contains selected tab")
        moveTab(2, to: "Work"); waitValue(group, "Expanded, 2 tabs, contains selected tab")
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        attachment.name = "macos-tab-groups"; attachment.lifetime = .keepAlways; add(attachment)
        group.rightClick(); chooseChromeContext("Ungroup and Remove Group")
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: group)], timeout: 10), .completed)
        XCTAssertTrue(app.buttons["tab-1"].exists); XCTAssertTrue(app.buttons["tab-2"].exists)
        waitValue(field, "Grouped 中文"); waitValue(app.buttons["page-zoom"], "150%")
        XCTAssertEqual(fixture.requests, ["/context-menu"], "Tab organization never fetches the page again")
        app.buttons["back"].click(); waitValue(app.textFields["address"], "about:credits")
        app.buttons["forward"].click(); waitValue(app.textFields["address"], fixture.origin + "/context-menu")
        waitValue(field, "hello")
        XCTAssertEqual(fixture.requests, ["/context-menu", "/context-menu"])
    }

    func testTabGroupKeyboardValidationCancelAndEmptyGroupLifecycle() throws {
        let saved = saveClipboard()
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); app.typeKey("g", modifierFlags: [.command, .option])
        let name = app.textFields["group-name"], save = app.buttons["group-save"]
        XCTAssertTrue(name.waitForExistence(timeout: 10)); XCTAssertFalse(save.isEnabled)
        paste(String(repeating: "e\u{301}", count: 41), into: name)
        XCTAssertFalse(save.isEnabled, "Core names are limited to 80 Unicode scalars, including combining marks")
        paste("Valid", into: name); paste("red", into: app.textFields["group-color"])
        XCTAssertFalse(save.isEnabled)
        paste("#4477CC", into: app.textFields["group-color"]); XCTAssertTrue(save.isEnabled)
        app.buttons["group-cancel"].click(); XCTAssertFalse(app.buttons["tab-group-1"].exists)
        app.buttons["tab-1"].rightClick(); chooseChromeContext("New Tab Group…")
        XCTAssertTrue(name.waitForExistence(timeout: 10)); paste("Keep", into: name); save.click()
        let group = app.buttons["tab-group-1"]
        XCTAssertTrue(group.waitForExistence(timeout: 10)); waitValue(group, "Expanded, 1 tabs, contains selected tab")
        app.buttons["close-tab-1"].click(); waitValue(group, "Expanded, 0 tabs")
        XCTAssertTrue(app.staticTexts["empty-page"].exists)
        group.rightClick(); chooseChromeContext("Edit Group…")
        XCTAssertTrue(name.waitForExistence(timeout: 10)); paste("Empty", into: name); save.click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == 'Empty'"), object: group)], timeout: 10), .completed)
        app.menuBars.menuBarItems["View"].click(); app.menuBars.menuItems["Tab Groups"].hover(); app.menuBars.menuItems["New Tab Group…"].click()
        XCTAssertTrue(name.waitForExistence(timeout: 10)); paste("Second empty", into: name); save.click()
        let second = app.buttons["tab-group-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 10)); waitValue(second, "Expanded, 0 tabs")
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        XCTAssertTrue(app.buttons["tab-2"].exists)
        waitValue(group, "Expanded, 0 tabs"); waitValue(second, "Expanded, 0 tabs")
        group.rightClick(); chooseChromeContext("Ungroup and Remove Group")
        second.rightClick(); chooseChromeContext("Ungroup and Remove Group")
        XCTAssertFalse(group.exists); XCTAssertFalse(second.exists)
        XCTAssertTrue(app.buttons["tab-2"].exists)
    }

    func testTabGroupEditorRejectsGroupRemovedThroughNativeMenu() throws {
        let saved = saveClipboard()
        defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); app.buttons["new-tab-group"].click()
        let name = app.textFields["group-name"]
        XCTAssertTrue(name.waitForExistence(timeout: 10)); paste("Stale", into: name)
        app.buttons["group-save"].click()
        let group = app.buttons["tab-group-1"]
        XCTAssertTrue(group.waitForExistence(timeout: 10))
        group.rightClick(); chooseChromeContext("Edit Group…")
        XCTAssertTrue(name.waitForExistence(timeout: 10))
        app.menuBars.menuBarItems["View"].click(); app.menuBars.menuItems["Tab Groups"].hover()
        app.menuBars.menuItems["Stale"].hover(); app.menuBars.menuItems["Ungroup and Remove Group"].click()
        XCTAssertTrue(app.staticTexts["group-target-closed"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["group-save"].isEnabled)
        XCTAssertEqual(name.value as? String, "Stale")
        app.buttons["group-cancel"].click()
        XCTAssertFalse(group.exists); XCTAssertTrue(app.buttons["tab-1"].exists)
        waitValue(app.textFields["address"], "about:credits")
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

    private func preferenceWindow() -> XCUIElement {
        app.typeKey(",", modifierFlags: .command)
        let window = app.windows.containing(.popUpButton, identifier: "appearance-choice").firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 10), app.debugDescription)
        return window
    }
    private func configureSearch(_ endpoint: String, parameter: String = "q") {
        let window = preferenceWindow()
        choosePreference(window,"search-provider","Custom")
        paste(endpoint,into: window.textFields["search-endpoint"])
        paste(parameter,into: window.textFields["search-parameter"])
        window.buttons["search-save"].click()
        XCTAssertFalse(window.staticTexts["search-settings-error"].exists,window.debugDescription)
        window.buttons[XCUIIdentifierCloseWindow].click()
        app.windows["browser-window"].click()
    }
    func testAddressSearchUsesExactUnicodeQueryOnlyOnHumanSubmitAndRetainsHistory() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); configureSearch(fixture.origin + "/search?lang=zh&q=old&q=older")
        let query = "C++ & 東京 👨‍👩‍👧 #100%"
        paste(query,into: app.textFields["address"])
        XCTAssertEqual(fixture.requests,[],"Typing must not contact the provider")
        app.textFields["address"].typeKey(.return,modifierFlags: [])
        waitPageContent("Search results"); waitPageContent(query)
        let path = try XCTUnwrap(fixture.requests.first)
        XCTAssertEqual(fixture.requests.count,1)
        XCTAssertTrue(path.hasPrefix("/search?lang=zh&q=C%2B%2B%20%26%20"))
        XCTAssertEqual(URLComponents(string: fixture.origin + path)?.queryItems,[URLQueryItem(name: "lang",value: "zh"),URLQueryItem(name: "q",value: query)])
        waitValue(app.textFields["address"],fixture.origin + path)
        let image = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        image.name = "macos-native-address-search"; image.lifetime = .keepAlways; add(image)
        configureSearch(fixture.origin + "/changed-provider")
        XCTAssertEqual(fixture.requests,[path],"Changing the provider must not navigate the current page")
        app.buttons["back"].click(); waitValue(app.textFields["address"],"about:credits")
        app.buttons["forward"].click(); waitValue(app.textFields["address"],fixture.origin + path)
        waitPageContent(query)
        let requests = fixture.requests
        XCTAssertTrue(requests == [path] || requests == [path,path],"History retains the confirmed URL after a provider change, whether the response is cached or fetched again")
    }
    func testSearchSettingsPersistAndNewWindowsUseTheChosenProvider() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); configureSearch(fixture.origin + "/search",parameter: "term")
        app.typeKey("n",modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 10))
        paste("?example.test",into: second.textFields["address"])
        XCTAssertEqual(fixture.requests,[])
        second.buttons["go"].click(); waitValue(second.textFields["address"],fixture.origin + "/search?term=example.test")
        XCTAssertEqual(fixture.requests,["/search?term=example.test"])
        app.typeKey("q",modifierFlags: .command); XCTAssertTrue(app.wait(for: .notRunning,timeout: 15))
        launch()
        let settings = preferenceWindow()
        XCTAssertEqual(settings.popUpButtons["search-provider"].value as? String,"Custom")
        waitValue(settings.textFields["search-endpoint"],fixture.origin + "/search")
        waitValue(settings.textFields["search-parameter"],"term")
        settings.buttons[XCUIIdentifierCloseWindow].click()
        paste("blueice browser",into: app.textFields["address"])
        app.buttons["go"].click(); waitPageContent("blueice browser")
        XCTAssertEqual(fixture.requests,["/search?term=example.test","/search?term=blueice%20browser"])
    }
    func testUnknownSearchSettingsRefuseSearchUntilAnExplicitHumanConfiguration() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        // Seed the application's argument preference domain; the generated
        // sandboxed runner has a separate UserDefaults suite of the same name.
        app.launchArguments += ["-browser.search.configuration","future-provider"]
        launch()
        let unknown = preferenceWindow()
        XCTAssertEqual(unknown.popUpButtons["search-provider"].value as? String,"Choose a search engine")
        unknown.buttons[XCUIIdentifierCloseWindow].click()
        enter(fixture.origin + "/second"); waitPageContent("BlueIce external page")
        let back = app.buttons["back"].isEnabled, forward = app.buttons["forward"].isEnabled
        paste("private query",into: app.textFields["address"])
        app.buttons["go"].click()
        waitValue(app.staticTexts["status"],"Choose a search engine in Settings before searching.")
        XCTAssertEqual(app.buttons["back"].isEnabled,back); XCTAssertEqual(app.buttons["forward"].isEnabled,forward)
        waitPageContent("BlueIce external page"); XCTAssertEqual(fixture.requests,["/second"])
        configureSearch(fixture.origin + "/search")
        paste("private query",into: app.textFields["address"]); app.buttons["go"].click()
        waitPageContent("private query"); XCTAssertEqual(fixture.requests,["/second","/search?q=private%20query"])
    }
    func testSearchNavigationStillUsesTheRealGatekeeperDenial() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/second"); waitPageContent("BlueIce external page")
        configureSearch("http://malware.test/search")
        paste("blocked query",into: app.textFields["address"]); app.buttons["go"].click()
        let denied = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS[c] 'Navigation blocked' OR label CONTAINS[c] 'Navigation blocked'"),object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [denied],timeout: 15),.completed)
        waitPageContent("BlueIce external page")
        // Exercise the confirmed history rather than inferring it from one
        // enabled control. A denied query must not add a history entry.
        app.buttons["back"].click(); waitValue(app.textFields["address"],"about:credits")
        app.buttons["forward"].click(); waitValue(app.textFields["address"],fixture.origin + "/second")
        waitPageContent("BlueIce external page")
        XCTAssertTrue(fixture.requests == ["/second"] || fixture.requests == ["/second","/second"])
    }
    func testSearchSettingsLocalizeAndKeyboardSaveRejectsInvalidDrafts() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        let languageIndex = try XCTUnwrap(app.launchArguments.firstIndex(of: "--interface-language"))
        app.launchArguments[languageIndex + 1] = "zh-Hant"
        launch()
        let settings = preferenceWindow()
        XCTAssertEqual(settings.popUpButtons["search-provider"].label,"搜尋引擎")
        for provider in ["Google","Bing","DuckDuckGo"] {
            choosePreference(settings,"search-provider",provider)
            XCTAssertEqual(settings.popUpButtons["search-provider"].value as? String,provider)
        }
        choosePreference(settings,"search-provider","自訂")
        paste("https://alice:secret@example.test/search",into: settings.textFields["search-endpoint"])
        paste("q",into: settings.textFields["search-parameter"])
        settings.textFields["search-parameter"].typeKey(.return,modifierFlags: [])
        XCTAssertTrue(settings.staticTexts["search-settings-error"].waitForExistence(timeout: 10))
        // Inspect durable user-visible state in a fresh process. A runner's
        // cross-process UserDefaults cache is not an acceptance boundary.
        settings.buttons[XCUIIdentifierCloseWindow].click()
        app.typeKey("q",modifierFlags: .command); XCTAssertTrue(app.wait(for: .notRunning,timeout: 15))
        launch()
        let restored = preferenceWindow()
        XCTAssertEqual(restored.popUpButtons["search-provider"].value as? String,"自訂")
        waitValue(restored.textFields["search-endpoint"],"")
        paste(fixture.origin + "/search",into: restored.textFields["search-endpoint"])
        restored.buttons["search-save"].click()
        XCTAssertFalse(restored.staticTexts["search-settings-error"].exists)
        XCTAssertEqual(restored.buttons["search-save"].label,"儲存自訂搜尋")
        XCTAssertTrue(restored.buttons["search-save"].isHittable)
        XCTAssertTrue(restored.checkBoxes["session-remember"].isHittable,"Search must keep the native settings window usable")
        let image = XCTAttachment(screenshot: restored.screenshot())
        image.name = "macos-native-search-settings-zh"; image.lifetime = .keepAlways; add(image)
        restored.buttons[XCUIIdentifierCloseWindow].click()
        app.typeKey("l",modifierFlags: .command)
        app.typeText("blueice browser"); app.typeKey(.return,modifierFlags: [])
        waitPageContent("Search results")
        XCTAssertEqual(fixture.requests,["/search?q=blueice%20browser"])
    }
    private func choosePreference(_ window: XCUIElement, _ identifier: String, _ title: String) {
        window.popUpButtons[identifier].click(); app.menuItems[title].click()
    }
    private func pageContent(_ name: String) -> XCUIElement {
        app.groups["page"].descendants(matching: .any).matching(NSPredicate(format: "label == %@ OR value == %@", name, name)).firstMatch
    }
    private func waitPageContent(_ name: String, exists: Bool = true) {
        let element = pageContent(name)
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == %@", NSNumber(value: exists)), object: element)], timeout: 15), .completed, name)
    }
    private func screenshotPixels(_ data: Data, area: CGRect? = nil) throws -> (Data, Int, Int) {
        var image = try XCTUnwrap(NSBitmapImageRep(data: data)?.cgImage)
        if let area {
            let rect = CGRect(x: area.minX * Double(image.width),y: area.minY * Double(image.height),
                              width: area.width * Double(image.width),height: area.height * Double(image.height)).integral
            image = try XCTUnwrap(image.cropping(to: rect))
        }
        let space = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        let context = try XCTUnwrap(CGContext(data: nil, width: image.width, height: image.height,
            bitsPerComponent: 8, bytesPerRow: image.width * 4, space: space,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        // Convert the screenshot's embedded monitor profile as a whole. colorAt()
        // produces Generic RGB colors and loses that profile on this macOS host.
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return (Data(bytes: try XCTUnwrap(context.data), count: image.width * image.height * 4), image.width, image.height)
    }
    private func colorCount(_ data: Data, red: Double, green: Double, blue: Double, area: CGRect? = nil) throws -> Int {
        let (pixels, width, height) = try screenshotPixels(data,area: area); var count = 0
        for y in stride(from: 0, to: height, by: 3) {
            for x in stride(from: 0, to: width, by: 3) {
                let offset = (y * width + x) * 4
                if abs(Double(pixels[offset]) / 255 - red) < 0.02,
                   abs(Double(pixels[offset + 1]) / 255 - green) < 0.02,
                   abs(Double(pixels[offset + 2]) / 255 - blue) < 0.02 { count += 1 }
            }
        }
        return count
    }
    private func brightCount(_ data: Data) throws -> Int {
        let (pixels, _, _) = try screenshotPixels(data)
        return stride(from: 0, to: pixels.count, by: 4).filter { offset in
            pixels[offset] > 191 && pixels[offset + 1] > 191 && pixels[offset + 2] > 191
        }.count
    }
    func testAppearanceSettingsPreservePageAndPersistAcrossNativeRelaunch() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/appearance")
        let editor = app.groups["page"].textFields["Appearance editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 15))
        editor.click(); editor.typeKey("a", modifierFlags: .command); editor.typeText("retained text")
        waitValue(editor, "retained text")
        let settings = preferenceWindow()
        choosePreference(settings, "appearance-choice", "Light")
        choosePreference(settings, "contrast-choice", "Standard")
        choosePreference(settings, "motion-choice", "No reduction")
        settings.buttons[XCUIIdentifierCloseWindow].click()
        waitPageContent("Light content"); waitPageContent("Standard contrast"); waitPageContent("Motion allowed")
        let lightShot = app.groups["page"].screenshot()
        let lightAttachment = XCTAttachment(screenshot: lightShot)
        lightAttachment.name = "macos-display-light-page"; lightAttachment.lifetime = .keepAlways; add(lightAttachment)
        let light = try XCTUnwrap(lightShot.image.tiffRepresentation)
        let pageFrame = app.groups["page"].frame
        // Sample the empty CSS surface immediately below the editor. Whole-page
        // near-color searches can match unrelated text/control antialiasing.
        let surface = CGRect(x: (editor.frame.minX - pageFrame.minX + 10) / pageFrame.width,
                             y: (editor.frame.maxY - pageFrame.minY + 5) / pageFrame.height,
                             width: 100 / pageFrame.width,height: 10 / pageFrame.height)
        XCTAssertGreaterThan(try colorCount(light, red: 170/255, green: 187/255, blue: 204/255,area: surface), 100)
        let panel = preferenceWindow()
        choosePreference(panel, "appearance-choice", "Dark")
        choosePreference(panel, "motion-choice", "Reduce")
        panel.buttons[XCUIIdentifierCloseWindow].click()
        waitPageContent("Dark content"); waitPageContent("Reduced motion")
        let tabs = app.descendants(matching: .any)["tab-strip"]
        let standard = try brightCount(try XCTUnwrap(tabs.screenshot().image.tiffRepresentation))
        let contrast = preferenceWindow()
        choosePreference(contrast, "contrast-choice", "Increased")
        contrast.buttons[XCUIIdentifierCloseWindow].click()
        waitPageContent("Dark content"); waitPageContent("Increased contrast"); waitPageContent("Reduced motion")
        XCTAssertGreaterThan(try brightCount(try XCTUnwrap(tabs.screenshot().image.tiffRepresentation)), standard + 50, "Increased contrast must add a visible selected-tab outline on the same dark chrome")
        waitPageContent("Light content", exists: false); waitPageContent("Motion allowed", exists: false)
        waitValue(editor, "retained text")
        let dark = try XCTUnwrap(app.groups["page"].screenshot().image.tiffRepresentation)
        XCTAssertGreaterThan(try colorCount(dark, red: 16/255, green: 32/255, blue: 48/255,area: surface), 100)
        XCTAssertEqual(try colorCount(dark, red: 170/255, green: 187/255, blue: 204/255,area: surface), 0)
        let screenshot = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        screenshot.name = "macos-display-dark"; screenshot.lifetime = .keepAlways; add(screenshot)
        XCTAssertEqual(fixture.requests, ["/appearance"])
        app.windows["browser-window"].buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 10))
        launch(); enter(fixture.origin + "/appearance")
        waitPageContent("Dark content"); waitPageContent("Increased contrast"); waitPageContent("Reduced motion")
        let restored = preferenceWindow()
        XCTAssertEqual(restored.popUpButtons["appearance-choice"].value as? String, "Dark")
        XCTAssertEqual(restored.popUpButtons["contrast-choice"].value as? String, "Increased")
        XCTAssertEqual(restored.popUpButtons["motion-choice"].value as? String, "Reduce")
        restored.buttons["restore-display-system"].click()
        XCTAssertEqual(restored.popUpButtons["appearance-choice"].value as? String, "System")
        XCTAssertEqual(restored.popUpButtons["contrast-choice"].value as? String, "System")
        XCTAssertEqual(restored.popUpButtons["motion-choice"].value as? String, "System")
        restored.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertEqual(fixture.requests, ["/appearance", "/appearance"])
    }
    func testAppearanceMenuKeepsPreferencesAcrossTabsAndHistory() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/appearance")
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Appearance"].hover(); app.menuItems["Dark Appearance"].click()
        waitPageContent("Dark content"); waitPageContent("Light content", exists: false)
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        enter(fixture.origin + "/appearance"); waitPageContent("Dark content")
        app.menuBars.menuBarItems["View"].click(); app.menuItems["Appearance"].hover(); app.menuItems["Light Appearance"].click()
        waitPageContent("Light content")
        app.buttons["tab-1"].click(); waitPageContent("Light content")
        enter(fixture.origin + "/destination"); app.buttons["back"].click()
        waitValue(app.textFields["address"], fixture.origin + "/appearance"); waitPageContent("Light content")
        XCTAssertEqual(fixture.requests, ["/appearance", "/appearance", "/destination", "/appearance"])
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
        let (pixels, width, height) = try screenshotPixels(try XCTUnwrap(app.groups["page"].screenshot().image.tiffRepresentation))
        var orange = 0
        for y in stride(from: 0, to: height, by: 2) {
            for x in stride(from: 0, to: width, by: 2) {
                let offset = (y * width + x) * 4
                if Double(pixels[offset]) / 255 > 0.85, (0.35...0.75).contains(Double(pixels[offset + 1]) / 255),
                   Double(pixels[offset + 2]) / 255 < 0.3 { orange += 1 }
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
        let (pixels, width, height) = try screenshotPixels(try XCTUnwrap(shot.image.tiffRepresentation))
        var dark = 0
        for y in stride(from: 0, to: height, by: 3) {
            for x in stride(from: 0, to: width, by: 3) {
                let offset = (y * width + x) * 4
                if Double(pixels[offset]) / 255 < 0.7, Double(pixels[offset + 1]) / 255 < 0.7,
                   Double(pixels[offset + 2]) / 255 < 0.7 { dark += 1 }
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
            let count = try? self.colorCount(page.screenshot().image.tiffRepresentation ?? Data(), red: 32/255, green: 120/255, blue: 64/255)
            return (count ?? 0) > 100
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

    func testNativeLiveRegionEditorPrivacyAndDocumentReload() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/accessibility-live")
        let page = app.groups["page"]
        let editor = page.textFields["Status editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 15))
        waitValue(editor, "1"); paste("完成 😀", into: editor); waitValue(editor, "完成 😀")
        let secret = page.secureTextFields["Live secret"]
        XCTAssertTrue(secret.exists)
        XCTAssertFalse(app.debugDescription.contains("private-live-secret"))
        XCTAssertFalse(app.debugDescription.contains("hidden-live-secret"))
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        XCTAssertFalse(page.textFields["Status editor"].exists)
        app.buttons["tab-1"].click(); waitValue(page.textFields["Status editor"], "完成 😀")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-accessibility-live-rotors"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["reload"].click(); waitValue(page.textFields["Status editor"], "1")
        XCTAssertEqual(fixture.requests, ["/accessibility-live", "/accessibility-live"])
    }

    func testNativeDescendantLiveRegionEditorsPrivacyTabIsolationAndReload() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/accessibility-descendants")
        let page = app.groups["page"]
        let atomic = page.textFields["Atomic editor"]
        XCTAssertTrue(atomic.waitForExistence(timeout: 15)); waitValue(atomic, "1")
        paste("完成 😀", into: atomic); waitValue(atomic, "完成 😀")
        let narrow = page.textFields["Narrow editor"]
        paste("narrow 😀", into: narrow); waitValue(narrow, "narrow 😀")
        let suppressed = page.textFields["Suppressed editor"]
        paste("silent", into: suppressed); waitValue(suppressed, "silent")
        XCTAssertTrue(page.secureTextFields["Descendant secret"].exists)
        XCTAssertFalse(app.debugDescription.contains("private-descendant-secret"))
        XCTAssertFalse(app.debugDescription.contains("hidden-descendant-secret"))
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        XCTAssertFalse(page.textFields["Atomic editor"].exists)
        app.buttons["tab-1"].click(); waitValue(atomic, "完成 😀"); waitValue(narrow, "narrow 😀"); waitValue(suppressed, "silent")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-accessibility-descendants"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["reload"].click(); waitValue(atomic, "1"); waitValue(narrow, "1"); waitValue(suppressed, "1")
        XCTAssertEqual(fixture.requests, ["/accessibility-descendants", "/accessibility-descendants"])
    }

    func testDocumentSelectAllCopyLinkDragPrivacyTabIsolationAndReload() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch(); enter(fixture.origin + "/document-selection")
        let page = app.groups["page"], first = page.staticTexts["Alpha bold 😀 é"]
        XCTAssertTrue(first.waitForExistence(timeout: 15))
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("document-copy-sentinel", forType: .string)
        first.click(); app.typeKey("a", modifierFlags: .command); app.typeKey("c", modifierFlags: .command)
        let expected = "Document selection\nAlpha bold 😀 é\nBeta 中文 selectable link\nVisit 1\nBottom finish"
        let copied = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in NSPasteboard.general.string(forType: .string) == expected }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [copied], timeout: 15), .completed)
        guard NSPasteboard.general.string(forType: .string) == expected else { return }
        let link = page.links["selectable link"]
        link.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.5)).press(forDuration: 0.1,
            thenDragTo: first.coordinate(withNormalizedOffset: CGVector(dx: 0.01, dy: 0.5)))
        app.typeKey("c", modifierFlags: .command)
        let dragged = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard let text = NSPasteboard.general.string(forType: .string) else { return false }
            return text != expected && text.contains("bold") && text.contains("😀") && !text.contains("secret")
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [dragged], timeout: 15), .completed)
        waitValue(app.textFields["address"], fixture.origin + "/document-selection")
        XCTAssertEqual(fixture.requests, ["/document-selection"])
        app.buttons["add-tab"].click(); waitValue(app.textFields["address"], "about:credits")
        XCTAssertFalse(page.staticTexts["Alpha bold 😀 é"].exists)
        app.buttons["tab-1"].click(); XCTAssertTrue(first.waitForExistence(timeout: 15))
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-document-selection"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["reload"].click(); XCTAssertTrue(page.staticTexts["Visit 2"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.debugDescription.contains("private-control-secret")); XCTAssertFalse(app.debugDescription.contains("hidden-document-secret"))
        XCTAssertEqual(fixture.requests, ["/document-selection", "/document-selection"])
        page.links["selectable link"].click()
        waitValue(app.textFields["address"], fixture.origin + "/selection-follow")
        XCTAssertTrue(page.staticTexts["Selection link reached"].waitForExistence(timeout: 15))
        XCTAssertEqual(fixture.requests, ["/document-selection", "/document-selection", "/selection-follow"])
    }

    func testAccessibleUnicodeTextControlsEditingScrollingAndReload() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/accessibility-text")
        let page = app.groups["page"]
        let editor = page.textFields["Unicode editor"]
        waitValue(editor, "A😀é👨‍👩‍👧‍👦B")
        editor.click(); editor.typeKey("a", modifierFlags: .command); editor.typeText("x")
        waitValue(editor, "x")
        editor.typeKey("z", modifierFlags: .command)
        waitValue(editor, "A😀é👨‍👩‍👧‍👦B")
        let long = page.textFields["Long editor"]
        XCTAssertTrue(long.exists)
        long.click(); long.typeKey(.rightArrow, modifierFlags: .command); long.typeKey(.delete, modifierFlags: [])
        waitValue(long, String(repeating: "x", count: 1500) + "尾")
        long.typeText("visible")
        waitValue(long, String(repeating: "x", count: 1500) + "尾visible")
        let notes = page.textFields["Scrollable notes"]
        notes.click(); notes.typeKey(.downArrow, modifierFlags: .command); notes.typeText("end")
        waitValue(notes, String(repeating: "row\n", count: 50) + "end")
        waitValue(page.textFields["Readonly"], "locked")
        XCTAssertFalse(app.debugDescription.contains("private-fixture-secret"))
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-accessibility-text"; attachment.lifetime = .keepAlways; add(attachment)
        app.buttons["reload"].click()
        waitValue(editor, "A😀é👨‍👩‍👧‍👦B")
        waitValue(long, String(repeating: "x", count: 1500) + "尾😀")
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

    func testNativeSelectPopupMouseKeyboardTypeaheadAndCancellation() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/select")
        let page = app.groups["page"], region = page.comboBoxes["Region"]
        XCTAssertTrue(region.waitForExistence(timeout: 15))
        XCTAssertFalse(page.comboBoxes["Disabled choices"].isEnabled)
        region.click()
        XCTAssertTrue(app.menuItems["Beta"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.menuItems["Locked choice"].isEnabled)
        app.menuItems["中文"].click(); waitValue(region, "cn")
        app.typeText("br"); waitValue(region, "bravo")
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(app.menuItems["Beta"].waitForExistence(timeout: 15))
        app.typeKey(.escape, modifierFlags: []); waitValue(region, "bravo")
        region.click(); XCTAssertTrue(app.menuItems["Beta"].waitForExistence(timeout: 15))
        app.menuItems["Beta"].click(); waitValue(region, "same")
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = "macos-select-controls"; attachment.lifetime = .keepAlways; add(attachment)
        page.buttons["Send choices GET"].click()
        waitValue(app.textFields["address"], fixture.origin + "/received?region=same&topic=a&topic=c")
        XCTAssertEqual(fixture.requests, ["/select", "/received?region=same&topic=a&topic=c"])
    }

    func testNativeMultipleSelectionCommandClickShiftRangeSelectAllAndReset() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/select")
        let page = app.groups["page"], topics = page.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Topics")).firstMatch
        XCTAssertTrue(topics.waitForExistence(timeout: 15))
        func choice(_ title: String) -> XCUIElement { topics.groups[title] }
        func waitSelected(_ title: String, _ selected: Bool) {
            let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { object, _ in
                (object as? XCUIElement)?.isSelected == selected
            }, object: choice(title))
            XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: 15), .completed)
        }
        waitSelected("Topic Alpha", true); waitSelected("Topic Gamma", true)
        XCUIElement.perform(withKeyModifiers: .command) { choice("Topic Beta").click() }
        waitSelected("Topic Alpha", true); waitSelected("Topic Beta", true); waitSelected("Topic Gamma", true)
        XCUIElement.perform(withKeyModifiers: .command) { choice("Topic Beta").click() }
        waitSelected("Topic Beta", false)
        XCUIElement.perform(withKeyModifiers: .shift) { choice("Topic Gamma").click() }
        waitSelected("Topic Alpha", false); waitSelected("Topic Beta", true); waitSelected("Topic Gamma", true)
        XCTAssertFalse(choice("Topic Locked").isEnabled)
        app.typeKey(.end, modifierFlags: [.shift])
        waitSelected("Topic Delta", true); waitSelected("Topic Emoji 😀", true)
        app.typeKey("a", modifierFlags: [.command])
        waitSelected("Topic Alpha", true); waitSelected("Topic Locked", false)
        page.buttons["Reset choices"].click()
        waitSelected("Topic Alpha", true); waitSelected("Topic Beta", false); waitSelected("Topic Gamma", true)
        // A reset also discards the old range anchor and restores scroll position.
        app.typeKey(.tab, modifierFlags: [.shift]); app.typeKey(.downArrow, modifierFlags: [.shift])
        waitSelected("Topic Alpha", true); waitSelected("Topic Beta", true); waitSelected("Topic Gamma", false)
        page.buttons["Send choices POST"].click(); waitValue(app.textFields["address"], fixture.origin + "/posted")
        let request = try XCTUnwrap(fixture.records.last)
        XCTAssertEqual(request.method, "POST"); XCTAssertEqual(String(data: request.body, encoding: .utf8), "region=a&topic=a&topic=b")
    }

    func testChromeTabFromAddressActivatesGoWithSpace() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/editing")
        XCTAssertTrue(app.groups["page"].textFields["Editor"].waitForExistence(timeout: 15))
        paste("about:settings", into: app.textFields["address"])
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(" ", modifierFlags: [])
        let navigated = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "about:settings"),
                                                  object: app.buttons["tab-1"])
        XCTAssertEqual(XCTWaiter.wait(for: [navigated], timeout: 15), .completed,
                       "Tab must focus Go and Space must navigate the actual selected core tab")
        XCTAssertFalse(app.groups["page"].textFields["Editor"].exists)
        XCTAssertEqual(fixture.requests, ["/editing"])
    }

    func testChromeKeyboardOpensProfileMenuAndDownloadsSheet() {
        launch()
        app.typeKey("l", modifierFlags: .command)
        for _ in 0..<4 { app.typeKey(.tab, modifierFlags: []) }
        app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(app.menuItems["New Profile…"].waitForExistence(timeout: 5),
                      "A focused profile control must open its actual native menu with Space")
        app.typeKey(.escape, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(app.buttons["downloads-close"].waitForExistence(timeout: 5))
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: app.buttons["downloads-close"])], timeout: 5), .completed)
        app.typeKey(.tab, modifierFlags: .shift)
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(app.menuItems["New Profile…"].waitForExistence(timeout: 5))
        app.typeKey(.escape, modifierFlags: [])
    }

    func testChromeFindControlsHandOffToPageInBothDirections() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/keyboard")
        let page = app.groups["page"]
        XCTAssertTrue(page.textFields["Name"].waitForExistence(timeout: 15))
        app.typeKey("f", modifierFlags: .command)
        app.typeText("no-such-visible-match")
        waitValue(app.searchFields["find-query"], "no-such-visible-match")
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(" ", modifierFlags: [])
        waitChecked(app.checkBoxes["find-case"], true)
        // No-match controls are disabled; the next two targets are Close and page.
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: [])
        app.typeText("Alice")
        waitValue(page.textFields["Name"], "Alice")
        app.typeKey(.tab, modifierFlags: .shift)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: app.searchFields["find-query"])], timeout: 5), .completed,
                       "A backward page exit must reach Close find rather than the address")
        XCTAssertEqual(fixture.requests, ["/keyboard"])
    }

    func testChromeReverseTabSkipsDisabledHistoryAndActivatesReload() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/keyboard")
        XCTAssertTrue(app.groups["page"].textFields["Name"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.buttons["forward"].isEnabled)
        app.typeKey("l", modifierFlags: .command)
        app.typeKey(.tab, modifierFlags: .shift)
        app.typeKey(.return, modifierFlags: [])
        let deadline = Date().addingTimeInterval(10)
        while fixture.requests.count < 2, Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.05)) }
        XCTAssertEqual(fixture.requests, ["/keyboard", "/keyboard"])
    }

    func testChromeRapidTabsPreservePageAndAssistantTextThenRecoverAfterClose() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/keyboard")
        let page = app.groups["page"]
        XCTAssertTrue(page.textFields["Name"].waitForExistence(timeout: 15))
        app.buttons["assistant"].click()
        XCTAssertTrue(app.textFields["assistant-instruction"].waitForExistence(timeout: 10))
        app.typeKey("l", modifierFlags: .command)
        // One event stream crosses seven chrome controls into the core loop.
        app.typeText(String(repeating: "\t", count: 7) + "Alice")
        waitValue(page.textFields["Name"], "Alice")
        // Core owns seven further moves through its controls and final exit.
        app.typeText(String(repeating: "\t", count: 7))
        app.typeKey(.tab, modifierFlags: [])
        if app.buttons["assistant-summarize"].isEnabled { app.typeKey(.tab, modifierFlags: []) }
        app.typeText("Keyboard note")
        waitValue(app.textFields["assistant-instruction"], "Keyboard note")
        app.typeKey(.tab, modifierFlags: .shift)
        if app.buttons["assistant-summarize"].isEnabled { app.typeKey(.tab, modifierFlags: .shift) }
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        attachment.name = "macos-chrome-keyboard-focus"; attachment.lifetime = .keepAlways; add(attachment)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: app.textFields["assistant-instruction"])], timeout: 5), .completed)
        // The removed assistant control recovers at Zoom, then wraps to tab 1.
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(" ", modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: app.buttons["tab-1"])], timeout: 10), .completed)
        XCTAssertEqual(fixture.requests, ["/keyboard"])
    }

    func testChromeRapidAddressTabAndSpaceNavigateExactlyOnce() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/keyboard")
        XCTAssertTrue(app.groups["page"].textFields["Name"].waitForExistence(timeout: 15))
        app.typeKey("l", modifierFlags: .command)
        app.typeText(fixture.origin + "/destination\t ")
        waitValue(app.textFields["address"], fixture.origin + "/destination")
        let navigated = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", fixture.origin + "/destination"),
                                                  object: app.buttons["tab-1"])
        XCTAssertEqual(XCTWaiter.wait(for: [navigated], timeout: 15), .completed)
        XCTAssertEqual(fixture.requests, ["/keyboard", "/destination"])
    }

    func testChromeKeyboardHistoryUsesOnlyEnabledDirections() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); enter(fixture.origin + "/first"); enter(fixture.origin + "/second")
        app.typeKey("l", modifierFlags: .command)
        app.typeText("\t") // Go is reachable in the forward direction.
        app.typeKey(.tab, modifierFlags: .shift)
        app.typeKey(.tab, modifierFlags: .shift) // Reload.
        app.typeKey(.tab, modifierFlags: .shift) // Back; disabled Forward is skipped.
        app.typeKey(" ", modifierFlags: [])
        waitValue(app.textFields["address"], fixture.origin + "/first")
        app.typeKey("l", modifierFlags: .command)
        app.typeKey(.tab, modifierFlags: .shift) // Reload.
        app.typeKey(.tab, modifierFlags: .shift) // Enabled Forward.
        app.typeKey(.return, modifierFlags: [])
        waitValue(app.textFields["address"], fixture.origin + "/second")
        XCTAssertEqual(fixture.requests, ["/first", "/second", "/first", "/second"])
    }

    func testChromeFocusScrollsTabsAndRecoversAfterClosingSelectedTab() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch()
        for index in 1...7 {
            if index > 1 { app.typeKey("t", modifierFlags: .command) }
            paste(fixture.origin + "/editing", into: app.textFields["address"])
            app.typeKey(.return, modifierFlags: [])
            XCTAssertTrue(app.groups["page"].textFields["Editor"].waitForExistence(timeout: 15))
        }
        app.typeKey("l", modifierFlags: .command)
        app.typeKey(.tab, modifierFlags: .shift) // Reload.
        if app.buttons["back"].isEnabled { app.typeKey(.tab, modifierFlags: .shift) }
        for _ in 0..<3 { app.typeKey(.tab, modifierFlags: .shift) } // Add, New Group, Close tab 7.
        let close = app.buttons["close-tab-7"]
        XCTAssertTrue(close.isHittable, "Keyboard focus must reveal an offscreen tab in the strip")
        let attachment = XCTAttachment(screenshot: app.windows["browser-window"].screenshot())
        attachment.name = "macos-chrome-scrolled-tab-focus"; attachment.lifetime = .keepAlways; add(attachment)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: close)], timeout: 10), .completed)
        // Closing the selected tab changes core ownership; the surviving next
        // chrome control is still New Group and opens the real native sheet.
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(app.textFields["group-name"].waitForExistence(timeout: 5))
        app.typeKey(.escape, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: .shift)
        XCTAssertTrue(app.buttons["close-tab-6"].isHittable)
        app.typeKey(" ", modifierFlags: [])
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: app.buttons["close-tab-6"])], timeout: 10), .completed)
        XCTAssertEqual(fixture.requests, Array(repeating: "/editing", count: 7))
    }

    func testChromeKeyboardActionsStayInActiveWindowAndOpenOwnedPermissions() throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        launch(); let first = app.windows["browser-window"]
        enter(fixture.origin + "/editing", in: first)
        app.typeKey("n", modifierFlags: .command)
        let second = app.windows["browser-window-2"]
        XCTAssertTrue(second.waitForExistence(timeout: 15))
        enter(fixture.origin + "/keyboard", in: second)
        activateWindow(1); app.typeKey("l", modifierFlags: .command)
        app.typeText("\t\t ")
        XCTAssertTrue(first.textFields["assistant-instruction"].waitForExistence(timeout: 5))
        XCTAssertFalse(second.textFields["assistant-instruction"].exists)
        activateWindow(2); app.typeKey("l", modifierFlags: .command)
        app.typeText(String(repeating: "\t", count: 6))
        app.typeKey(.return, modifierFlags: [])
        let panels = XCUIApplication(bundleIdentifier: "cc.blueice.BlueIcePanels")
        XCTAssertTrue(panels.windows["permissions-window"].waitForExistence(timeout: 10))
        XCTAssertTrue(panels.staticTexts["permissions-empty"].waitForExistence(timeout: 10))
        XCTAssertEqual(panels.state, .runningForeground)
        app.activate(); second.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: second)], timeout: 10), .completed)
        app.typeKey("l", modifierFlags: .command); app.typeText("\t\t ")
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: first.textFields["assistant-instruction"])], timeout: 5), .completed)
        XCTAssertEqual(fixture.requests, ["/editing", "/keyboard"])
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
        let disabledReset = page.buttons["Disabled reset"], window = app.windows["browser-window"]
        XCTAssertFalse(disabledReset.isEnabled)
        let disabledFrame = disabledReset.frame
        XCTAssertGreaterThan(disabledFrame.width, 0); XCTAssertGreaterThan(disabledFrame.height, 0)
        XCTAssertTrue(window.frame.contains(disabledFrame))
        // A coordinate based on a disabled element can fall back to the window
        // center. Anchor its actual frame to the enabled window instead.
        window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: disabledFrame.midX - window.frame.minX,
            dy: disabledFrame.midY - window.frame.minY)).click()
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

    func testReverseTabAndPageBoundaryTraverseChromeToNativeAddressEditing() throws {
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
            // The confirmed backward core exit reaches Permissions. Traverse
            // the enabled toolbar in reverse before editing the address.
            for _ in 0..<6 { app.typeKey(.tab, modifierFlags: .shift) }
            app.typeText("chrome-probe")
            let address = app.textFields["address"]
            let chrome = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS 'chrome-probe'"), object: address)
            XCTAssertEqual(XCTWaiter.wait(for: [chrome], timeout: 15), .completed,
                           "Reverse Tab through chrome must reach native address editing; received \(String(describing: address.value))")
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

    func testMissingLauncherShowsFailureAndDisabledNavigation() throws {
        let fixtureRoot = FileManager.default.temporaryDirectory.appendingPathComponent("blueice-launcher-retry-" + UUID().uuidString)
        let directory = fixtureRoot.appendingPathComponent("BlueIce.app/Contents/MacOS")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: fixtureRoot) }
        app.launchArguments += ["--launcher-exe", directory.appendingPathComponent("blueice-launcher").path]
        app.launch()
        let failed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS[c] 'launcher' OR label CONTAINS[c] 'launcher'"), object: app.staticTexts["status"])
        XCTAssertEqual(XCTWaiter.wait(for: [failed], timeout: 10), .completed, app.debugDescription)
        XCTAssertFalse(app.buttons["reload"].isEnabled)
        XCTAssertFalse(app.buttons["add-tab"].isEnabled)
        let application = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce").first)
        let originalPID = application.processIdentifier
        XCTAssertTrue(app.buttons["restart-browser"].isEnabled)
        waitAssistantText(app.staticTexts["recovery-details"], "No confirmed session is available. Restart opens a new page.")
        app.typeKey(.tab, modifierFlags: []); app.typeKey(" ", modifierFlags: [])
        XCTAssertTrue(app.buttons["restart-browser"].waitForExistence(timeout: 10))
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "enabled == true"), object: app.buttons["restart-browser"])], timeout: 10), .completed)
        XCTAssertFalse(app.buttons["reload"].isEnabled)
        let bundled = try XCTUnwrap(application.bundleURL).appendingPathComponent("Contents/MacOS")
        for name in ["blueice-launcher", "blueice-core", "blueice-ai-gatekeeper", "BlueIcePanels.app"] {
            try FileManager.default.createSymbolicLink(at: directory.appendingPathComponent(name), withDestinationURL: bundled.appendingPathComponent(name))
        }
        app.buttons["restart-browser"].click()
        waitValue(app.textFields["address"], "about:credits")
        waitAssistantText(app.staticTexts["status"], "Ready")
        XCTAssertTrue(app.buttons["reload"].isEnabled)
        XCTAssertFalse(app.buttons["restart-browser"].exists)
        XCTAssertEqual(NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIce").first?.processIdentifier, originalPID)
        XCTAssertEqual(app.windows.matching(NSPredicate(format: "identifier BEGINSWITH 'browser-window'")).count, 1)
    }

    func testWindowCloseTerminatesApplication() {
        launch()
        app.windows.firstMatch.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 10))
    }

    func testMissingCoreShowsFailureAndDisabledNavigation() {
        app.launchArguments += ["--core-exe", "/nonexistent/blueice-core"]
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
