// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
import XCTest

@MainActor
final class SessionRestoreTests: XCTestCase {
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
