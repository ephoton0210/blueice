// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
import XCTest
import Darwin

@MainActor
final class SessionRestoreTests: XCTestCase {
    func testRecoveryRestorationRejectsUserCommandsBeforeCanonicalStateSettles() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let domain = "cc.blueice.session-test." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let key = UUID(), workKey = UUID(), workWindow = UUID()
        try BrowserContextPreferences(defaults: defaults).save([
            .init(id: BrowserContextPreferences.defaultKey, name: "Default"), .init(id: workKey, name: "Work")
        ])
        let saved = SavedBrowserSession(version: 1, profiles: [.init(key: BrowserContextPreferences.defaultKey, groups: [], windows: [
            .init(key: key, tabs: [.init(history: .init(entries: [.init(url: fixture.origin + "/appearance", was_post: false)], cursor: 0, zoom: 1), group: nil)], selected: 0, frame: nil)
        ]), .init(key: workKey, groups: [.init(name: "Work study", color: "#4477cc", collapsed: true)], windows: [
            .init(key: workWindow, tabs: [.init(history: .init(entries: [.init(url: fixture.origin + "/editing", was_post: false)], cursor: 0, zoom: 1.5), group: 0)], selected: 0, frame: nil)
        ])], active: workWindow)
        let workspace = BrowserWorkspace(contextDefaults: defaults, recovery: saved)
        var attemptedInput = false
        workspace.onWindowCreated = { model in
            guard workspace.restoringSession else { return }
            attemptedInput = true
            model.action(.values("OpenTab", ["url": .string(fixture.origin + "/unexpected")]))
        }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent()
            .appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await workspace.start(launcher: launcher)
        workspace.onWindowCreated = nil
        defer { Task { await workspace.stop() } }
        XCTAssertTrue(attemptedInput, "Exercise user input at the real replacement-window boundary")
        XCTAssertEqual(workspace.windows.count, 2)
        XCTAssertEqual(workspace.windows.map { $0.tabs.count }, [1, 1], "Recovery must reject a user command racing its canonical tab creation")
        XCTAssertEqual(fixture.requests, ["/appearance", "/editing"])
        let work = try XCTUnwrap(workspace.contexts.first { $0.name == "Work" })
        XCTAssertEqual(work.groups.map(\.name), ["Work study"])
        XCTAssertEqual(work.groups.first?.collapsed, true)
        XCTAssertEqual(workspace.models[workspace.activeWindowID]?.contextID, work.id)
        XCTAssertEqual(workspace.models[workspace.activeWindowID]?.canInteract, true)
        XCTAssertEqual(workspace.windows.first { work.windows.contains($0.id) }?.tabs.first?.groupID, work.groups.first?.id)
        XCTAssertNil(defaults.object(forKey: "browser.session.archive"))
        await workspace.stop()
    }
    func testFailedRestartIsRetryableUsesNewRuntimeAndRejectsDuplicateAndOldOwners() async throws {
        let domain = "cc.blueice.session-test." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let fixtureRoot = FileManager.default.temporaryDirectory.appendingPathComponent("blueice-recovery-" + UUID().uuidString)
        let directory = fixtureRoot.appendingPathComponent("BlueIce.app/Contents/MacOS")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: fixtureRoot) }
        let bundled = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS")
        let launcher = directory.appendingPathComponent("blueice-launcher")
        let invalidArchive = Data("unreadable recovery archive".utf8)
        defaults.set(invalidArchive, forKey: "browser.session.archive")
        let stopped = BrowserWorkspace(contextDefaults: defaults)
        let oldModel = try XCTUnwrap(stopped.models[1])
        await stopped.start(launcher: launcher)
        XCTAssertNotNil(stopped.failureMessage)
        XCTAssertNil(stopped.recoverySnapshot)
        XCTAssertFalse(oldModel.canInteract)
        var attempts = 0
        var failedReplacement: BrowserWorkspace?
        stopped.onRecovery = { replacement in
            attempts += 1; failedReplacement = replacement
            await replacement.startReplacement()
        }
        let duplicate = Task { await stopped.restartBrowser() }
        await stopped.restartBrowser(); await duplicate.value
        XCTAssertEqual(attempts, 1, "A second activation cannot launch another replacement")
        let retry = try XCTUnwrap(failedReplacement)
        XCTAssertNotNil(retry.failureMessage)
        XCTAssertNotEqual(retry.session.runtimeDirectory, stopped.session.runtimeDirectory)
        XCTAssertFalse(FileManager.default.fileExists(atPath: stopped.session.runtimeDirectory.path))
        var recovered: BrowserWorkspace?
        retry.onRecovery = { replacement in recovered = replacement; await replacement.startReplacement() }
        XCTAssertTrue(retry.canRestart)
        for name in ["blueice-launcher", "blueice-core", "blueice-ai-gatekeeper", "BlueIcePanels.app"] {
            try FileManager.default.createSymbolicLink(at: directory.appendingPathComponent(name), withDestinationURL: bundled.appendingPathComponent(name))
        }
        await retry.restartBrowser()
        let current = try XCTUnwrap(recovered)
        defer { Task { await current.stop() } }
        XCTAssertNil(current.failureMessage)
        XCTAssertNotEqual(current.session.runtimeDirectory, retry.session.runtimeDirectory)
        XCTAssertFalse(FileManager.default.fileExists(atPath: retry.session.runtimeDirectory.path))
        let pid = try XCTUnwrap(current.processID)
        await wait { current.models[1]?.tabs.first?.url == "about:credits" && (current.models[1]?.generation ?? 0) > 0 }
        XCTAssertEqual(current.windows.count, 1)
        XCTAssertEqual(current.models[1]?.tabs.first?.url, "about:credits")
        XCTAssertEqual(current.models[1]?.canInteract, true)
        oldModel.action(.values("OpenTab", ["url": .string("about:settings")]))
        let oldClose = await stopped.closeWindow(1)
        XCTAssertFalse(oldClose)
        XCTAssertEqual(current.windows.first?.tabs.count, 1)
        XCTAssertEqual(current.processID, pid)
        XCTAssertEqual(defaults.data(forKey: "browser.session.archive"), invalidArchive)
        XCTAssertNotNil(current.sessionPreferences.error)
        await current.stop()
        XCTAssertFalse(FileManager.default.fileExists(atPath: current.session.runtimeDirectory.path))
        XCTAssertEqual(kill(pid, 0), -1)
    }
    func testVolatileCaptureKeepsLastConfirmedSnapshotWhenNewWindowFrameIsInvalid() async throws {
        let domain = "cc.blueice.session-test." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        let workspace = BrowserWorkspace(contextDefaults: defaults)
        defer { Task { await workspace.stop() } }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent()
            .appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        await workspace.start(launcher: launcher)
        await wait { workspace.recoverySnapshot?.profiles.first?.windows.first?.tabs.first?.history.current.url == "about:credits" }
        let confirmed = try XCTUnwrap(workspace.recoverySnapshot)
        let model = try XCTUnwrap(workspace.models[1])
        var attemptedCapture = false
        workspace.onCaptureWindowFrame = { _ in
            attemptedCapture = true
            return .init(x: 0, y: 0, width: 1, height: 1)
        }
        model.address = "about:settings"; model.navigateAddress()
        await wait { attemptedCapture && model.tabs.first?.url == "about:settings" }
        XCTAssertEqual(workspace.recoverySnapshot, confirmed, "An incomplete capture must never replace the confirmed recovery session")
        XCTAssertNil(defaults.object(forKey: "browser.session.archive"))
        workspace.onCaptureWindowFrame = nil
        workspace.scheduleSessionSave()
        await wait { workspace.recoverySnapshot?.profiles.first?.windows.first?.tabs.first?.history.current.url == "about:settings" }
        XCTAssertTrue(workspace.recoverySnapshot?.valid == true)
        await workspace.stop()
    }
    private func wait(_ predicate: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
        let deadline = Date().addingTimeInterval(10)
        while !predicate(), Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(predicate(), file: file, line: line)
    }
    func testClosingRecoveryWindowPreservesProfilesGroupsAndChoosesSurvivingActiveWindow() {
        let original = snapshot(), secondKey = UUID()
        let profile = original.profiles[0]
        let expanded = SavedBrowserSession(version: 1, profiles: [
            .init(key: profile.key, groups: profile.groups, windows: profile.windows + [
                .init(key: secondKey, tabs: profile.windows[0].tabs, selected: 0, frame: nil)
            ])
        ], active: original.active)
        let reduced = expanded.removingWindow(original.active)
        XCTAssertEqual(reduced?.active, secondKey)
        XCTAssertEqual(reduced?.profiles[0].groups, profile.groups)
        XCTAssertEqual(reduced?.profiles[0].windows.count, 1)
        XCTAssertEqual(reduced?.profiles[0].windows[0].tabs, profile.windows[0].tabs)
        XCTAssertEqual(expanded.removingWindow(UUID()), expanded)
        XCTAssertNil(reduced?.removingWindow(secondKey))
    }
    func snapshot() -> SavedBrowserSession {
        let key=UUID()
        return .init(version:1,profiles:[.init(key:BrowserContextPreferences.defaultKey,groups:[.init(name:"Study",color:"#123456",collapsed:true)],windows:[.init(key:key,tabs:[.init(history:.init(entries:[.init(url:"https://example.test/form",was_post:false),.init(url:"https://example.test/result",was_post:true)],cursor:1,zoom:1.5),group:0)],selected:0,frame:.init(x:20,y:20,width:900,height:650))])],active:key)
    }
    func testSavedMetadataRoundTripsWithoutEphemeralIDsOrPageData() throws {
        let session=snapshot();XCTAssertTrue(session.valid)
        let data=try JSONEncoder().encode(session);let text=String(decoding:data,as:UTF8.self)
        for key in ["tab_id","frame_source","document_generation","body","html","password","permission"] { XCTAssertFalse(text.contains(key)) }
        XCTAssertEqual(try JSONDecoder().decode(SavedBrowserSession.self,from:data),session)
    }
    func testRememberingIsOptInAndForgetRemovesPersistedMetadata() throws {
        let suite="cc.blueice.session-test."+UUID().uuidString;let defaults=UserDefaults(suiteName:suite)!;defer{defaults.removePersistentDomain(forName:suite)}
        let prefs=BrowserSessionPreferences(defaults:defaults);XCTAssertFalse(prefs.remember);XCTAssertThrowsError(try prefs.save(snapshot()))
        prefs.setRemember(true);prefs.setReopen(true);try prefs.save(snapshot())
        let reopened=BrowserSessionPreferences(defaults:defaults);XCTAssertEqual(reopened.saved,prefs.saved);XCTAssertTrue(reopened.reopen)
        reopened.setRemember(false);XCTAssertNil(defaults.object(forKey:"browser.session.archive"));XCTAssertFalse(reopened.reopen)
    }
    func testMalformedStoredArchiveIsPreservedUntilExplicitForget() throws {
        let suite="cc.blueice.session-test."+UUID().uuidString;let defaults=UserDefaults(suiteName:suite)!;defer{defaults.removePersistentDomain(forName:suite)}
        let raw=Data("invalid archive".utf8);defaults.set(raw,forKey:"browser.session.archive")
        let prefs=BrowserSessionPreferences(defaults:defaults);prefs.setRemember(true)
        XCTAssertNotNil(prefs.error);XCTAssertThrowsError(try prefs.save(snapshot()));XCTAssertEqual(defaults.data(forKey:"browser.session.archive"),raw)
        prefs.setRemember(false);XCTAssertNil(prefs.error);XCTAssertNil(defaults.data(forKey:"browser.session.archive"))
    }
    func testHistoryBoundsCredentialsUnsupportedSchemesAndInvalidCursorFailClosed() {
        for url in ["https://name:secret@example.test/","file:///private/secret","javascript:alert(1)","http://","about:extension-popup"] {
            XCTAssertFalse(NavigationEntry(url:url,was_post:false).valid)
        }
        XCTAssertFalse(NavigationHistory(entries:[.init(url:nil,was_post:true)],cursor:0,zoom:1).valid)
        XCTAssertFalse(NavigationHistory(entries:[.init(url:nil,was_post:false)],cursor:1,zoom:1).valid)
        XCTAssertFalse(NavigationHistory(entries:[.init(url:nil,was_post:false)],cursor:0,zoom:Double.infinity).valid)
    }
}
