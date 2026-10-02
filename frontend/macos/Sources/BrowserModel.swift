// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine

@MainActor
final class BrowserModel: ObservableObject {
    @Published private(set) var tabs: [BrowserTab] = []
    @Published private(set) var selected: UInt64?
    @Published var address = ""
    @Published private(set) var status = "Starting BlueIce…"
    @Published private(set) var resubmission: FormResubmission?
    @Published var resubmissionPresented = false
    private var resubmissions: [UInt64: FormResubmission] = [:]
    @Published private(set) var ready = false
    @Published private(set) var image: CGImage?
    @Published private(set) var generation: UInt64 = 0
    @Published private(set) var history = HistoryState()
    @Published private(set) var representation: PageRepresentation?
    @Published private(set) var accessibilityEpoch: UInt64 = 0
    @Published private(set) var textInputState: TextInputState?
    @Published private(set) var textInputBusy = false
    @Published private(set) var pageFocusSerial: UInt64 = 0
    @Published private(set) var addressFocusSerial: UInt64 = 0
    func requestAddressFocus() { addressFocusSerial &+= 1 }
    private struct InputRequest {
        let epoch: UInt64
        let serial: UInt64
        let generation: UInt64
        var request: UInt64?
    }
    private struct InputEdit {
        let tab: UInt64
        let epoch: UInt64
        let serial: UInt64
        let action: TextInputAction
    }
    private var inputStates: [UInt64: TextInputState] = [:]
    private var inputSerials: [UInt64: UInt64] = [:]
    private var inputFocusPending: Set<UInt64> = []
    private var inputRequests: [UInt64: InputRequest] = [:]
    private var inputQueue: [InputEdit] = []
    private var inputFlight: (edit: InputEdit, request: UInt64?)?
    private var earlyInputReplies: [IncomingEnvelope] = []
    private var inputTimeout: Task<Void, Never>?
    private var nativeInputFailure: (tab: UInt64, epoch: UInt64, focus: UInt64?, message: String)?
    private var representations: [UInt64: PageRepresentation] = [:]
    private var documentEpochs: [UInt64: UInt64] = [:]
    private var representationRequests: [UInt64: (epoch: UInt64, generation: UInt64)] = [:]
    private let session = BrowserSession()
    private var frames: [UInt64: FramePixels] = [:]
    private var histories: [UInt64: HistoryState] = [:]
    private var viewport = CGSize(width: 1024, height: 640)
    private var resizeTask: Task<Void, Never>?

    func start(launcher: URL? = nil) async {
        let arguments = ProcessInfo.processInfo.arguments
        let executable: URL
        let supervised: Bool
        if let launcher {
            executable = launcher
            supervised = true
        } else if let option = arguments.firstIndex(of: "--core-exe") {
            guard arguments.indices.contains(option + 1) else { status = "--core-exe requires a core executable path."; return }
            executable = URL(fileURLWithPath: arguments[option + 1])
            supervised = false
        } else {
            if let option = arguments.firstIndex(of: "--launcher-exe") {
                guard arguments.indices.contains(option + 1) else { status = "--launcher-exe requires a launcher executable path."; return }
                executable = URL(fileURLWithPath: arguments[option + 1])
            } else { executable = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/blueice-launcher") }
            supervised = true
        }
        session.received = { [weak self, directory = session.frameDirectory] result in
            do {
                let envelope = try result.get()
                var pixels: FramePixels?
                if case .frame(let notice) = envelope.message {
                    do { pixels = try FramePixels.read(notice, directory: directory) }
                    catch let error as NSError where error.domain == NSCocoaErrorDomain && error.code == NSFileReadNoSuchFileError { return }
                }
                let frame = pixels
                Task { @MainActor [weak self] in self?.apply(envelope, pixels: frame) }
            } catch {
                let message = error.localizedDescription
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    self.status = message; self.ready = false; self.representation = nil; self.clearTextInput()
                    await self.session.stop()
                }
            }
        }
        do {
            if supervised { try await session.start(launcher: executable) }
            else { try await session.start(executable: executable) }
            ready = true
            status = "Ready"
            await send(.unit("ListTabs"))
            await send(.values("Navigate", ["url": .string("about:credits")]), tab: 1)
        } catch { status = error.localizedDescription; await session.stop() }
    }

