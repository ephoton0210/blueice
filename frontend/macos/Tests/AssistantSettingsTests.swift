// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import XCTest

@MainActor
final class AssistantSettingsTests: XCTestCase {
    private let initial = NativeAssistantSettings.defaults
    private func proposal(_ value: NativeAssistantSettings, id: UInt64 = 7) -> NativeAssistantProposal {
        .init(id: id, digest: String(repeating: "a", count: 64), diff: ["Idle timeout: 600 -> 630"], proposed: value, seconds_left: 300)
    }
    func testInspectionDraftAndCancelNeverApplySettings() async throws {
        let service = MockAssistantSettingsService([.assistant(.init(current: initial, pending: nil))])
        let model = AssistantSettingsModel(service: service)
        await model.refresh(); model.draft.idle = "630"; model.prepareEdit()
        XCTAssertNotNil(model.confirmation); XCTAssertEqual(model.state?.current, initial)
        XCTAssertEqual(service.requests.count, 1)
        model.cancel(); await model.confirm()
        XCTAssertEqual(service.requests.count, 1); XCTAssertNil(model.confirmation)
    }
    func testDirectEditRequiresMatchingAcknowledgementAndConsumesConfirmation() async throws {
        var changed = initial; changed.idle_timeout_secs = 630
        for reply in [PermissionReply.assistant(.init(current: changed, pending: nil)), .assistant(.init(current: initial, pending: nil)), .rejected("write failed")] {
            let service = MockAssistantSettingsService([.assistant(.init(current: initial, pending: nil)), reply])
            let model = AssistantSettingsModel(service: service)
            await model.refresh(); model.draft.idle = "630"; model.prepareEdit(); await model.confirm()
            XCTAssertNil(model.confirmation); XCTAssertFalse(model.busy)
            let request = try JSONSerialization.jsonObject(with: JSONEncoder().encode(service.requests[1])) as! [String: [String: Any]]
            XCTAssertNotNil(request["edit_assistant_settings"]?["settings"])
            if case .assistant(let state) = reply, state.current == changed { XCTAssertEqual(model.state?.current, changed) }
            else { XCTAssertNil(model.state); XCTAssertNotNil(model.notice) }
            await model.confirm(); XCTAssertEqual(service.requests.count, 2)
        }
    }
    func testProposalRefreshCancelsApprovalAndConfirmedRequestNamesExactIDAndDigest() async throws {
        var changed = initial; changed.nice = 11
        let pending = proposal(changed)
        let service = MockAssistantSettingsService([.assistant(.init(current: initial, pending: pending)), .assistant(.init(current: initial, pending: nil)), .assistant(.init(current: initial, pending: pending)), .assistant(.init(current: changed, pending: nil))])
        let model = AssistantSettingsModel(service: service)
        await model.refresh(); model.prepareApproval(); XCTAssertNotNil(model.confirmation)
        await model.refresh(); await model.confirm(); XCTAssertEqual(service.requests.count, 2)
        await model.refresh(); model.prepareApproval(); await model.confirm()
        let request = try JSONSerialization.jsonObject(with: JSONEncoder().encode(service.requests[3])) as! [String: [String: Any]]
        XCTAssertEqual(request["approve_assistant_proposal"]?["id"] as? Int, 7)
        XCTAssertEqual(request["approve_assistant_proposal"]?["digest"] as? String, pending.digest)
        XCTAssertEqual(model.state?.current, changed)
    }
    func testDeniedProposalDoesNotChangeCurrentSettingsAndRejectsInconsistentReply() async throws {
        var changed = initial; changed.nice = 11
        for after in [NativeAssistantState(current: initial, pending: nil), .init(current: changed, pending: nil)] {
            let service = MockAssistantSettingsService([.assistant(.init(current: initial, pending: proposal(changed))), .assistant(after)])
            let model = AssistantSettingsModel(service: service)
            await model.refresh(); await model.deny()
            if after.current == initial { XCTAssertEqual(model.state?.current, initial) } else { XCTAssertNil(model.state) }
        }
    }
    func testInvalidDraftAndHostileMetadataCannotBecomeConsentChrome() throws {
        var draft = AssistantSettingsDraft(initial)
        for value in ["-1", "29", "86401", "18446744073709551616"] { draft.idle = value; XCTAssertThrowsError(try draft.settings()) }
        draft.idle = "630"; draft.backend = "loopback"; draft.baseURL = "https://example.com"; draft.model = "local"
        XCTAssertThrowsError(try draft.settings())
        draft.baseURL = "http://127.0.0.1:11434/v1/"; XCTAssertEqual(try draft.settings().loopback?.model, "local")
        XCTAssertEqual(AssistantConsentText.display("safe\u{202E}evil\nallow"), "safe[U+202E]evil[U+000A]allow")
        XCTAssertFalse(proposal(initial, id: 0).valid)
        let invalid = #"{"assistant_settings_state":{"current":{"version":2,"backend":"none","idle_timeout_secs":600,"nice":10},"pending":null}}"#
        XCTAssertThrowsError(try JSONDecoder().decode(PermissionReply.self, from: Data(invalid.utf8)))
    }
}

@MainActor
private final class MockAssistantSettingsService: PermissionService {
    var replies: [PermissionReply]
    var requests: [PermissionRequest] = []
    init(_ replies: [PermissionReply]) { self.replies = replies }
    func request(_ request: PermissionRequest) async throws -> PermissionReply {
        requests.append(request)
        guard !replies.isEmpty else { throw PanelFailure.invalid("No fixture reply") }
        return replies.removeFirst()
    }
    func listTabs() async throws -> [PermissionTab] { [] }
}
