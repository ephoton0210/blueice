// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import Combine

enum AssistantSettingsConfirmation {
    case edit(NativeAssistantSettings)
    case approve(NativeAssistantProposal)
    var settings: NativeAssistantSettings {
        switch self { case .edit(let value): return value; case .approve(let value): return value.proposed }
    }
}
@MainActor
final class AssistantSettingsModel: ObservableObject {
    @Published private(set) var state: NativeAssistantState?
    @Published private(set) var busy = false
    @Published private(set) var notice: String?
    @Published private(set) var confirmation: AssistantSettingsConfirmation?
    @Published var draft = AssistantSettingsDraft(.defaults)
    private let service: any PermissionService
    private var presentation: UInt64 = 0
    init(service: any PermissionService) { self.service = service }
    func refresh() async {
        guard !busy else { return }
        cancel(); let epoch = presentation; busy = true; notice = nil
        defer { busy = false }
        do {
            let value = try checked(try await service.request(.inspectAssistant))
            // Closing or switching panels fences late inspection state.
            guard epoch == presentation else { return }
            state = value; draft = AssistantSettingsDraft(value.current)
        } catch { state = nil; notice = error.localizedDescription }
    }
    func prepareEdit() {
        guard !busy, let state else { return }
        cancel(); notice = nil
        do {
            let settings = try draft.settings()
            guard settings != state.current else { notice = "No settings changed."; return }
            confirmation = .edit(settings)
        } catch { notice = error.localizedDescription }
    }
    func prepareApproval() {
        guard !busy, let pending = state?.pending, pending.seconds_left > 0 else { return }
        cancel(); notice = nil; confirmation = .approve(pending)
    }
    func confirm() async {
        guard !busy, let confirmation, let previous = state else { return }
        cancel(); busy = true; defer { busy = false }; notice = nil
        do {
            let request: PermissionRequest
            switch confirmation {
            case .edit(let settings): request = .editAssistant(settings)
            case .approve(let proposal):
                guard previous.pending == proposal else { throw PanelFailure.invalid("The proposal changed. Refresh and review it again.") }
                request = .approveAssistant(proposal.id, proposal.digest)
            }
            let value = try checked(try await service.request(request))
            guard value.current == confirmation.settings, value.pending == nil else {
                throw PanelFailure.invalid("The settings change was not confirmed. Refresh to inspect what is in force.")
            }
            state = value; draft = AssistantSettingsDraft(value.current); notice = "Assistant settings applied."
        } catch { state = nil; notice = error.localizedDescription }
    }
    func deny() async {
        guard !busy, let previous = state, let pending = previous.pending else { return }
        cancel(); busy = true; defer { busy = false }; notice = nil
        do {
            let value = try checked(try await service.request(.denyAssistant(pending.id)))
            guard value.current == previous.current, value.pending == nil else {
                throw PanelFailure.invalid("The proposal denial was not confirmed. Refresh to inspect the current settings.")
            }
            state = value; notice = "Proposal denied. Settings unchanged."
        } catch { state = nil; notice = error.localizedDescription }
    }
    func cancel() { presentation &+= 1; confirmation = nil }
    private func checked(_ reply: PermissionReply) throws -> NativeAssistantState {
        switch reply {
        case .assistant(let state):
            guard state.valid else { throw PanelFailure.invalid("Invalid assistant settings state.") }; return state
        case .rejected(let reason): throw PanelFailure.invalid(reason)
        default: throw PanelFailure.invalid("Unexpected assistant settings reply.")
        }
    }
}