    private func apply(_ envelope: IncomingEnvelope, pixels: FramePixels?) {
        let tab = envelope.tabID
        switch envelope.message {
        case .tabs(let list):
            tabs = list
            let live = Set(list.map(\.id))
            resubmissions = resubmissions.filter { live.contains($0.key) }
            if selected.map({ !live.contains($0) }) == true { resubmission = nil; resubmissionPresented = false }
            frames = frames.filter { live.contains($0.key) }
            histories = histories.filter { live.contains($0.key) }
            representations = representations.filter { live.contains($0.key) }
            documentEpochs = documentEpochs.filter { live.contains($0.key) }
            representationRequests = representationRequests.filter { live.contains($0.key) }
            inputStates = inputStates.filter { live.contains($0.key) }
            inputRequests = inputRequests.filter { live.contains($0.key) }
            inputQueue.removeAll { !live.contains($0.tab) }
            if selected == nil || !live.contains(selected!) { selected = list.first?.id }
            showSelected()
        case .opened(let id):
            selected = id
            Task { await send(.unit("ListTabs")); await navigate("about:credits", tab: id) }
        case .closed:
            Task { await send(.unit("ListTabs")) }
        case .navigationStarted:
            if tab == selected { status = "Loading…" }
        case .navigated(let url):
            if let tab { resubmissions.removeValue(forKey: tab) }
            if tab == selected { resubmission = nil; resubmissionPresented = false }
            if let tab {
                documentEpochs[tab, default: 0] += 1
                representations.removeValue(forKey: tab)
                inputStates.removeValue(forKey: tab)
                inputSerials[tab, default: 0] += 1
                inputQueue.removeAll { $0.tab == tab }
                if tab == selected { textInputState = nil }
                if tab == selected { representation = nil; accessibilityEpoch = documentEpochs[tab] ?? 0 }
            }
            if let id = tab, let index = tabs.firstIndex(where: { $0.id == id }) { tabs[index].url = url }
            if tab == selected { address = url; status = "Ready" }
            Task { await send(.unit("GetHistoryState"), tab: tab) }
        case .history(let value):
            if let tab { histories[tab] = value }
            if tab == selected { history = value }
        case .frame:
            guard let tab, let pixels, pixels.generation > (frames[tab]?.generation ?? 0) else { return }
            frames[tab] = pixels
            representations.removeValue(forKey: tab)
            if tab == selected { showFrame(pixels); representation = nil }
            requestRepresentation(tab)
            requestTextInput(tab)
        case .representation(let snapshot):
            guard let tab, snapshot.tabID == tab, envelope.requestID != nil,
                  let requested = representationRequests.removeValue(forKey: tab),
                  tabs.contains(where: { $0.id == tab }), let frame = frames[tab] else { return }
            if requested.epoch == documentEpochs[tab, default: 0],
               snapshot.matches(tab: tab, generation: frame.generation,
                                source: PageRepresentation.frameSource(directory: session.frameDirectory.path),
                                url: tabs.first(where: { $0.id == tab })?.url) {
                representations[tab] = snapshot
                if tab == selected { representation = snapshot }
            } else if requested.epoch != documentEpochs[tab, default: 0] || requested.generation != frame.generation {
                // Retry only when a newer document/frame superseded the in-flight request.
                requestRepresentation(tab)
            }
        case .representationUnavailable:
            if let tab { representationRequests.removeValue(forKey: tab); representations.removeValue(forKey: tab) }
            if tab == selected { representation = nil; status = "Page accessibility unavailable" }
        case .formResubmission(let prompt):
            guard let tab, tabs.contains(where: { $0.id == tab }) else { return }
            resubmissions[tab] = prompt
            if tab == selected { resubmission = prompt; resubmissionPresented = true; status = "Waiting for form resubmission confirmation" }
        case .formResubmissionResolved(let id):
            if let tab, resubmissions[tab]?.confirmationID == id { resubmissions.removeValue(forKey: tab) }
            if resubmission?.confirmationID == id { resubmission = nil; resubmissionPresented = false }
        case .blocked(let reason):
            if tab == selected { status = "Navigation blocked: " + reason }
        case .error(let message):
            if tab == nil || tab == selected { status = message }
            if let request = envelope.requestID, request == inputFlight?.request {
                applyTextInput(envelope)
            } else if inputFlight != nil && inputFlight?.request == nil {
                if earlyInputReplies.count < 32 { earlyInputReplies.append(envelope) }
            }
        case .textInputState, .textInputUnavailable:
            applyTextInput(envelope)
        default: break
        }
    }

