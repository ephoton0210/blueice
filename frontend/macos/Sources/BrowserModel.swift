// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine

@MainActor
final class BrowserModel: ObservableObject {
    let appearance: BrowserAppearance
    @Published private(set) var displayPreferences: DisplayPreferences?
    private var preferenceStates: [UInt64: DisplayPreferencesState] = [:]
    private var preferenceObservation: AnyCancellable?
    private var preferenceTask: Task<Void, Never>?
    private var lastSentPreferences: DisplayPreferences?
    init(appearance: BrowserAppearance? = nil) {
        self.appearance = appearance ?? BrowserAppearance()
        preferenceObservation = self.appearance.$resolved.removeDuplicates().sink { [weak self] _ in
            Task { @MainActor in self?.synchronizePreferences() }
        }
    }
    private func synchronizePreferences() {
        guard ready, selected != nil, preferenceTask == nil else { return }
        preferenceTask = Task {
            while ready, let tab = selected, lastSentPreferences != appearance.resolved, !Task.isCancelled {
                let next = appearance.resolved
                guard await send(.displayPreferences(next), tab: tab) != nil else { break }
                lastSentPreferences = next
            }
            preferenceTask = nil
        }
    }
    @Published private(set) var tabs: [BrowserTab] = []
    @Published private(set) var groups: [BrowserTabGroup] = []
    @Published private(set) var groupsAvailable = false
    @Published private(set) var groupBusy = false
    @Published private(set) var groupError: String?
    @Published var groupEditor: TabGroupEditor?
    private var groupOperation: UUID?
    private var groupReplies: [IncomingEnvelope] = []
    private var groupRequests: [UInt64] = []
    @Published private(set) var selected: UInt64?
    @Published var address = ""
    @Published private(set) var status = "Starting BlueIce…"
    @Published private(set) var resubmission: FormResubmission?
    @Published var resubmissionPresented = false
    private var resubmissions: [UInt64: FormResubmission] = [:]
    @Published private(set) var ready = false
    @Published private(set) var image: CGImage?
    @Published private(set) var generation: UInt64 = 0
    @Published private(set) var displayState: ViewportState?
    private var displayStates: [UInt64: ViewportState] = [:]
    private var desiredZooms: [UInt64: Double] = [:]
    private var zoomWrites: [UInt64: Double] = [:]
    private var zoomTask: Task<Void, Never>?
    var cssViewportSize: CGSize? {
        guard let state = displayState, state.frameGeneration == generation else { return nil }
        return CGSize(width: state.cssWidth, height: state.cssHeight)
    }
    var zoomPercent: Int { Int(((displayState?.zoom ?? 1) * 100).rounded()) }
    private let zoomSteps: [Double] = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5]
    func changeZoom(increase: Bool) {
        guard ready, selected != nil else { return }
        let current = selected.flatMap { desiredZooms[$0] } ?? displayState?.zoom ?? 1
        let value = increase ? zoomSteps.first(where: { $0 > current + 0.001 }) : zoomSteps.last(where: { $0 < current - 0.001 })
        if let value { setZoom(value) }
    }
    func setZoom(_ zoom: Double) {
        guard ready, let tab = selected, zoom.isFinite, (0.25...5).contains(zoom) else { return }
        desiredZooms[tab] = zoom
        zoomWrites[tab] = zoom
        guard zoomTask == nil else { return }
        // Absolute zoom updates may coalesce, but their writes must be ordered.
        // Independent Tasks can send an older step after the user's final step.
        zoomTask = Task {
            defer { zoomTask = nil }
            while ready, !Task.isCancelled, let next = zoomWrites.first {
                zoomWrites.removeValue(forKey: next.key)
                guard tabs.contains(where: { $0.id == next.key }) else { continue }
                guard await send(.values("SetPageZoom", ["zoom": .number(next.value)]), tab: next.key) != nil else { break }
            }
        }
    }
    @Published private(set) var history = HistoryState()
    @Published private(set) var representation: PageRepresentation?
    @Published private(set) var accessibilityEpoch: UInt64 = 0
    @Published private(set) var textInputState: TextInputState?
    @Published private(set) var textInputBusy = false
    @Published private(set) var pageFocusSerial: UInt64 = 0
    @Published private(set) var addressFocusSerial: UInt64 = 0
    func requestAddressFocus() { addressFocusSerial &+= 1 }
    @Published private(set) var findVisible = false
    @Published private(set) var findQuery = ""
    @Published private(set) var findCaseSensitive = false
    @Published private(set) var findResult: FindState?
    @Published private(set) var findFocusSerial: UInt64 = 0
    private struct FindPanel {
        var shown = false
        var query = ""
        var sensitive = false
        var result: FindState?
    }
    private var findPanels: [UInt64: FindPanel] = [:]
    private var findTask: Task<Void, Never>?
    private var menuOperation: UUID?
    private var menuReplies: [IncomingEnvelope] = []
    private var linkTabReplies: [UUID: [IncomingEnvelope]] = [:]
    var findSummary: String {
        if findQuery.isEmpty { return "Type to find in page" }
        if findQuery.utf8.count > 1024 { return "Search is too long" }
        guard let result = findResult else { return "Searching…" }
        if result.matchCount == 0 { return result.limited ? "No matches in searched portion" : "No matches" }
        return "\(result.activeMatch ?? 0) of \(result.matchCount)\(result.limited ? "+ · Partial results" : "")\(result.wrapped ? " · Wrapped" : "")"
    }
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
        var clipboard: ClipboardAction? = nil
    }
    private enum ClipboardAction { case copy, cut, paste }
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
    private var deviceScale = 1.0
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
                // The reader is serial; preserve that order on the main queue.
                // Independent Tasks may run metadata before its matching frame.
                DispatchQueue.main.async { [weak self] in self?.apply(envelope, pixels: frame) }
            } catch {
                let message = error.localizedDescription
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.status = message; self.ready = false; self.representation = nil; self.clearTextInput()
                    Task { await self.session.stop() }
                }
            }
        }
        do {
            if supervised { try await session.start(launcher: executable) }
            else { try await session.start(executable: executable) }
            ready = true
            status = "Ready"
            await resize(tab: 1)
            let preferences = appearance.resolved
            if await send(.displayPreferences(preferences), tab: 1) != nil { lastSentPreferences = preferences }
            await send(.unit("ListTabs"))
            await send(.unit("ListTabGroups"))
            await send(.values("Navigate", ["url": .string("about:credits")]), tab: 1)
        } catch { status = error.localizedDescription; await session.stop() }
    }

    private func apply(_ envelope: IncomingEnvelope, pixels: FramePixels?) {
        let tab = envelope.tabID
        let groupRequest = envelope.requestID.map { groupRequests.contains($0) } ?? false
        if groupRequest { groupRequests.removeAll { $0 == envelope.requestID } }
        if groupOperation != nil, groupRequest, groupReplies.count < 128 {
            switch envelope.message {
            case .groupChanged, .groupAssigned, .groupClosed, .groupsUnavailable, .error:
                groupReplies.append(envelope)
            default: break
            }
        }
        if groupRequest, case .error(let message) = envelope.message {
            groupError = message
            return
        }
        switch envelope.message {
        case .opened, .blocked, .error:
            for ticket in Array(linkTabReplies.keys) where (linkTabReplies[ticket]?.count ?? 16) < 16 {
                linkTabReplies[ticket]?.append(envelope)
            }
        default: break
        }
        switch envelope.message {
        case .groups(let list):
            groups = list; groupsAvailable = true
        case .groupChanged(let group, _):
            if let index = groups.firstIndex(where: { $0.id == group.id }) { groups[index] = group }
            else { groups.append(group) }
        case .groupAssigned(let id, let group):
            guard tab == id, let index = tabs.firstIndex(where: { $0.id == id }) else { return }
            tabs[index].groupID = group
            if let group, !groups.contains(where: { $0.id == group }) { action(.unit("ListTabGroups")) }
        case .groupClosed(let id):
            groups.removeAll { $0.id == id }
            for index in tabs.indices where tabs[index].groupID == id { tabs[index].groupID = nil }
        case .groupsUnavailable:
            groups = []; groupsAvailable = false; groupError = "Tab groups unavailable"
        case .tabs(let list):
            tabs = list
            let live = Set(list.map(\.id))
            findPanels = findPanels.filter { live.contains($0.key) }
            resubmissions = resubmissions.filter { live.contains($0.key) }
            if selected.map({ !live.contains($0) }) == true { resubmission = nil; resubmissionPresented = false }
            frames = frames.filter { live.contains($0.key) }
            displayStates = displayStates.filter { live.contains($0.key) }
            preferenceStates = preferenceStates.filter { live.contains($0.key) }
            desiredZooms = desiredZooms.filter { live.contains($0.key) }
            zoomWrites = zoomWrites.filter { live.contains($0.key) }
            histories = histories.filter { live.contains($0.key) }
            representations = representations.filter { live.contains($0.key) }
            documentEpochs = documentEpochs.filter { live.contains($0.key) }
            representationRequests = representationRequests.filter { live.contains($0.key) }
            inputStates = inputStates.filter { live.contains($0.key) }
            inputRequests = inputRequests.filter { live.contains($0.key) }
            inputQueue.removeAll { !live.contains($0.tab) }
            if selected == nil || !live.contains(selected!) { selected = list.first?.id }
            showSelected()
        case .opened(let id, let url):
            if menuOperation != nil && menuReplies.count < 16 { menuReplies.append(envelope) }
            selected = id
            if !tabs.contains(where: { $0.id == id }) { tabs.append(BrowserTab(id: id, url: url)) }
            showSelected()
            if url != nil { status = "Ready" }
            Task {
                await send(.unit("ListTabs"))
                if url == nil { await navigate("about:credits", tab: id) }
                await send(.unit("GetHistoryState"), tab: id)
            }
        case .closed:
            Task { await send(.unit("ListTabs")) }
        case .navigationStarted:
            if tab == selected { status = "Loading…" }
        case .navigated(let url):
            if let tab { findPanels.removeValue(forKey: tab) }
            if tab == selected { findTask?.cancel(); findVisible = false; findQuery = ""; findResult = nil }
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
            if tab == selected { displayState = nil }
            if tab == selected { displayPreferences = nil }
            representations.removeValue(forKey: tab)
            if tab == selected { showFrame(pixels); representation = nil }
            requestRepresentation(tab)
            requestTextInput(tab)
            if findPanels[tab]?.shown == true { action(.unit("GetFindState"), tab: tab) }
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
            if menuOperation != nil && menuReplies.count < 16 { menuReplies.append(envelope) }
            if tab == selected { status = "Navigation blocked: " + reason }
        case .error(let message):
            if menuOperation != nil && menuReplies.count < 16 { menuReplies.append(envelope) }
            if tab == nil || tab == selected { status = message }
            if let request = envelope.requestID, request == inputFlight?.request {
                applyTextInput(envelope)
            } else if inputFlight != nil && inputFlight?.request == nil {
                if earlyInputReplies.count < 32 { earlyInputReplies.append(envelope) }
            }
        case .textInputState, .textInputUnavailable:
            applyTextInput(envelope)
        case .findState(let state):
            guard let tab, tab == state.tabID, let context = inputStates[tab]?.context,
                  state.frameSource == context.frame_source, state.documentGeneration == context.document_generation,
                  var panel = findPanels[tab], panel.shown, panel.query == state.query, panel.sensitive == state.caseSensitive,
                  state.revision >= (panel.result?.revision ?? 0) else { return }
            panel.result = state; findPanels[tab] = panel
            if tab == selected { findResult = state }
        case .findUnavailable:
            if let tab { findPanels[tab]?.result = nil }
            if tab == selected { findResult = nil; status = "Page find unavailable" }
        case .contextMenu, .contextLink, .contextUnavailable:
            if menuOperation != nil && menuReplies.count < 16 { menuReplies.append(envelope) }
        case .viewportState(let state):
            guard let tab, state.tabID == tab, let frame = frames[tab], state.frameGeneration == frame.generation,
                  state.pixelWidth == frame.width, state.pixelHeight == frame.height,
                  state.frameSource == PageRepresentation.frameSource(directory: session.frameDirectory.path) else { return }
            displayStates[tab] = state
            if desiredZooms[tab] == state.zoom { desiredZooms.removeValue(forKey: tab) }
            if tab == selected { displayState = state }
        case .viewportUnavailable:
            if let tab { displayStates.removeValue(forKey: tab) }
            if tab == selected { displayState = nil; status = "Display geometry unavailable" }
        case .displayPreferences(let state):
            guard let tab, state.tabID == tab, state.frameGeneration == frames[tab]?.generation,
                  state.frameSource == PageRepresentation.frameSource(directory: session.frameDirectory.path) else { return }
            preferenceStates[tab] = state
            if tab == selected { displayPreferences = state.preferences }
        case .displayPreferencesUnavailable:
            if let tab { preferenceStates.removeValue(forKey: tab) }
            if tab == selected { displayPreferences = nil; status = "Display preferences unavailable" }
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
        displayState = selected.flatMap { displayStates[$0] }
        displayPreferences = selected.flatMap { preferenceStates[$0]?.preferences }
        synchronizePreferences()
        representation = selected.flatMap { representations[$0] }
        accessibilityEpoch = selected.flatMap { documentEpochs[$0] } ?? 0
        textInputState = selected.flatMap { inputStates[$0] }
        let find = selected.flatMap { findPanels[$0] } ?? FindPanel()
        findVisible = find.shown; findQuery = find.query; findCaseSensitive = find.sensitive; findResult = find.result
    }

    func select(_ tab: UInt64) {
        findTask?.cancel()
        if let previous = selected, inputStates[previous]?.focused?.marked != nil {
            textInput(.finishComposition)
        }
        selected = tab
        resubmission = resubmissions[tab]; resubmissionPresented = resubmission != nil
        showSelected()
        if findVisible { action(.unit("GetFindState"), tab: tab) }
        Task { await send(.unit("GetHistoryState"), tab: tab); await resize() }
    }

    func action(_ command: BrowserCommand, tab: UInt64? = nil) {
        let target = tab ?? selected
        Task { await send(command, tab: target) }
    }

    func beginTabGroupEditor(group: UInt64? = nil, tab: UInt64? = nil) {
        guard ready, groupsAvailable, !groupBusy else { return }
        groupError = nil
        if let group, let current = groups.first(where: { $0.id == group }) {
            groupEditor = TabGroupEditor(groupID: group, name: current.name, color: current.color, tabID: nil)
        } else if group == nil {
            groupEditor = TabGroupEditor(groupID: nil, name: "", color: "#4477cc", tabID: tab ?? selected)
        }
    }

    private func beginGroupOperation() -> UUID? {
        guard ready, groupsAvailable, !groupBusy else { return nil }
        let token = UUID(); groupOperation = token; groupReplies = []; groupBusy = true; groupError = nil
        return token
    }
    private func endGroupOperation(_ token: UUID) {
        guard groupOperation == token else { return }
        groupOperation = nil; groupReplies = []; groupBusy = false
    }
    private func groupReply(_ action: TabGroupAction, tab: UInt64? = nil, token: UUID) async -> BrowserMessage? {
        guard let request = await sendTabGroupCommand(action, tab: tab) else { return nil }
        let deadline = Date().addingTimeInterval(5)
        while ready, !Task.isCancelled, groupOperation == token, Date() < deadline {
            if let index = groupReplies.firstIndex(where: { $0.requestID == request && $0.tabID == tab }) {
                let message = groupReplies.remove(at: index).message
                if case .error(let message) = message { groupError = message; return nil }
                if case .groupsUnavailable = message { groupError = "Tab groups unavailable"; return nil }
                return message
            }
            try? await Task.sleep(for: .milliseconds(10))
        }
        if ready { groupError = "The tab group update did not complete" }
        return nil
    }
    @discardableResult
    func sendTabGroupCommand(_ action: TabGroupAction, tab: UInt64? = nil) async -> UInt64? {
        guard ready else { return nil }
        do {
            return try await session.send(.tabGroup(action), tab: tab) { [weak self] request in
                // Both registration and inbound messages use the serial main
                // queue, so even an immediate core error has its own notice.
                DispatchQueue.main.async { [weak self] in
                    guard let self, self.ready else { return }
                    self.groupRequests.append(request)
                    if self.groupRequests.count > 128 { self.groupRequests.removeFirst() }
                }
            }
        } catch { groupError = error.localizedDescription; return nil }
    }
    func dismissTabGroupError() { groupError = nil }
    func createTabGroup(name: String, color: String, tab: UInt64?) async -> UInt64? {
        guard let token = beginGroupOperation() else { return nil }; defer { endGroupOperation(token) }
        guard BrowserTabGroup.validName(name), BrowserTabGroup.validColor(color.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            groupError = "Enter a name of up to 80 characters and a six-digit hex color"; return nil
        }
        if let tab, !tabs.contains(where: { $0.id == tab }) { groupError = "The tab was closed"; return nil }
        guard case .groupChanged(let group, true) = await groupReply(.create(name, color), token: token) else { return nil }
        if let tab {
            if tabs.contains(where: { $0.id == tab }) {
                guard case .groupAssigned(let replyTab, let id) = await groupReply(.assign(group.id), tab: tab, token: token),
                      replyTab == tab, id == group.id else { return nil }
            } else { groupError = "The group was created, but its tab was closed" }
        }
        return group.id
    }
    func updateTabGroup(_ id: UInt64, name: String, color: String) async -> Bool {
        guard let token = beginGroupOperation() else { return false }; defer { endGroupOperation(token) }
        guard groups.contains(where: { $0.id == id }) else { groupError = "The group no longer exists"; return false }
        guard BrowserTabGroup.validName(name), BrowserTabGroup.validColor(color.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            groupError = "Enter a name of up to 80 characters and a six-digit hex color"; return false
        }
        guard case .groupChanged(let renamed, false) = await groupReply(.rename(id, name), token: token), renamed.id == id else { return false }
        guard case .groupChanged(let recolored, false) = await groupReply(.color(id, color), token: token), recolored.id == id else { return false }
        return true
    }
    func setTabGroup(_ group: UInt64?, tab: UInt64) async -> Bool {
        guard let token = beginGroupOperation() else { return false }; defer { endGroupOperation(token) }
        guard tabs.contains(where: { $0.id == tab }), group == nil || groups.contains(where: { $0.id == group }) else {
            groupError = "The tab or group no longer exists"; return false
        }
        guard case .groupAssigned(let replyTab, let id) = await groupReply(.assign(group), tab: tab, token: token) else { return false }
        return replyTab == tab && id == group
    }
    func setTabGroupCollapsed(_ id: UInt64, collapsed: Bool) async -> Bool {
        guard let token = beginGroupOperation() else { return false }; defer { endGroupOperation(token) }
        guard groups.contains(where: { $0.id == id }) else { groupError = "The group no longer exists"; return false }
        guard case .groupChanged(let group, false) = await groupReply(.collapse(id, collapsed), token: token) else { return false }
        return group.id == id && group.collapsed == collapsed
    }
    func removeTabGroup(_ id: UInt64) async -> Bool {
        guard let token = beginGroupOperation() else { return false }; defer { endGroupOperation(token) }
        guard groups.contains(where: { $0.id == id }) else { groupError = "The group no longer exists"; return false }
        guard case .groupClosed(let removed) = await groupReply(.remove(id), token: token) else { return false }
        return removed == id
    }

    func contextMenuIsCurrent(_ context: PageMenuContext) -> Bool {
        ready && selected == context.tabID && tabs.contains(where: { $0.id == context.tabID })
            && context.frameSource == PageRepresentation.frameSource(directory: session.frameDirectory.path)
            && frames[context.tabID]?.generation == context.frameGeneration
    }

    func requestContextMenu(x: Double, y: Double) async -> PageContextMenu? {
        guard ready, !textInputBusy, let tab = selected, let frame = frames[tab] else { return nil }
        let operation = UUID(), epoch = documentEpochs[tab, default: 0]
        menuOperation = operation; menuReplies.removeAll()
        defer { if menuOperation == operation { menuOperation = nil; menuReplies.removeAll() } }
        guard let request = await send(.values("GetContextMenu", ["tab_id": .unsigned(tab),
            "frame_source": .unsigned(PageRepresentation.frameSource(directory: session.frameDirectory.path)),
            "frame_generation": .unsigned(frame.generation), "x": .number(x), "y": .number(y)]), tab: tab) else { return nil }
        let deadline = Date().addingTimeInterval(5)
        while !Task.isCancelled, ready, menuOperation == operation, selected == tab, documentEpochs[tab, default: 0] == epoch, Date() < deadline {
            if let reply = menuReplies.first(where: { $0.requestID == request && $0.tabID == tab }) {
                guard case .contextMenu(let menu) = reply.message, contextMenuIsCurrent(menu.context) else { return nil }
                if let input = menu.input {
                    inputSerials[tab, default: 0] += 1
                    inputRequests.removeValue(forKey: tab)
                    inputStates[tab] = input; textInputState = input
                }
                return menu
            }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return nil
    }

    func performContextLink(_ context: PageMenuContext, action: PageMenuLinkAction) async {
        guard contextMenuIsCurrent(context) else { return }
        if action == .open { await send(.contextMenuLink(context, action), tab: context.tabID); return }
        if action == .newTab { await openContextLinkTab(context); return }
        let operation = UUID()
        menuOperation = operation; menuReplies.removeAll()
        defer { if menuOperation == operation { menuOperation = nil; menuReplies.removeAll() } }
        guard let request = await send(.contextMenuLink(context, action), tab: context.tabID) else { return }
        let deadline = Date().addingTimeInterval(5)
        while !Task.isCancelled, menuOperation == operation, contextMenuIsCurrent(context), Date() < deadline {
            if let reply = menuReplies.first(where: { $0.requestID == request && $0.tabID == context.tabID }) {
                guard case .contextLink(let link) = reply.message, link.context == context else { return }
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(link.url, forType: .string)
                return
            }
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    private func openContextLinkTab(_ context: PageMenuContext) async {
        guard linkTabReplies.count < 8 else { return }
        let ticket = UUID()
        linkTabReplies[ticket] = []
        defer { linkTabReplies.removeValue(forKey: ticket) }
        guard let request = await send(.contextMenuLink(context, .newTab), tab: context.tabID) else { return }
        let deadline = Date().addingTimeInterval(30)
        while !Task.isCancelled, ready, Date() < deadline {
            if let reply = linkTabReplies[ticket]?.first(where: { $0.requestID == request }) {
                let notice: String?
                switch reply.message {
                case .blocked(let reason): notice = "Navigation blocked: " + reason
                case .error(let message): notice = message
                default: notice = nil
                }
                // OpenTab's deferred result addresses the new tab. Preserve
                // the source URL/history and visibly report its denied request.
                if let notice {
                    if contextMenuIsCurrent(context) { status = notice }
                    if let tab = reply.tabID, tab != context.tabID, !tabs.contains(where: { $0.id == tab }) {
                        await send(.unit("CloseTab"), tab: tab)
                    }
                }
                return
            }
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    func showFind() {
        guard ready, let selected else { return }
        var panel = findPanels[selected] ?? FindPanel()
        let wasShown = panel.shown
        panel.shown = true; findPanels[selected] = panel
        findVisible = true; findQuery = panel.query; findCaseSensitive = panel.sensitive
        findFocusSerial &+= 1
        if !wasShown { scheduleFind() }
    }

    func setFindQuery(_ query: String) {
        guard let selected else { return }
        findQuery = query; findResult = nil
        findPanels[selected, default: FindPanel()].query = query
        findPanels[selected]?.result = nil
        scheduleFind()
    }

    func setFindCaseSensitive(_ sensitive: Bool) {
        guard let selected else { return }
        findCaseSensitive = sensitive; findResult = nil
        findPanels[selected, default: FindPanel()].sensitive = sensitive
        findPanels[selected]?.result = nil
        scheduleFind()
    }

    private func scheduleFind() {
        findTask?.cancel()
        guard ready, let tab = selected, let panel = findPanels[tab], panel.shown, panel.query.utf8.count <= 1024 else { return }
        let epoch = documentEpochs[tab, default: 0]
        findTask = Task {
            try? await Task.sleep(for: .milliseconds(120))
            for _ in 0..<1500 {
                guard !Task.isCancelled, selected == tab, documentEpochs[tab, default: 0] == epoch,
                      findPanels[tab]?.shown == true else { return }
                if let context = inputStates[tab]?.context {
                    await send(.find(tab, context, .update(panel.query, panel.sensitive)), tab: tab)
                    return
                }
                try? await Task.sleep(for: .milliseconds(10))
            }
        }
    }

    func findNext(backwards: Bool = false) {
        guard ready, let tab = selected else { return }
        if !findVisible { showFind(); return }
        guard let context = inputStates[tab]?.context, findResult?.matchCount ?? 0 > 0 else { return }
        action(.find(tab, context, .next(backwards)), tab: tab)
    }

    func closeFind() {
        findTask?.cancel()
        guard let tab = selected else { return }
        findPanels[tab]?.shown = false; findPanels[tab]?.result = nil
        findVisible = false; findResult = nil; pageFocusSerial &+= 1
        if let context = inputStates[tab]?.context { action(.find(tab, context, .close), tab: tab) }
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
        guard let cssSize = cssViewportSize else { return false }
        let visible = CGRect(x: node.bounds.x, y: node.bounds.y - snapshot.scrollY,
                             width: node.bounds.width, height: node.bounds.height)
            .intersection(CGRect(origin: .zero, size: cssSize))
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

    func copySelection(cut: Bool = false) { enqueueClipboard(cut ? .cut : .copy) }
    func pasteClipboard() { enqueueClipboard(.paste) }
    private func enqueueClipboard(_ command: ClipboardAction) {
        guard ready, let tab = selected, inputQueue.count < 128 else { return }
        inputQueue.append(InputEdit(tab: tab, epoch: documentEpochs[tab, default: 0],
                                    serial: inputSerials[tab, default: 0], action: .replace("", nil), clipboard: command))
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
            var action = edit.action
            if let clipboard = edit.clipboard {
                guard selected == edit.tab, let field = state.focused else { continue }
                switch clipboard {
                case .copy, .cut:
                    guard !field.protected, clipboard != .cut || field.writable,
                          field.selection.length > 0, let text = field.text,
                          NSMaxRange(field.selection.nsRange) <= text.utf16.count else { continue }
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString((text as NSString).substring(with: field.selection.nsRange), forType: .string)
                    if clipboard == .copy { continue }
                case .paste:
                    guard let text = NSPasteboard.general.string(forType: .string), text.utf16.count <= 65_536 else { continue }
                    action = .replace(text, nil)
                }
            }
            inputFlight = (edit, nil)
            inputTimeout?.cancel()
            inputTimeout = Task {
                try? await Task.sleep(for: .seconds(15))
                if !Task.isCancelled { finishInputFlight(); inputQueue.removeAll(); status = "Text input did not respond" }
            }
            Task {
                let request = await send(.textInput(state.context, action), tab: edit.tab)
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

    func viewportChanged(_ size: CGSize, deviceScale: Double = 1) {
        guard size.width > 0, size.height > 0, deviceScale.isFinite, deviceScale >= 1,
              viewport != size || self.deviceScale != deviceScale else { return }
        viewport = size
        self.deviceScale = min(4, deviceScale)
        resizeTask?.cancel()
        resizeTask = Task {
            try? await Task.sleep(for: .milliseconds(100))
            if !Task.isCancelled { await resize() }
        }
    }

    private func resize(tab: UInt64? = nil) async {
        guard let target = tab ?? selected else { return }
        let width = min(4096, max(1, viewport.width)), height = min(4096, max(1, viewport.height))
        let density = min(deviceScale, 4096 / max(width, height))
        await send(.viewport(width, height, density, backingScale: deviceScale), tab: target)
    }

    func stop() async { groupOperation = nil; groupReplies = []; groupRequests = []; groupBusy = false; groups = []; groupsAvailable = false; groupEditor = nil; zoomTask?.cancel(); zoomWrites.removeAll(); preferenceTask?.cancel(); lastSentPreferences = nil; preferenceStates.removeAll(); displayPreferences = nil; findTask?.cancel(); findPanels.removeAll(); findVisible = false; findResult = nil; resubmissions.removeAll(); resubmission = nil; resubmissionPresented = false; ready = false; representation = nil; representations.removeAll(); representationRequests.removeAll(); clearTextInput(); resizeTask?.cancel(); await session.stop() }
}
