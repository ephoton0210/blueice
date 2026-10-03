// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import XCTest

@MainActor
final class PermissionPanelTests: XCTestCase {
    private func reply(_ text: String) throws -> PermissionReply { try JSONDecoder().decode(PermissionReply.self, from: Data(text.utf8)) }
    private let package = #"{"extension_id":"sha256:package","name":"Notes","version":"1","optional":[{"capability":"storage","granted":false,"origins":[]}],"runtime_ephemeral":[{"capability":"dom:read","origins":[]}]}"#
    private func state(_ granted: Bool = false, generation: UInt64 = 4) throws -> PermissionReply {
        try reply("{\"state\":{\"core_generation\":\(generation),\"installed\":\(package.replacingOccurrences(of: "false", with: String(granted)))}}")
    }
    private func reviewReply(epoch: UInt64 = 12) throws -> PermissionReply {
        try reply("{\"ephemeral_review\":{\"core_generation\":4,\"installed\":\(package),\"capability\":\"dom:read\",\"tab_id\":7,\"document_epoch\":\(epoch),\"url\":\"https://example.test/page\"}}")
    }
    func testInspectAndCancelNeverSendAGrantAndConfirmationWaitsForCoreState() async throws {
        let service = MockPermissionService([try state(), try state(true), try state(false)])
        let model = PermissionPanelModel(service: service)
        await model.refresh(); XCTAssertEqual(service.requests.count, 1)
        let permission = try XCTUnwrap(model.snapshot?.installed?.optional.first)
        model.prepareChange(permission); model.cancel(); await model.confirmChange()
        XCTAssertEqual(service.requests.count, 1); XCTAssertFalse(try XCTUnwrap(model.snapshot?.installed?.optional.first).granted)
        model.prepareChange(permission); await model.confirmChange()
        XCTAssertTrue(try XCTUnwrap(model.snapshot?.installed?.optional.first).granted)
        let request = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(service.requests[1])) as? [String: [String: Any]])
        XCTAssertEqual(request["change"]?["expected_core_generation"] as? Int, 4)
        XCTAssertEqual(request["change"]?["expected_extension_id"] as? String, "sha256:package")
        XCTAssertEqual(request["change"]?["action"] as? String, "grant")
        model.prepareChange(try XCTUnwrap(model.snapshot?.installed?.optional.first)); await model.confirmChange()
        XCTAssertFalse(try XCTUnwrap(model.snapshot?.installed?.optional.first).granted)
    }
    func testRejectedOrWrongGenerationAcknowledgementsDoNotInferSuccess() async throws {
        for result in [PermissionReply.rejected("the core changed"), try state(true, generation: 5)] {
            let service = MockPermissionService([try state(), result]); let model = PermissionPanelModel(service: service)
            await model.refresh(); model.prepareChange(try XCTUnwrap(model.snapshot?.installed?.optional.first)); await model.confirmChange()
            XCTAssertNil(model.snapshot); XCTAssertNotNil(model.notice); XCTAssertFalse(model.busy)
        }
    }
    func testOneShotRequiresSeparateReviewAndConfirmationAndRejectsStaleDocument() async throws {
        let service = MockPermissionService([try state(), try reviewReply(), .rejected("the reviewed HTTP(S) document changed before confirmation")])
        let model = PermissionPanelModel(service: service)
        await model.refresh(); await model.confirmOneShot(); XCTAssertEqual(service.requests.count, 1)
        await model.reviewOneShot(); XCTAssertNotNil(model.review); XCTAssertEqual(service.requests.count, 2)
        await model.confirmOneShot(); XCTAssertNil(model.review); XCTAssertTrue(model.notice?.contains("changed") == true)
        let text = String(decoding: try JSONEncoder().encode(service.requests[2]), as: UTF8.self)
        XCTAssertTrue(text.contains("\"document_epoch\":12")); XCTAssertFalse(text.contains("ticket"))
        await model.confirmOneShot(); XCTAssertEqual(service.requests.count, 3)
    }
    func testCancelReviewDiscardsThePendingConfirmation() async throws {
        let service = MockPermissionService([try state(), try reviewReply()]); let model = PermissionPanelModel(service: service)
        await model.refresh(); await model.reviewOneShot(); model.cancel(); await model.confirmOneShot()
        XCTAssertEqual(service.requests.count, 2); XCTAssertNil(model.review)
    }
    func testArmedAcknowledgementMustMatchTheReviewedDocument() async throws {
        let wrong = try reply("{\"ephemeral_armed\":{\"core_generation\":4,\"installed\":\(package),\"capability\":\"dom:read\",\"tab_id\":7,\"document_epoch\":13}}")
        let service = MockPermissionService([try state(), try reviewReply(), wrong]); let model = PermissionPanelModel(service: service)
        await model.refresh(); await model.reviewOneShot(); await model.confirmOneShot()
        XCTAssertEqual(model.notice, "The one-shot permission was not confirmed.")
        XCTAssertNil(model.review)
        await model.confirmOneShot(); XCTAssertEqual(service.requests.count, 3)
    }
    func testFramingAndMalformedPermissionStateFailClosed() throws {
        let frame = try PermissionFraming.frame(PermissionRequest.inspect)
        XCTAssertEqual(try PermissionFraming.length(Data(frame.prefix(4))), 9)
        XCTAssertEqual(String(decoding: frame.dropFirst(4), as: UTF8.self), "\"inspect\"")
        for prefix in [Data(), Data([0,0,0,0]), Data([1,0,2,0])] { XCTAssertThrowsError(try PermissionFraming.length(prefix)) }
        for text in [#"{"state":{},"rejected":{"reason":"x"}}"#, #"{"unknown":{}}"#,
                     "{\"state\":{\"core_generation\":4,\"installed\":\(package.replacingOccurrences(of: "\"storage\"", with: "\"\""))}}"] {
            XCTAssertThrowsError(try reply(text))
        }
    }
}
@MainActor
private final class MockPermissionService: PermissionService {
    private var replies: [PermissionReply]
    private(set) var requests: [PermissionRequest] = []
    init(_ replies: [PermissionReply]) { self.replies = replies }
    func request(_ request: PermissionRequest) async throws -> PermissionReply {
        requests.append(request)
        guard !replies.isEmpty else { throw PanelFailure.invalid("No mock reply") }
        return replies.removeFirst()
    }
    func listTabs() async throws -> [PermissionTab] { [.init(id: 7, url: "https://example.test/page")] }
}