    private func showFrame(_ frame: FramePixels?) {
        do { image = try frame?.image(); generation = frame?.generation ?? 0 }
        catch { status = error.localizedDescription }
    }

    private func showSelected() {
        address = tabs.first { $0.id == selected }?.url ?? ""
        history = selected.flatMap { histories[$0] } ?? HistoryState()
        showFrame(selected.flatMap { frames[$0] })
        representation = selected.flatMap { representations[$0] }
        accessibilityEpoch = selected.flatMap { documentEpochs[$0] } ?? 0
        textInputState = selected.flatMap { inputStates[$0] }
    }

    func select(_ tab: UInt64) {
        if let previous = selected, inputStates[previous]?.focused?.marked != nil {
            textInput(.finishComposition)
        }
        selected = tab
        resubmission = resubmissions[tab]; resubmissionPresented = resubmission != nil
        showSelected()
        Task { await send(.unit("GetHistoryState"), tab: tab); await resize() }
    }

    func action(_ command: BrowserCommand, tab: UInt64? = nil) {
        let target = tab ?? selected
        Task { await send(command, tab: target) }
    }

    private func requestRepresentation(_ tab: UInt64) {
        guard ready, representationRequests[tab] == nil, let frame = frames[tab] else { return }
        representationRequests[tab] = (documentEpochs[tab, default: 0], frame.generation)
        Task { await send(.unit("GetRepresentation"), tab: tab) }
    }

    func accessibilityAction(_ snapshot: PageRepresentation, epoch: UInt64, node: PageNode) -> Bool {
        guard ready, selected == snapshot.tabID, documentEpochs[snapshot.tabID, default: 0] == epoch,
              let frame = frames[snapshot.tabID], representation?.generation == snapshot.generation,
              snapshot.matches(tab: snapshot.tabID, generation: frame.generation,
                               source: PageRepresentation.frameSource(directory: session.frameDirectory.path),
                               url: tabs.first(where: { $0.id == snapshot.tabID })?.url),
              !node.state.disabled, !node.occluded else { return false }
        let visible = CGRect(x: node.bounds.x, y: node.bounds.y - snapshot.scrollY,
                             width: node.bounds.width, height: node.bounds.height)
            .intersection(CGRect(x: 0, y: 0, width: frame.width, height: frame.height))
        guard !visible.isEmpty else { return false }
        // Use the same hit-test/default-action pipeline as a physical page click.
        focusPage(x: visible.midX, y: visible.midY)
        return true
    }

    // Serialize edits through acknowledgements. An IME can issue several
    // callbacks in one event; none may use a different document or focus.
    func textInput(_ action: TextInputAction) {
        guard ready, let tab = selected, inputQueue.count < 128 else { return }
        inputQueue.append(InputEdit(tab: tab, epoch: documentEpochs[tab, default: 0],
                                    serial: inputSerials[tab, default: 0], action: action))
        textInputBusy = true
        drainTextInput()
    }

