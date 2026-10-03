// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import Combine

@MainActor
protocol PermissionService: AnyObject {
    func request(_ request: PermissionRequest) async throws -> PermissionReply
    func listTabs() async throws -> [PermissionTab]
}
struct PermissionConfirmation: Identifiable {
    let snapshot: PermissionSnapshot
    let permission: OptionalPermission
    var id: String { permission.capability }
}
@MainActor
final class PermissionPanelModel: ObservableObject {
    @Published private(set) var snapshot: PermissionSnapshot?
    @Published private(set) var busy = false
    @Published private(set) var notice: String?
    @Published private(set) var tabs: [PermissionTab] = []
    @Published var selectedTab: UInt64?
    @Published private(set) var confirmation: PermissionConfirmation?
    @Published private(set) var review: OneShotReview?
    private let service: any PermissionService
    private var presentation: UInt64 = 0
    init(service: any PermissionService) { self.service = service }

    func refresh() async {
        guard !busy else { return }; busy = true; defer { busy = false }
        presentation &+= 1; confirmation = nil; review = nil; notice = nil
        do {
            switch try await service.request(.inspect) {
            case .state(let state): snapshot = state
            case .rejected(let reason): snapshot = nil; notice = reason; return
            default: throw PanelFailure.invalid("Unexpected permission inspection reply.")
            }
            tabs = try await service.listTabs()
            if !tabs.contains(where: { $0.id == selectedTab }) { selectedTab = tabs.first?.id }
        } catch { snapshot = nil; tabs = []; notice = error.localizedDescription }
    }
    // Only native button actions call these methods. Inspection/replies never
    // promote a permission into a grant or automatically confirm a review.
    func prepareChange(_ permission: OptionalPermission) {
        guard !busy, let snapshot, snapshot.installed?.optional.contains(permission) == true else { return }
        presentation &+= 1; review = nil; notice = nil
        confirmation = PermissionConfirmation(snapshot: snapshot, permission: permission)
    }
    func confirmChange() async {
        guard !busy, let confirmation, let installed = confirmation.snapshot.installed else { return }
        busy = true; defer { busy = false }
        self.confirmation = nil
        let grant = !confirmation.permission.granted
        do {
            let reply = try await service.request(.change(confirmation.snapshot.core_generation, installed.extension_id, confirmation.permission.capability, grant))
            switch reply {
            case .state(let state):
                guard state.core_generation == confirmation.snapshot.core_generation,
                      state.installed?.extension_id == installed.extension_id,
                      state.installed?.optional.contains(where: { $0.capability == confirmation.permission.capability && $0.granted == grant }) == true else {
                    throw PanelFailure.invalid("The permission change was not confirmed. Refresh before trying again.")
                }
                snapshot = state; notice = grant ? "Permission allowed." : "Permission revoked."
            case .rejected(let reason): snapshot = nil; notice = reason
            default: throw PanelFailure.invalid("Unexpected permission change reply.")
            }
        } catch { snapshot = nil; notice = error.localizedDescription }
    }
    func reviewOneShot() async {
        guard !busy, let snapshot, let installed = snapshot.installed,
              installed.runtime_ephemeral.contains(where: { $0.capability == "dom:read" }),
              let tab = selectedTab, tabs.contains(where: { $0.id == tab }) else { return }
        busy = true; defer { busy = false }
        presentation &+= 1; let epoch = presentation; confirmation = nil; review = nil; notice = nil
        do {
            switch try await service.request(.review(snapshot.core_generation, installed.extension_id, tab)) {
            case .review(let value):
                guard value.core_generation == snapshot.core_generation, value.installed.extension_id == installed.extension_id, value.tab_id == tab else {
                    throw PanelFailure.invalid("The reviewed permission target changed. Refresh before trying again.")
                }
                if presentation == epoch { review = value }
            case .rejected(let reason): notice = reason
            default: throw PanelFailure.invalid("Unexpected document review reply.")
            }
        } catch { notice = error.localizedDescription }
    }
    func confirmOneShot() async {
        guard !busy, let review else { return }
        busy = true; defer { busy = false }; self.review = nil
        do {
            switch try await service.request(.arm(review)) {
            case .armed(let armed):
                guard armed.core_generation == review.core_generation, armed.installed.extension_id == review.installed.extension_id,
                      armed.capability == review.capability, armed.tab_id == review.tab_id, armed.document_epoch == review.document_epoch else {
                    throw PanelFailure.invalid("The one-shot permission was not confirmed.")
                }
                notice = "One DOM read allowed for the reviewed document. This is not a persistent permission."
            case .rejected(let reason): notice = reason
            default: throw PanelFailure.invalid("Unexpected one-shot acknowledgement.")
            }
        } catch { notice = error.localizedDescription }
    }
    func cancel() { presentation &+= 1; confirmation = nil; review = nil }
}
