// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import XCTest

@MainActor
final class LocalizationTests: XCTestCase {
    func testBundledLanguagesResolveRegionalPreferencesAndUnknownKeys() throws {
        XCTAssertEqual(BrowserStrings.resolved(.system, preferred: ["zh-TW", "en"]), "zh-Hant")
        XCTAssertEqual(BrowserStrings.resolved(.system, preferred: ["zh-Hant-HK"]), "zh-Hant")
        XCTAssertEqual(BrowserStrings.resolved(.system, preferred: ["de-DE"]), "en")
        XCTAssertEqual(BrowserStrings.resolved(.english, preferred: ["zh-TW"]), "en")
        XCTAssertEqual(BrowserStrings.text("Back", language: "en"), "Back")
        XCTAssertEqual(BrowserStrings.text("Back", language: "zh-Hant"), "返回")
        XCTAssertEqual(BrowserStrings.text("界面以外 😀 % secret", language: "zh-Hant"), "界面以外 😀 % secret")
        XCTAssertEqual(BrowserStrings.text("Back", language: "../../private"), "Back")
        XCTAssertEqual(BrowserStrings.format("Rendered %llu × %llu, frame %llu", UInt64(2240), UInt64(1360), UInt64(7)), "Rendered 2240 × 1360, frame 7")
    }
    func testPreferenceRelaunchPreservesPageTranslationAndOtherState() throws {
        let domain = "cc.blueice.localization-tests." + UUID().uuidString
        let defaults = try XCTUnwrap(UserDefaults(suiteName: domain))
        defer { defaults.removePersistentDomain(forName: domain) }
        defaults.set("fr", forKey: "browser.translationLanguage")
        defaults.set("keep-session", forKey: "browser.session.archive")
        defaults.set("../invalid", forKey: BrowserStrings.preferenceKey)
        let first = BrowserLocalization(defaults: defaults, domain: domain, observeChanges: false)
        XCTAssertEqual(first.language, .system)
        first.setLanguage(.traditionalChinese)
        let relaunched = BrowserLocalization(defaults: defaults, domain: domain, observeChanges: false)
        XCTAssertEqual(relaunched.language, .traditionalChinese)
        XCTAssertEqual(relaunched.locale.identifier, "zh-Hant")
        XCTAssertEqual(defaults.string(forKey: "browser.translationLanguage"), "fr")
        XCTAssertEqual(defaults.string(forKey: "browser.session.archive"), "keep-session")
        defaults.set("en", forKey: BrowserStrings.preferenceKey); relaunched.refresh()
        XCTAssertEqual(relaunched.language, .english)
    }
    func testOwnerPreferenceDomainIsBoundedAndFallsBackSafely() {
        XCTAssertEqual(BrowserStrings.preferenceDomain(arguments: ["app", "--preferences-domain", "test.owner"], environment: [:]), "test.owner")
        XCTAssertEqual(BrowserStrings.preferenceDomain(arguments: ["panel"], environment: ["BLUEICE_PREFERENCES_DOMAIN": "test.owner"]), "test.owner")
        XCTAssertEqual(BrowserStrings.preferenceDomain(arguments: ["app", "--preferences-domain", String(repeating: "x", count: 256)], environment: [:]), "cc.blueice.BlueIce")
        XCTAssertEqual(BrowserStrings.preferenceDomain(arguments: [], environment: ["BLUEICE_PREFERENCES_DOMAIN": "bad\nname"]), "cc.blueice.BlueIce")
    }
    func testResourceTablesHaveMatchingKeysAndFormatArguments() throws {
        func table(_ language: String) throws -> [String: String] {
            let path = try XCTUnwrap(BrowserStrings.resourceBundle.path(forResource: "Localizable", ofType: "strings", inDirectory: language + ".lproj"))
            let value = try PropertyListSerialization.propertyList(from: Data(contentsOf: URL(fileURLWithPath: path)), options: [], format: nil)
            return try XCTUnwrap(value as? [String: String])
        }
        let english = try table("en"), chinese = try table("zh-Hant")
        XCTAssertGreaterThan(english.count, 200)
        XCTAssertEqual(Set(english.keys), Set(chinese.keys))
        let formats = try NSRegularExpression(pattern: "%[0-9]*\\$?(?:ll)?[diu@f]")
        func signature(_ value: String) -> [String] {
            formats.matches(in: value, range: NSRange(value.startIndex..., in: value)).map {
                String(value[Range($0.range, in: value)!]).replacingOccurrences(of: "%[0-9]+\\$", with: "%", options: .regularExpression)
            }.sorted()
        }
        for (key, value) in english {
            XCTAssertFalse(chinese[key]?.isEmpty ?? true, key)
            XCTAssertEqual(signature(value), signature(chinese[key] ?? ""), key)
        }
        let pattern = try XCTUnwrap(chinese["Resending will repeat the previous form submission to %@."])
        let text = String(format: pattern, "例子😀%2$@.test")
        XCTAssertEqual(text, "重新傳送會再次提交先前的表單至 例子😀%2$@.test。")
    }
}