    func focusPage(x: Double, y: Double, extend: Bool = false, clickCount: Int = 1) {
        guard ready, let tab = selected else { return }
        inputSerials[tab, default: 0] += 1
        inputFocusPending.insert(tab)
        inputStates.removeValue(forKey: tab); textInputState = nil
        inputQueue.removeAll { $0.tab == tab }
        textInput(.pointer(x, y, extend, UInt8(min(3, max(1, clickCount)))))
        Task {
            await send(.values("Click", ["x": .number(x), "y": .number(y)]), tab: tab)
            inputFocusPending.remove(tab)
            requestTextInput(tab)
        }
    }

    private func requestTextInput(_ tab: UInt64?) {
        guard let tab, ready, inputRequests[tab] == nil, inputFlight == nil,
              !inputFocusPending.contains(tab),
              let frame = frames[tab], tabs.contains(where: { $0.id == tab }) else { return }
        inputRequests[tab] = InputRequest(epoch: documentEpochs[tab, default: 0],
                                         serial: inputSerials[tab, default: 0], generation: frame.generation)
        Task {
            let request = await send(.unit("GetTextInputState"), tab: tab)
            inputRequests[tab]?.request = request
            replayEarlyInputReplies()
        }
    }

    private func drainTextInput() {
        guard ready, inputFlight == nil else { return }
        while let edit = inputQueue.first {
            guard edit.epoch == documentEpochs[edit.tab, default: 0], edit.serial == inputSerials[edit.tab, default: 0],
                  tabs.contains(where: { $0.id == edit.tab }) else { inputQueue.removeFirst(); continue }
            guard inputRequests[edit.tab] == nil, let state = inputStates[edit.tab] else { requestTextInput(edit.tab); return }
            guard state.focused != nil || edit.action.isPageKey else { inputQueue.removeFirst(); continue }
            inputQueue.removeFirst()
            inputFlight = (edit, nil)
            inputTimeout?.cancel()
            inputTimeout = Task {
                try? await Task.sleep(for: .seconds(15))
                if !Task.isCancelled { finishInputFlight(); inputQueue.removeAll(); status = "Text input did not respond" }
            }
            Task {
                let request = await send(.textInput(state.context, edit.action), tab: edit.tab)
                inputFlight?.request = request
                replayEarlyInputReplies()
            }
            return
        }
        textInputBusy = false
    }

    private func finishInputFlight() {
        inputFlight = nil; inputTimeout?.cancel(); inputTimeout = nil
        textInputBusy = !inputQueue.isEmpty
    }

    private func replayEarlyInputReplies() {
        let replies = earlyInputReplies; earlyInputReplies.removeAll()
        for reply in replies { applyTextInput(reply) }
    }

