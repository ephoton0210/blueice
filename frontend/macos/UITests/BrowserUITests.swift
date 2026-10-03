// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Carbon
import ApplicationServices
import XCTest

@MainActor
final class BrowserUITests: XCTestCase {
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
        XCTAssertTrue(field.waitForExistence(timeout: 5)); field.click()
        field.typeKey("a", modifierFlags: .command); field.typeText(text)
    }
    func testNativeAssistantSettingsReviewCancelApplyAndRelaunch() throws {
        launch()
        var panels = openAssistantSettings()
        waitPermissionValue(panels.staticTexts["assistant-settings-current"], "In force: none · idle 600 seconds · niceness 10")
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
        waitPermissionValue(panels.staticTexts["assistant-settings-current"], "In force: none · idle 630 seconds · niceness 10")
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
    private func openPermissionPanel() -> XCUIApplication {
        app.buttons["permissions"].click()
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
    func testNativePermissionChildExitClosesTheOwnedServiceConnection() throws {
        launch()
        let panels = openPermissionPanel()
        XCTAssertTrue(panels.staticTexts["permissions-empty"].waitForExistence(timeout: 10))
        // This companion is launcher-owned, so XCTest's terminate/reap path
        // races the launcher's own waitpid. Kill only the uniquely identified
        // child and observe both applications through their ordinary UI.
        let ownedPanels = NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BlueIcePanels")
        XCTAssertEqual(ownedPanels.count, 1, "Identify this test's permission child before terminating it")
        let child = try XCTUnwrap(ownedPanels.first)
        XCTAssertEqual(kill(child.processIdentifier, SIGKILL), 0)
        XCTAssertTrue(panels.wait(for: .notRunning, timeout: 10))
        let status = app.staticTexts["status"]
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate:
            NSPredicate(format: "value BEGINSWITH 'Browser service connection ended:'"), object: status)], timeout: 15), .completed, app.debugDescription)
        XCTAssertFalse(app.textFields["address"].isEnabled)
        XCTAssertFalse(app.buttons["permissions"].isEnabled)
        XCTAssertTrue(app.windows["browser-window"].exists)
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
        let panel = app.dialogs["Print"]
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
        app.menuBars.menuBarItems["File"].click()
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"),object: app.menuItems["Print…"])],timeout: 15),.completed)
        app.typeKey(.escape,modifierFlags: [])
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
        let cancel = app.dialogs["Print"].buttons["Cancel"]
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

    func testNativeDownloadsTransferControlsPersistenceAndFinderActions() throws {
        let fixture = try DownloadFixture(); defer { fixture.stop() }
        let root = URL(fileURLWithPath: "/private/tmp/bi-download-ui-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let files = root.appendingPathComponent("files")
        app.launchArguments += ["--downloads-directory",files.path,"--downloads-data-directory",root.appendingPathComponent("data").path]
        let saved = saveClipboard(); defer { NSPasteboard.general.clearContents(); NSPasteboard.general.writeObjects(saved) }
        launch()
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
        app.windows["browser-window"].buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: .notRunning,timeout: 15))
        let before = fixture.requests.count
        launch(); app.buttons["downloads"].click(); waitDownload(5,"Paused")
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

    private var app: XCUIApplication!
    private var originalInputSource: TISInputSource?
    private var preferenceDomain = ""
    private var assistantSettingsFile: URL!
    private var assistantControlSocket: URL!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        preferenceDomain = "cc.blueice.uitests." + UUID().uuidString
        app.launchArguments = ["--preferences-domain", preferenceDomain]
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
            try? FileManager.default.removeItem(at: assistantSettingsFile.deletingLastPathComponent())
            try? FileManager.default.removeItem(at: assistantControlSocket)
            UserDefaults(suiteName: preferenceDomain)?.removePersistentDomain(forName: preferenceDomain)
            if let originalInputSource { XCTAssertEqual(TISSelectInputSource(originalInputSource), noErr) }
        }
        if app.state != .notRunning {
            if app.sheets["open-panel"].buttons["Cancel"].exists { app.sheets["open-panel"].buttons["Cancel"].click() }
            if app.sheets["GoToWindow"].exists { app.typeKey(.escape,modifierFlags: []) }
            if app.sheets["save-panel"].buttons["CancelButton"].exists { app.sheets["save-panel"].buttons["CancelButton"].click() }
            if app.sheets["alert"].buttons["OK"].exists { app.sheets["alert"].buttons["OK"].click() }
            if app.dialogs["Print"].menus.firstMatch.exists { app.typeKey(.escape,modifierFlags: []) }
            if app.dialogs["Print"].buttons["Cancel"].exists { app.dialogs["Print"].buttons["Cancel"].click() }
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
        app.launchArguments += ["--launcher-exe", "/nonexistent/blueice-launcher"]
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
