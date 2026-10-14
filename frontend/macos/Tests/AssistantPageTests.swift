// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import XCTest

@MainActor
final class AssistantPageTests: XCTestCase {
    let document = AssistantDocument(tab_id: 1, frame_source: 44, document_generation: 3)
    func testOnlyExactRequestedDocumentKindAndIDCanPresentPlainText() throws {
        for mismatch in 0..<4 {
            let assistant = BrowserAssistant(); assistant.observe(document, url: "https://example.test/page")
            let operation = try XCTUnwrap(assistant.begin(1, task: .summary)); operation.requestID = 9
            let wrong = AssistantDocument(tab_id: 1, frame_source: 44, document_generation: 4)
            let reply = AssistantPageResult(context: mismatch == 0 ? wrong : document, kind: mismatch == 1 ? .organized : .summary, text: "<script>literal</script> **plain**")
            _ = assistant.receive(IncomingEnvelope(requestID: mismatch == 2 ? 10 : 9, tabID: mismatch == 3 ? 2 : 1, message: .assistantResult(reply)))
            XCTAssertNil(assistant.page(1)?.result)
        }
        let assistant = BrowserAssistant(); assistant.observe(document, url: "https://example.test/page")
        let operation = try XCTUnwrap(assistant.begin(1, task: .summary)); operation.requestID = 9
        XCTAssertTrue(assistant.receive(.init(requestID: 9, tabID: 1, message: .assistantResult(.init(context: document, kind: .summary, text: "<script>literal</script> **plain**")))))
        XCTAssertEqual(assistant.page(1)?.result?.text, "<script>literal</script> **plain**")
    }
    func testNavigationStopWaitingTimeoutAndTabCloseDiscardLateResults() throws {
        for action in 0..<4 {
            let assistant = BrowserAssistant(); assistant.observe(document, url: "https://example.test/page")
            let operation = try XCTUnwrap(assistant.begin(1, task: .summary)); operation.requestID = 9
            switch action { case 0: assistant.invalidate(1, clear: true); case 1: assistant.stopWaiting(1); case 2: assistant.expire(operation); default: assistant.retainTabs([]) }
            _ = assistant.receive(.init(requestID: 9, tabID: 1, message: .assistantResult(.init(context: document, kind: .summary, text: "late"))))
            XCTAssertNil(assistant.page(1)?.result)
        }
    }
    func testMovingAnInFlightTaskPreservesReplyOwnership() throws {
        let source = BrowserAssistant(); let destination = BrowserAssistant()
        source.observe(document, url: "https://example.test/page")
        let operation = try XCTUnwrap(source.begin(1, task: .organized("table")))
        source.transfer(1, to: destination); operation.requestID = 21
        let envelope = IncomingEnvelope(requestID: 21, tabID: 1, message: .assistantResult(.init(context: document, kind: .organized, text: "moved result")))
        XCTAssertFalse(source.receive(envelope)); XCTAssertTrue(destination.receive(envelope))
        XCTAssertEqual(destination.page(1)?.result?.text, "moved result")
        XCTAssertTrue(operation.owner === destination)
    }
    func testTranslationAcknowledgementsDoNotInferOptimisticChanges() throws {
        let assistant = BrowserAssistant(); assistant.observe(document, url: "https://example.test/page")
        let operation = try XCTUnwrap(assistant.translate(1, action: .language("zh-TW"))); operation.requestID = 4
        XCTAssertNil(assistant.page(1)?.translation)
        _ = assistant.receive(.init(requestID: 4, tabID: 1, message: .translation(.init(language: "ja", available: false, shown: false))))
        XCTAssertNil(assistant.page(1)?.translation); XCTAssertNotNil(assistant.page(1)?.notice)
        let valid = try XCTUnwrap(assistant.translate(1, action: .inspect)); valid.requestID = 5
        _ = assistant.receive(.init(requestID: 5, tabID: 1, message: .translation(.init(language: "zh-TW", available: true, shown: true))))
        XCTAssertTrue(assistant.page(1)?.translation?.shown == true)
    }
    func testWireBoundsLanguageAndDocumentShapeAreValidated() throws {
        XCTAssertTrue(TranslationState.validTag("zh-TW")); XCTAssertFalse(TranslationState.validTag("ignore instructions"))
        XCTAssertFalse(TranslationState(language: nil, available: false, shown: true).valid)
        let data = try BrowserWire.encode(.assistantPage(document, .organize("table")), tab: 1, request: 19)
        let root = try JSONSerialization.jsonObject(with: data.dropFirst(4)) as! [String: Any]
        XCTAssertNotNil((root["message"] as? [String: Any])?["AssistantPage"])
        XCTAssertFalse(AssistantPageResult(context: document, kind: .summary, text: String(repeating: "a", count: 16385)).valid)
    }
    func testInspectionPreservesTaskButShownTextChangeInvalidatesItsResult() throws {
        let assistant = BrowserAssistant(); assistant.observe(document, url: "https://example.test/page")
        let task = try XCTUnwrap(assistant.begin(1, task: .summary)); task.requestID = 8
        let inspection = try XCTUnwrap(assistant.translate(1, action: .inspect)); inspection.requestID = 9
        _ = assistant.receive(.init(requestID: 9, tabID: 1, message: .translation(.init(language: "zh-TW", available: true, shown: true))))
        XCTAssertTrue(assistant.page(1)?.pending === task)
        _ = assistant.receive(.init(requestID: 8, tabID: 1, message: .assistantResult(.init(context: document, kind: .summary, text: "translated result"))))
        XCTAssertEqual(assistant.page(1)?.result?.text, "translated result")
        let original = try XCTUnwrap(assistant.translate(1, action: .show(false))); original.requestID = 10
        _ = assistant.receive(.init(requestID: 10, tabID: 1, message: .translation(.init(language: "zh-TW", available: true, shown: false))))
        XCTAssertNil(assistant.page(1)?.result)
        XCTAssertFalse(assistant.page(1)?.translation?.shown ?? true)
    }
}
