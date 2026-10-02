// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import XCTest

@MainActor
final class BrowserAppearanceTests: XCTestCase {
    private func defaults() -> (UserDefaults, String) {
        let name = "cc.blueice.appearance.tests." + UUID().uuidString
        return (UserDefaults(suiteName: name)!, name)
    }
    func testChoicesResolveAndPersistWithoutChangingSystemPreferences() {
        let (store, domain) = defaults(); defer { store.removePersistentDomain(forName: domain) }
        let system = DisplayPreferences(dark: false, highContrast: true, reducedMotion: false)
        let appearance = BrowserAppearance(defaults: store, systemPreferences: { system }, observeApplication: false)
        XCTAssertEqual(appearance.resolved, system)
        appearance.setAppearance(.dark); appearance.setContrast(.standard); appearance.setMotion(.reduced)
        XCTAssertEqual(appearance.resolved, DisplayPreferences(dark: true, highContrast: false, reducedMotion: true))
        let restored = BrowserAppearance(defaults: store, systemPreferences: { system }, observeApplication: false)
        XCTAssertEqual(restored.appearance, .dark); XCTAssertEqual(restored.contrast, .standard); XCTAssertEqual(restored.motion, .reduced)
        restored.restoreSystem()
        XCTAssertEqual(restored.resolved, system)
        XCTAssertEqual(restored.windowAppearance?.name, nil)
        XCTAssertEqual(system, DisplayPreferences(dark: false, highContrast: true, reducedMotion: false))
    }
    func testSystemNotificationUpdatesResolvedPreferencesAndHonorsOverrides() async {
        let (store, domain) = defaults(); defer { store.removePersistentDomain(forName: domain) }
        var system = DisplayPreferences()
        let center = NotificationCenter()
        let appearance = BrowserAppearance(defaults: store, systemPreferences: { system }, notificationCenter: center, observeApplication: false)
        system = DisplayPreferences(dark: true, highContrast: true, reducedMotion: true)
        center.post(name: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification, object: nil)
        await wait { appearance.resolved == system }
        appearance.setAppearance(.light); appearance.setContrast(.standard)
        system = DisplayPreferences(dark: false, highContrast: true, reducedMotion: false)
        center.post(name: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification, object: nil)
        await wait { appearance.resolved == DisplayPreferences() }
        appearance.setContrast(.increased)
        XCTAssertTrue(appearance.resolved.highContrast)
        XCTAssertEqual(appearance.windowAppearance?.bestMatch(from: [.aqua, .darkAqua]), .aqua)
    }
    func testApplicationEffectiveAppearanceUsesNativeObservation() async {
        let application = NSApplication.shared
        let previous = application.appearance; defer { application.appearance = previous }
        let (store, domain) = defaults(); defer { store.removePersistentDomain(forName: domain) }
        application.appearance = NSAppearance(named: .aqua)
        let appearance = BrowserAppearance(defaults: store)
        XCTAssertFalse(appearance.resolved.dark)
        application.appearance = NSAppearance(named: .darkAqua)
        await wait { appearance.resolved.dark }
        appearance.setAppearance(.light)
        XCTAssertFalse(appearance.resolved.dark)
        XCTAssertEqual(application.appearance?.name, .darkAqua, "The window override must not change application/system appearance")
    }
    func testDisplayWireRequiresTypedBooleansAndPreservesCorrelationFields() throws {
        let preferences = DisplayPreferences(dark: true, highContrast: true, reducedMotion: true)
        let bytes = try BrowserWire.encode(.displayPreferences(preferences), tab: 2, request: 7)
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes.dropFirst(4)) as? [String: Any])
        let payload = ((root["message"] as? [String: Any])?["SetDisplayPreferences"] as? [String: Any])?["preferences"] as? [String: Bool]
        XCTAssertEqual(payload, ["dark": true, "high_contrast": true, "reduced_motion": true])
        func decode(_ bad: Bool) throws -> BrowserMessage {
            let fields: [String: Any] = ["tab_id": 2, "frame_source": 19, "frame_generation": 8,
                "preferences": ["dark": bad ? "yes" as Any : true as Any, "high_contrast": true, "reduced_motion": false]]
            return try JSONDecoder().decode(IncomingEnvelope.self, from: JSONSerialization.data(withJSONObject: ["message": ["DisplayPreferencesState": fields]] )).message
        }
        if case .displayPreferences(let state) = try decode(false) { XCTAssertEqual(state.tabID, 2); XCTAssertEqual(state.frameGeneration, 8) }
        else { XCTFail("Typed preference state must reach the model") }
        if case .displayPreferencesUnavailable = try decode(true) {} else { XCTFail("Malformed preference booleans must fail soft") }
    }
    func testCSSPixelsHaveAnExplicitSRGBProfileAndRoundTripWithoutDeviceColorChanges() throws {
        let pixels = Data([170, 187, 204, 255, 16, 32, 48, 255])
        let frame = FramePixels(data: pixels, width: 2, height: 1, generation: 1)
        let image = try frame.image()
        XCTAssertEqual(image.colorSpace?.name, CGColorSpace.sRGB)
        let colorSpace = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        let context = try XCTUnwrap(CGContext(data: nil, width: 2, height: 1, bitsPerComponent: 8, bytesPerRow: 8,
            space: colorSpace, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.draw(image, in: CGRect(x: 0, y: 0, width: 2, height: 1))
        let result = Data(bytes: try XCTUnwrap(context.data), count: 8)
        XCTAssertEqual(result, pixels)
    }
    private func wait(_ condition: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async {
        let deadline = Date().addingTimeInterval(5)
        while !condition() && Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(condition(), file: file, line: line)
    }
}
