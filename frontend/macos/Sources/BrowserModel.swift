// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine

@MainActor
final class BrowserModel: ObservableObject {
    let assistant = BrowserAssistant()
    @Published var assistantPresented = false
    private var assistantRequestIDs: Set<UInt64> = []
    private var assistantObservation: AnyCancellable?
    var canAskAssistant: Bool { ready && status != "Loading…" && selected.flatMap { assistant.page($0)?.translation } != nil }
    var canOpenAssistantTask: Bool { ready && status != "Loading…" && textInputState != nil }
    func showAndAskAssistant(organize: Bool = false) {
        guard canOpenAssistantTask, let tab = selected else { return }
        assistantPresented = true; observeAssistant(tab)
        guard let document = assistant.page(tab)?.document else { return }
        refreshAssistantTranslation()
        Task {
            let deadline = Date().addingTimeInterval(5)
            while ready, selected == tab, assistant.page(tab)?.document == document, Date() < deadline {
                if canAskAssistant { askAssistant(organize: organize); return }
                try? await Task.sleep(for: .milliseconds(20))
            }
        }
    }
    func toggleAssistant() {
        assistantPresented.toggle()
        if assistantPresented { refreshAssistantTranslation() }
    }
    private func observeAssistant(_ tab: UInt64) {
        guard let input = inputStates[tab], let url = tabs.first(where: { $0.id == tab })?.url else { return }
        assistant.observe(.init(tab_id: tab, frame_source: input.frame_source, document_generation: input.document_generation), url: url)
    }
    func refreshAssistantTranslation() {
        guard ready, status != "Loading…", let tab = selected else { return }
        observeAssistant(tab)
        if let operation = assistant.translate(tab, action: .inspect) { performAssistant(operation) }
    }
    func askAssistant(organize: Bool = false) {
        guard canAskAssistant, let tab = selected else { return }
        observeAssistant(tab)
        let task: AssistantTask = organize ? .organized(assistant.page(tab)?.instruction ?? "") : .summary
        if let operation = assistant.begin(tab, task: task) { performAssistant(operation) }
    }
    func changeTranslation(_ action: AssistantTranslationAction) {
        guard ready, status != "Loading…", let tab = selected else { return }
        observeAssistant(tab)
        if let operation = assistant.translate(tab, action: action) { performAssistant(operation) }
    }
    private func performAssistant(_ operation: AssistantOperation) {
        Task {
            do {
                let register: @Sendable (UInt64) -> Void = { [weak model = self] request in
                    DispatchQueue.main.async {
                        operation.requestID = request
                        model?.assistantRequestIDs.insert(request)
                        if let model, model.assistantRequestIDs.count > 512, let oldest = model.assistantRequestIDs.min() { model.assistantRequestIDs.remove(oldest) }
                    }
                }
                if let workspace { _ = try await workspace.send(operation.command, tab: operation.document.tab_id, owner: self, willSend: register) }
                else { _ = try await session.send(operation.command, tab: operation.document.tab_id, willSend: register) }
            } catch { operation.owner?.expire(operation) }
        }
    }
    let appearance: BrowserAppearance
    let windowID: UInt64
    private(set) var contextID: UInt64 = 1
    @Published private(set) var profileName = "Default"
    @Published var profileEditor: ProfileEditor?
    @Published var downloadsPresented = false
    @Published var fileInputErrorPresented = false
    private(set) var fileInputError = ""
    private var fileOperation: UUID?
    private var fileReplies: [IncomingEnvelope] = []
    func presentFileInputError(_ text: String) { fileInputError = text; fileInputErrorPresented = true }
    func fileInputIsCurrent(_ context: FileInputContext, tab: UInt64) -> Bool {
        ready && selected == tab && context.tab_id == tab && tabs.contains(where: { $0.id == tab }) && status != "Loading…"
            && context.frame_source == PageRepresentation.frameSource(directory: session.frameDirectory.path)
            && textInputState?.document_generation == context.document_generation
    }
    private func fileReply(_ action: FileInputAction, tab: UInt64) async -> FileInputState? {
        guard ready, fileOperation == nil else { return nil }
        let token = UUID(); fileOperation = token; fileReplies.removeAll()
        defer { if fileOperation == token { fileOperation = nil; fileReplies.removeAll() } }
        let epoch = documentEpochs[tab,default: 0]
        guard let request = await send(.fileInput(action),tab: tab) else { return nil }
        let deadline = Date().addingTimeInterval(5)
        while !Task.isCancelled, ready, fileOperation == token, selected == tab,
              documentEpochs[tab,default: 0] == epoch, Date() < deadline {
            if let reply = fileReplies.first(where: { $0.requestID == request && $0.tabID == tab }) {
                if case .fileInputState(let state) = reply.message { return state }
                return nil
            }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return nil
    }
    func prepareFileInput(_ node: UInt64) async -> FileInputState? {
        guard ready, !textInputBusy, let tab = selected, let input = textInputState,
              representation?.nodes.contains(where: { $0.id == node && $0.state.fileInput && !$0.state.disabled }) == true else { return nil }
        guard let state = await fileReply(.prepare(input.frame_source,input.document_generation,node),tab: tab),
              state.context.tab_id == tab,
              state.context.frame_source == input.frame_source, state.context.document_generation == input.document_generation,
              state.context.node_id == node, state.accept.utf8.count <= 4096, state.names.count <= 16,
              state.names.allSatisfy(SelectedFile.validName) else { return nil }
        return state
    }
    func setFileInput(_ context: FileInputContext, files: [SelectedFile], tab: UInt64) async -> Bool {
        guard fileInputIsCurrent(context,tab: tab) else { return false }
        guard let state = await fileReply(.set(context,files),tab: tab),
              state.context.frame_source == context.frame_source, state.context.document_generation == context.document_generation,
              state.context.node_id == context.node_id, state.context.revision == context.revision &+ 1,
              state.names == files.map(\.name) else {
            if fileInputIsCurrent(context,tab: tab) { presentFileInputError("The file selection could not be applied. Please choose the files again.") }
            return false
        }
        return true
    }
    @Published private(set) var printBusy = false
    @Published var printErrorPresented = false
    private(set) var printError = ""
    var canPrint: Bool {
        ready && !printBusy && generation > 0 && status != "Loading…" && selected != nil
            && representation?.tabID == selected && textInputState?.tab_id == selected
    }
    func printCurrentPage() async {
        guard canPrint, let tab = selected, let document = textInputState?.document_generation else { return }
        printBusy = true
        defer { printBusy = false }
        let runtime = session.runtimeDirectory
        let source = PageRepresentation.frameSource(directory: session.frameDirectory.path)
        let context = contextID
        let window = windowID
        do {
            let info = BrowserPrintView.info()
            let profile = try PrintProfile.from(info)
            let (printer,output) = try await Task.detached(priority: .userInitiated) {
                let printer = try BrowserPrintSession(runtime: runtime,tab: tab,context: context,window: window,source: source,document: document)
                do { return (printer,try printer.render(profile)) }
                catch { printer.end(); throw error }
            }.value
            defer { printer.end() }
            guard ready, selected == tab, contextID == context else { return }
            let view = BrowserPrintView(session: printer,output: output)
            let operation = NSPrintOperation(view: view,printInfo: info)
            operation.jobTitle = BrowserPrintView.jobTitle(representation)
            operation.showsPrintPanel = true; operation.showsProgressPanel = true
            operation.printPanel.options = [.showsCopies,.showsPageRange,.showsPaperSize,.showsOrientation,.showsScaling,.showsPreview]
            _ = operation.run()
            if let failure = view.failure { throw failure }
        } catch {
            printError = error.localizedDescription; printErrorPresented = true
        }
    }
    private var canonicalContextGroups: [BrowserTabGroup]?
    private weak var workspace: BrowserWorkspace?
    private let session: BrowserSession
    private var windowTabs: [UInt64]?
    var windowManager: BrowserWorkspace? { workspace }
    func updateContext(_ context: BrowserContextSummary) {
        contextID = context.id
        if profileName != context.name { profileName = context.name }
        canonicalContextGroups = context.groups
        if groups != context.groups { groups = context.groups }
        groupsAvailable = true
    }
    @Published private(set) var displayPreferences: DisplayPreferences?
    private var preferenceStates: [UInt64: DisplayPreferencesState] = [:]
    private var preferenceObservation: AnyCancellable?
    private var preferenceTask: Task<Void, Never>?
    private var lastSentPreferences: DisplayPreferences?
    init(appearance: BrowserAppearance? = nil, workspace: BrowserWorkspace? = nil, windowID: UInt64 = 1) {
        self.appearance = appearance ?? BrowserAppearance()
        self.workspace = workspace; self.windowID = windowID
        self.session = workspace?.session ?? BrowserSession()
        assistantObservation = assistant.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
        assistant.languageConfirmed = { [weak self] language in
            let store = BrowserAppearance.preferenceStore()
            if let language { store.set(language, forKey: "browser.translationLanguage") }
            else { store.removeObject(forKey: "browser.translationLanguage") }
            self?.workspace?.refreshAssistantTranslation()
        }
        preferenceObservation = self.appearance.$resolved.removeDuplicates().sink { [weak self] _ in
            Task { @MainActor in self?.synchronizePreferences() }
        }
    }
    private func synchronizePreferences() {
        if let workspace { workspace.synchronizePreferences(); return }
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
    @Published var permissionsErrorPresented = false

    func openPermissions() {
        if !session.openPermissions() { permissionsErrorPresented = true }
    }
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
        workspace?.scheduleSessionSave()
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
    private var frames: [UInt64: FramePixels] = [:]
    private var histories: [UInt64: HistoryState] = [:]
    private var viewport = CGSize(width: 1024, height: 640)
    private var deviceScale = 1.0
    private var resizeTask: Task<Void, Never>?

    func start(launcher: URL? = nil) async {
        if let workspace { await workspace.start(launcher: launcher); return }
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
                    self.assistant.clear(); self.status = message; self.ready = false; self.representation = nil; self.clearTextInput()
                    Task { await self.session.stop() }
                }
            }
        }
        do {
            try await session.startForBrowser(launcher: launcher)
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

    func apply(_ envelope: IncomingEnvelope, pixels: FramePixels?) {
        let tab = envelope.tabID
        switch envelope.message {
        case .assistantResult, .assistantUnavailable, .translation, .translationUnavailable:
            _ = assistant.receive(envelope); return
        case .error where envelope.requestID.map({ assistantRequestIDs.contains($0) || workspace?.isAssistantRequest($0) == true }) == true:
            _ = assistant.receive(envelope); return
        default: break
        }
        if fileOperation != nil, fileReplies.count < 16 {
            switch envelope.message {
            case .fileInputState, .error: fileReplies.append(envelope)
            default: break
            }
        }
        if windowTabs != nil, let tab, !tabs.contains(where: { $0.id == tab }) {
            switch envelope.message {
            case .opened, .closed, .frame, .navigated, .navigationStarted, .history, .textInputState, .textInputUnavailable,
                 .viewportState, .displayPreferences, .representation, .representationUnavailable, .blocked, .error: return
            default: break
            }
        }
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
            if let canonicalContextGroups { groups = canonicalContextGroups; groupsAvailable = true; return }
            groups = list; groupsAvailable = true
        case .groupChanged(let group, _):
            if canonicalContextGroups != nil { return }
            if let index = groups.firstIndex(where: { $0.id == group.id }) { groups[index] = group }
            else { groups.append(group) }
        case .groupAssigned(let id, let group):
            if canonicalContextGroups != nil { return }
            guard tab == id, let index = tabs.firstIndex(where: { $0.id == id }) else { return }
            tabs[index].groupID = group
            if let group, !groups.contains(where: { $0.id == group }) { action(.unit("ListTabGroups")) }
        case .groupClosed(let id):
            if canonicalContextGroups != nil { return }
            groups.removeAll { $0.id == id }
            for index in tabs.indices where tabs[index].groupID == id { tabs[index].groupID = nil }
        case .groupsUnavailable:
            groups = []; groupsAvailable = false; groupError = "Tab groups unavailable"
        case .tabs(let incoming):
            let list = windowTabs.map { ordered in
                let byID = Dictionary(incoming.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
                let existing = Dictionary(tabs.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
                // A context-scoped list is broadcast to every native model.
                // Only the window registry removes ownership; absent values
                // in another context's reply must not clear a live page.
                return ordered.compactMap { byID[$0] ?? existing[$0] }
            } ?? incoming
            if windowTabs != nil, list == tabs { return }
            let live = Set(list.map(\.id))
            assistant.retainTabs(live)
            if let flight = inputFlight, !live.contains(flight.edit.tab) { clearTextInput() }
            tabs = list
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
            if let index = tabs.firstIndex(where: { $0.id == id }) { tabs[index].url = url }
            else { tabs.append(BrowserTab(id: id, url: url)) }
            showSelected()
            if url != nil { status = "Ready" }
            let restoring = workspace?.restoringSession == true
            Task {
                await send(.unit("ListTabs"))
                if url == nil && !restoring { await navigate("about:credits", tab: id) }
                await send(.unit("GetHistoryState"), tab: id)
            }
        case .closed:
            Task { await send(.unit("ListTabs")) }
        case .navigationStarted:
            if let tab { assistant.invalidate(tab, clear: false) }
            if tab == selected { status = "Loading…" }
        case .navigated(let url):
            if let tab { assistant.invalidate(tab, clear: true) }
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
            if tab == selected || (workspace != nil && tabs.contains(where: { $0.id == tab && $0.url == nil })) { status = "Navigation blocked: " + reason }
        case .error(let message):
            if menuOperation != nil && menuReplies.count < 16 { menuReplies.append(envelope) }
            if tab == nil || tab == selected || (workspace != nil && tabs.contains(where: { $0.id == tab && $0.url == nil })) { status = message }
            if let request = envelope.requestID, request == inputFlight?.request {
                applyTextInput(envelope)
            } else if inputFlight != nil && inputFlight?.request == nil {
                if earlyInputReplies.count < 32 { earlyInputReplies.append(envelope) }
            }
        case .textInputState, .textInputUnavailable:
            applyTextInput(envelope)
            if let tab {
                observeAssistant(tab)
                if assistantPresented, tab == selected, assistant.page(tab)?.translation == nil { refreshAssistantTranslation() }
            }
            if let tab, findPanels[tab]?.shown == true, findPanels[tab]?.result == nil { action(.unit("GetFindState"), tab: tab) }
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
        workspace?.scheduleSessionSave()
        resubmission = resubmissions[tab]; resubmissionPresented = resubmission != nil
        showSelected()
        if assistantPresented { refreshAssistantTranslation() }
        if findVisible { action(.unit("GetFindState"), tab: tab) }
        Task { await send(.unit("GetHistoryState"), tab: tab); await resize() }
    }

    func action(_ command: BrowserCommand, tab: UInt64? = nil) {
        let target = tab ?? selected
        if workspace != nil, case .values("OpenTab", let fields) = command {
            let url: String? = { if case .string(let text) = fields["url"] { return text }; return nil }()
            Task { await send(.window(.open(windowID, url))) }; return
        }
        Task { await send(command, tab: target) }
    }

    func prepareManagedSession(_ value: Bool) {
        guard value != ready else { return }; ready = value
        guard value else { return }; status = "Ready"
        if let tab = selected {
            // Retry recovers fresh pixels, semantics and input identity even
            // when canonical membership is unchanged.
            Task { await send(.unit("GetHistoryState"), tab: tab); await resize() }
        }
    }
    func transferLocalTabState(_ tab: UInt64, to destination: BrowserModel) {
        assistant.transfer(tab, to: destination.assistant)
        if var panel = findPanels[tab] { panel.result = nil; destination.findPanels[tab] = panel }
        if let prompt = resubmissions[tab] { destination.resubmissions[tab] = prompt }
    }
    func updateWindowTabs(_ list: [BrowserTab]) {
        windowTabs = list.map(\.id)
        apply(IncomingEnvelope(requestID: nil, tabID: nil, message: .tabs(list)), pixels: nil)
    }
    func workspaceFailed(_ message: String) { assistant.clear(); status = message; ready = false; representation = nil; clearTextInput() }
    func openInitialPage() async {
        await send(.unit("ListTabGroups"))
        await resize()
        if let tab = selected { await send(.values("Navigate", ["url": .string("about:credits")]), tab: tab); await resize() }
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
            let willSend: @Sendable (UInt64) -> Void = { [weak self] request in
                // Both registration and inbound messages use the serial main
                // queue, so even an immediate core error has its own notice.
                DispatchQueue.main.async { [weak self] in
                    guard let self, self.ready else { return }
                    self.groupRequests.append(request)
                    if self.groupRequests.count > 128 { self.groupRequests.removeFirst() }
                }
            }
            if let workspace { return try await workspace.send(.tabGroup(action), tab: tab, owner: self, willSend: willSend) }
            return try await session.send(.tabGroup(action), tab: tab, willSend: willSend)
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

    func performContextLink(_ context: PageMenuContext, action: PageMenuLinkAction, download: Bool = false) async {
        guard contextMenuIsCurrent(context) else { return }
        guard !download || (action == .copy && windowManager != nil) else { return }
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
                if download, let workspace = windowManager {
                    downloadsPresented = true
                    _ = await workspace.downloads.start(link.url,name: nil)
                    return
                }
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
                    if let tab = reply.tabID, tab != context.tabID, windowTabs == nil || windowTabs?.contains(tab) == true {
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
        status = "Loading…"
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
        workspace?.scheduleSessionSave()
        if tab == selected { status = "Loading…" }
        await send(.values("Navigate", ["url": .string(url)]), tab: tab)
        await resize()
    }

    @discardableResult
    private func send(_ command: BrowserCommand, tab: UInt64? = nil) async -> UInt64? {
        guard ready else { return nil }
        do {
            if let workspace { return try await workspace.send(command, tab: tab, owner: self) }
            return try await session.send(command, tab: tab)
        }
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
        let width = min(4096, max(1, viewport.width)), height = min(4096, max(1, viewport.height))
        let density = min(deviceScale, 4096 / max(width, height))
        if let workspace {
            await workspace.resizeWindow(windowID, viewport: WindowViewport(width: width, height: height, deviceScale: density, backingScale: deviceScale))
        } else if let target = tab ?? selected {
            await send(.viewport(width, height, density, backingScale: deviceScale), tab: target)
        }
    }

    func stop() async { assistant.clear(); groupOperation = nil; groupReplies = []; groupRequests = []; groupBusy = false; groups = []; groupsAvailable = false; groupEditor = nil; zoomTask?.cancel(); zoomWrites.removeAll(); preferenceTask?.cancel(); lastSentPreferences = nil; preferenceStates.removeAll(); displayPreferences = nil; findTask?.cancel(); findPanels.removeAll(); findVisible = false; findResult = nil; resubmissions.removeAll(); resubmission = nil; resubmissionPresented = false; ready = false; representation = nil; representations.removeAll(); representationRequests.removeAll(); clearTextInput(); resizeTask?.cancel(); if workspace == nil { await session.stop() } }
}
