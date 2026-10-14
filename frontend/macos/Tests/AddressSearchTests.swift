// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import CoreFoundation
import XCTest

@MainActor
final class AddressSearchTests: XCTestCase {
    private func preferences() -> (BrowserSearchPreferences, UserDefaults, String) {
        let domain = "cc.blueice.search-tests." + UUID().uuidString
        let defaults = UserDefaults(suiteName: domain)!
        return (BrowserSearchPreferences(defaults: defaults), defaults, domain)
    }
    func testAddressesRetainNavigationAndNeverBecomeThirdPartyQueries() throws {
        let (settings, defaults, domain) = preferences(); defer { defaults.removePersistentDomain(forName: domain) }
        for (input, expected) in [("example.test/path?x=1", "https://example.test/path?x=1"),
            ("http://127.0.0.1:8000/notes", "http://127.0.0.1:8000/notes"),
            ("localhost:8000/notes", "https://localhost:8000/notes"),
            ("example.test:8443/notes", "https://example.test:8443/notes"),
            ("[::1]:8000/notes", "https://[::1]:8000/notes"),
            ("//example.test/notes", "https://example.test/notes"),
            ("/tmp/notes.txt", "file:///tmp/notes.txt"),
            ("about:credits", "about:credits"), ("file:///tmp/notes.txt", "file:///tmp/notes.txt"),
            ("javascript:alert(1)", "javascript:alert(1)"), ("malformed:secret", "malformed:secret"),
            ("mailto:user@example.test", "mailto:user@example.test"),
            ("ftp:user@example.test/path", "ftp:user@example.test/path"),
            ("custom:token@service.test", "custom:token@service.test"),
            ("custom:123", "custom:123") ] {
            let target = try settings.resolve(input)
            XCTAssertFalse(target.isSearch, input); XCTAssertEqual(target.url, expected, input)
        }
        let longURL = "https://example.test/" + String(repeating: "x",count: 10000)
        XCTAssertEqual(try settings.resolve(longURL),BrowserAddressTarget(url: longURL,isSearch: false),"Existing URL transport limits remain core-owned")
    }
    func testSearchEncodesAnExactSingleQueryIncludingUnicodeAndLiteralPlus() throws {
        let (settings, defaults, domain) = preferences(); defer { defaults.removePersistentDomain(forName: domain) }
        let query = "C++ & 東京 👨‍👩‍👧 #100%"
        let target = try settings.resolve("  " + query + "  ")
        XCTAssertTrue(target.isSearch)
        XCTAssertTrue(target.url.hasPrefix("https://duckduckgo.com/?q="))
        XCTAssertTrue(target.url.contains("C%2B%2B%20%26%20")); XCTAssertFalse(target.url.contains("+"))
        XCTAssertEqual(URLComponents(string: target.url)?.queryItems, [URLQueryItem(name: "q", value: query)])
        XCTAssertTrue(try settings.resolve("?example.test").isSearch)
        XCTAssertThrowsError(try settings.resolve("one\ntwo"))
        XCTAssertThrowsError(try settings.resolve("one\u{2028}two"))
        XCTAssertThrowsError(try settings.resolve(String(repeating: "x", count: 2049)))
        XCTAssertThrowsError(try settings.resolve("?"))
    }
    func testCustomProviderPreservesFixedParametersAndReplacesDuplicateQueryValues() throws {
        let (settings, defaults, domain) = preferences(); defer { defaults.removePersistentDomain(forName: domain) }
        settings.setProvider(.custom)
        try settings.configureCustom(endpoint: "http://127.0.0.1:8000/search?lang=zh&q=old&q=older", parameter: "q")
        XCTAssertEqual(try settings.resolve("a+b & c").url,"http://127.0.0.1:8000/search?lang=zh&q=a%2Bb%20%26%20c")
        try settings.configureCustom(endpoint: "http://127.0.0.1:8000/search?flag&lang=en+US&q=old", parameter: "q")
        XCTAssertEqual(try settings.resolve("a+b").url,"http://127.0.0.1:8000/search?flag&lang=en+US&q=a%2Bb")
        try settings.configureCustom(endpoint: settings.endpoint, parameter: "term")
        XCTAssertEqual(URLComponents(string: try settings.resolve("中文").url)?.queryItems?.last, URLQueryItem(name: "term", value: "中文"))
        for endpoint in ["file:///tmp/search", "https://alice:secret@example.test/search", "https://example.test/search#private", "https:///search"] {
            XCTAssertThrowsError(try settings.configureCustom(endpoint: endpoint, parameter: "q"),endpoint)
            XCTAssertTrue(try settings.resolve("private query").url.hasPrefix("http://127.0.0.1:8000/search?"),"An invalid edit must retain the last confirmed provider")
            XCTAssertEqual(try settings.resolve("about:credits").url,"about:credits","Broken search settings must not break ordinary navigation")
        }
    }
    func testSearchBoundsAndEncodedDuplicateFieldsRefuseOversizedOrInvalidRequests() throws {
        let exactLimit = String(repeating: "é",count: 1024)
        XCTAssertNoThrow(try BrowserSearchConfiguration().resolve(exactLimit))
        XCTAssertThrowsError(try BrowserSearchConfiguration().resolve(exactLimit + "é"))
        let prefix = "https://example.test/"
        let endpoint = prefix + String(repeating: "x",count: 2048 - prefix.utf8.count)
        let long = BrowserSearchConfiguration(provider: .custom,endpoint: endpoint)
        XCTAssertThrowsError(try long.resolve(exactLimit),"Endpoint and query must also fit the final URL limit")
        let encoded = BrowserSearchConfiguration(provider: .custom,endpoint: "https://example.test/search?%71=old&q=older&lang=en+US&flag")
        XCTAssertEqual(try encoded.resolve("a+b").url,"https://example.test/search?lang=en+US&flag&q=a%2Bb")
        let invalid = BrowserSearchConfiguration(provider: .custom,endpoint: "https://example.test/search",parameter: "q\nsecret")
        XCTAssertThrowsError(try invalid.resolve("private query"))
        let unknown = BrowserSearchConfiguration(provider: .unconfigured)
        XCTAssertThrowsError(try unknown.resolve("private query"))
        XCTAssertEqual(try unknown.resolve("https://example.test/"),BrowserAddressTarget(url: "https://example.test/",isSearch: false))
    }
    func testModelsShareTheWorkspaceChoiceAndReplacementReloadsConfirmedPreferences() throws {
        let (_, defaults, domain) = preferences(); defer { defaults.removePersistentDomain(forName: domain) }
        let appearance = BrowserAppearance(defaults: defaults,systemPreferences: { DisplayPreferences() },notificationCenter: NotificationCenter(),observeApplication: false)
        let workspace = BrowserWorkspace(appearance: appearance,contextDefaults: defaults)
        let first = try XCTUnwrap(workspace.models[1])
        let second = BrowserModel(appearance: appearance,workspace: workspace,windowID: 2)
        XCTAssertTrue(first.search === workspace.search); XCTAssertTrue(second.search === workspace.search)
        try workspace.search.configureCustom(endpoint: "http://127.0.0.1:8000/search",parameter: "term")
        XCTAssertEqual(try second.search.resolve("blueice").url,"http://127.0.0.1:8000/search?term=blueice")
        let replacement = BrowserWorkspace(appearance: appearance,contextDefaults: defaults)
        XCTAssertFalse(replacement.search === workspace.search)
        XCTAssertEqual(try XCTUnwrap(replacement.models[1]).search.resolve("blueice").url,"http://127.0.0.1:8000/search?term=blueice")
    }
    func testPreferenceRelaunchAndUnknownValuesNeverSilentlySwitchProviders() throws {
        let (settings, defaults, domain) = preferences(); defer { defaults.removePersistentDomain(forName: domain) }
        settings.setProvider(.bing)
        XCTAssertTrue(try BrowserSearchPreferences(defaults: defaults).resolve("blueice").url.hasPrefix("https://www.bing.com/search?q="))
        settings.setProvider(.google)
        XCTAssertTrue(try BrowserSearchPreferences(defaults: defaults).resolve("blueice").url.hasPrefix("https://www.google.com/search?q="))
        defaults.set("future-provider",forKey: BrowserSearchPreferences.configurationKey)
        let future = BrowserSearchPreferences(defaults: defaults)
        XCTAssertEqual(future.provider,.unconfigured); XCTAssertThrowsError(try future.resolve("private query"))
        future.setProvider(.duckDuckGo)
        XCTAssertTrue(try future.resolve("blueice").url.hasPrefix("https://duckduckgo.com/?q="))
        for version in [true, 1.5, 2] as [Any] {
            // Install the actual malformed type, rather than replacing an
            // equal numeric version in the preference cache.
            defaults.removeObject(forKey: BrowserSearchPreferences.configurationKey)
            defaults.set(["version": version,"provider": "duckduckgo","endpoint": "","parameter": "q"],forKey: BrowserSearchPreferences.configurationKey)
            let stored = try XCTUnwrap(defaults.dictionary(forKey: BrowserSearchPreferences.configurationKey)?["version"] as? NSNumber)
            if version is Bool { XCTAssertEqual(CFGetTypeID(stored),CFBooleanGetTypeID(),"The fixture must contain a real Boolean version") }
            XCTAssertEqual(BrowserSearchPreferences(defaults: defaults).provider,.unconfigured,"Unsupported version \(version), stored type \(type(of: stored)), CF type \(CFGetTypeID(stored)), bool type \(CFBooleanGetTypeID())")
        }
    }
}