    private func applyTextInput(_ envelope: IncomingEnvelope) {
        guard let tab = envelope.tabID, let request = envelope.requestID else { return }
        let query = inputRequests[tab]
        let flight = inputFlight
        let isEdit = flight?.request == request && flight?.edit.tab == tab
        let isQuery = query?.request == request
        guard isEdit || isQuery else {
            if flight?.request == nil && flight != nil || query != nil && query?.request == nil {
                if earlyInputReplies.count < 32 { earlyInputReplies.append(envelope) }
            }
            return
        }
        let epoch = isEdit ? flight!.edit.epoch : query!.epoch
        let serial = isEdit ? flight!.edit.serial : query!.serial
        if isEdit { finishInputFlight() } else { inputRequests.removeValue(forKey: tab) }
        if case .error(let message) = envelope.message {
            if isEdit, tab == selected, epoch == documentEpochs[tab, default: 0], serial == inputSerials[tab, default: 0] {
                nativeInputFailure = (tab, epoch, inputStates[tab]?.focus_generation, message)
            }
            inputQueue.removeAll { $0.tab == tab }; requestTextInput(tab); drainTextInput(); return
        }
        if case .textInputState(let state) = envelope.message,
           epoch == documentEpochs[tab, default: 0], serial == inputSerials[tab, default: 0],
           state.tab_id == tab, state.frame_generation == frames[tab]?.generation,
           state.frame_source == PageRepresentation.frameSource(directory: session.frameDirectory.path) {
            inputStates[tab] = state
            if tab == selected { textInputState = state }
            if let failure = nativeInputFailure, failure.tab == tab, failure.epoch == epoch,
               isEdit || failure.focus.map({ $0 != state.focus_generation }) == true {
                // Clear only the correlated editing notice after an accepted
                // edit or focus change. Navigation/policy/service errors keep
                // their own status, even if an input reply arrives afterward.
                if tab == selected, status == failure.message { status = "Ready" }
                nativeInputFailure = nil
            }
        } else if case .textInputUnavailable = envelope.message,
                  epoch == documentEpochs[tab, default: 0], serial == inputSerials[tab, default: 0] {
            inputStates.removeValue(forKey: tab); inputQueue.removeAll { $0.tab == tab }
            if tab == selected { textInputState = nil; status = "Native text input unavailable" }
        } else { requestTextInput(tab) }
        drainTextInput()
    }

    private func clearTextInput() {
        nativeInputFailure = nil
        inputTimeout?.cancel(); inputFlight = nil; inputQueue.removeAll(); inputStates.removeAll()
        inputRequests.removeAll(); inputFocusPending.removeAll(); earlyInputReplies.removeAll(); textInputState = nil; textInputBusy = false
    }

    func navigateAddress() {
        var url = address.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !url.isEmpty, let selected else { return }
        if !url.contains(":") { url = "https://" + url }
        pageFocusSerial &+= 1
        Task { await navigate(url, tab: selected) }
    }

    func reload() {
        guard let selected, tabs.first(where: { $0.id == selected })?.url != nil else { return }
        status = "Loading…"
        action(.unit("Reload"), tab: selected)
    }

    func resolveResubmission(_ id: UInt64, accept: Bool) {
        guard let selected, resubmissions[selected]?.confirmationID == id else { return }
        resubmissions.removeValue(forKey: selected); resubmission = nil; resubmissionPresented = false
        status = accept ? "Loading…" : "Ready"
        action(.values("ConfirmFormResubmission", ["confirmation_id": .unsigned(id), "accept": .boolean(accept)]), tab: selected)
    }

    private func navigate(_ url: String, tab: UInt64) async {
        if tab == selected { status = "Loading…" }
        await send(.values("Navigate", ["url": .string(url)]), tab: tab)
        await resize()
    }

    @discardableResult
    private func send(_ command: BrowserCommand, tab: UInt64? = nil) async -> UInt64? {
        guard ready else { return nil }
        do { return try await session.send(command, tab: tab) }
        catch { status = error.localizedDescription; return nil }
    }

    func viewportChanged(_ size: CGSize) {
        guard size.width > 0, size.height > 0, viewport != size else { return }
        viewport = size
        resizeTask?.cancel()
        resizeTask = Task {
            try? await Task.sleep(for: .milliseconds(100))
            if !Task.isCancelled { await resize() }
        }
    }

    private func resize() async {
        guard let selected else { return }
        let width = UInt64(min(4096, max(1, viewport.width.rounded())))
        let height = UInt64(min(4096, max(1, viewport.height.rounded())))
        await send(.values("Resize", ["width": .unsigned(width), "height": .unsigned(height)]), tab: selected)
    }

    func stop() async { resubmissions.removeAll(); resubmission = nil; resubmissionPresented = false; ready = false; representation = nil; representations.removeAll(); representationRequests.removeAll(); clearTextInput(); resizeTask?.cancel(); await session.stop() }
}
