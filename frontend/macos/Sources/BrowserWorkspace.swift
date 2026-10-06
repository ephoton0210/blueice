// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine
import SwiftUI

@MainActor
final class BrowserWorkspace: ObservableObject {
    let session = BrowserSession()
    let downloads: BrowserDownloadsModel
    let appearance: BrowserAppearance
    @Published private(set) var windows: [BrowserWindowSummary] = []
    @Published private(set) var contexts: [BrowserContextSummary] = []
    @Published private(set) var canManageContexts = false
    @Published var activeWindowID: UInt64 = 1 { didSet { if activeWindowID != oldValue { scheduleSessionSave() } } }
    let contextPreferences: BrowserContextPreferences
    let sessionPreferences: BrowserSessionPreferences
    @Published private(set) var restoringSession = false
    @Published private var sessionReady = false
    private var sessionRestored = false
    private var captureArmed = false
    private var saveRevision: UInt64 = 0
    private var saveTask: Task<Void, Never>?
    private var windowKeys: [UInt64: UUID] = [:]
    private var sessionRequests: [UInt64: Bool] = [:]
    private var sessionReplies: [IncomingEnvelope] = []
    private var unrestoredHistory: [UInt64: NavigationHistory] = [:]
    var onCaptureWindowFrame: ((UInt64) -> SavedBrowserSession.Frame?)?
    var onRestoreWindowFrame: ((UInt64, SavedBrowserSession.Frame) -> Void)?
    private var contextKeys: [UInt64: UUID] = [1: BrowserContextPreferences.defaultKey]
    private var restoringProfiles = true
    private var pendingProfileKey: UUID?
    private var contextRequests: Set<UInt64> = []
    private var contextReplies: [IncomingEnvelope] = []
    @Published private(set) var canManageWindows = false
    @Published private(set) var canPlaceTabs = false
    let tabDragging = BrowserTabDragging()
    @Published private(set) var busy = false
    @Published private(set) var notice: String?
    private(set) var models: [UInt64: BrowserModel] = [:]
    var onWindowCreated: ((BrowserModel) -> Void)?
    var onWindowClosed: ((UInt64) -> Void)?
    var onActivateWindow: ((UInt64) -> Void)?
    var onContextsChanged: (() -> Void)?
    private var connected = false
    private var started = false
    private var stopping = false
    private var finishing = false
    private var replies: [IncomingEnvelope] = []
    private var requests: [UInt64: (WindowAction, Bool)] = [:]
    private var requestOwners: [UInt64: UInt64] = [:]
    private var preferenceObservation: AnyCancellable?
    private var sessionPreferenceObservation: AnyCancellable?
    private var preferenceTask: Task<Void, Never>?
    private var sentPreferences: DisplayPreferences?
    var processID: Int32? { session.processID }

    init(appearance: BrowserAppearance? = nil, contextDefaults: UserDefaults? = nil, downloadConfiguration: DownloadConfiguration? = nil) {
        self.appearance = appearance ?? BrowserAppearance()
        self.downloads = BrowserDownloadsModel(browser: session,configuration: downloadConfiguration ?? .configured())
        self.contextPreferences = BrowserContextPreferences(defaults: contextDefaults)
        self.sessionPreferences = BrowserSessionPreferences(defaults: contextDefaults)
        captureArmed = self.sessionPreferences.saved == nil
        models[1] = BrowserModel(appearance: self.appearance, workspace: self, windowID: 1)
        sessionPreferenceObservation = self.sessionPreferences.objectWillChange.sink { [weak self] _ in
            Task { @MainActor in self?.objectWillChange.send() }
        }
        preferenceObservation = self.appearance.$resolved.removeDuplicates().sink { [weak self] _ in
            Task { @MainActor in self?.synchronizePreferences() }
        }
    }
    func start(launcher: URL? = nil) async {
        guard !started else { return }; started = true
        session.received = { [weak self, directory = session.frameDirectory] result in
            do {
                let envelope = try result.get()
                var pixels: FramePixels?
                if case .frame(let notice) = envelope.message {
                    do { pixels = try FramePixels.read(notice, directory: directory) }
                    catch let error as NSError where error.domain == NSCocoaErrorDomain && error.code == NSFileReadNoSuchFileError { return }
                }
                let frame = pixels
                DispatchQueue.main.async { [weak self] in self?.receive(envelope, pixels: frame) }
            } catch {
                let message = error.localizedDescription
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.connected = false; self.canManageWindows = false; self.canPlaceTabs = false; self.tabDragging.cancel()
                    for model in self.models.values { model.workspaceFailed(message) }
                    Task { await self.downloads.stop(); await self.session.stop() }
                }
            }
        }
        do {
            try await session.startForBrowser(launcher: launcher)
            connected = true
            for model in models.values { model.prepareManagedSession(true) }
            guard await contextCommand(.list) != nil else { throw BrowserFailure.invalid(notice ?? "Browser context state unavailable") }
            let saved = contextPreferences.profiles
            if let first = saved.first, first.name != "Default" { _ = await contextCommand(.rename(1, first.name)) }
            for profile in saved.dropFirst() {
                pendingProfileKey = profile.id
                guard await contextCommand(.create(profile.name)) != nil else { pendingProfileKey = nil; throw BrowserFailure.invalid(notice ?? "Cannot reopen a saved profile") }
                pendingProfileKey = nil
            }
            restoringProfiles = false
            canManageContexts = contextPreferences.error == nil
            if let error = contextPreferences.error { notice = error }
            guard await command(.list) != nil else { throw BrowserFailure.invalid(notice ?? "Browser window state unavailable") }
            sessionReady = true
            if sessionPreferences.remember, sessionPreferences.reopen, sessionPreferences.saved != nil {
                await restoreSavedSession()
                if !sessionRestored { await models[1]?.openInitialPage() }
            } else { await models[1]?.openInitialPage() }
        } catch {
            connected = false; canManageWindows = false; canPlaceTabs = false; tabDragging.cancel()
            for model in models.values { model.workspaceFailed(error.localizedDescription) }
            await downloads.stop()
            await session.stop()
        }
    }
    func send(_ command: BrowserCommand, tab: UInt64? = nil, owner: BrowserModel? = nil, willSend: (@Sendable (UInt64) -> Void)? = nil) async throws -> UInt64 {
        guard connected, !stopping else { throw BrowserFailure.invalid("Browser workspace is closed") }
        let window = owner?.windowID
        let scopedWindow = window.flatMap { id in tab.map { _ in BrowserCommand.window(.command(id, command)) } } ?? command
        let outbound = owner.map { BrowserCommand.browserContext(.command($0.contextID, scopedWindow)) } ?? scopedWindow
        let assistantCommand: Bool
        switch command {
        case .assistantPage, .unit("GetTranslationState"), .values("SetTranslationLanguage", _): assistantCommand = true
        default: assistantCommand = false
        }
        return try await session.send(outbound, tab: tab) { [weak self] request in
            if assistantCommand {
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.assistantRequests.insert(request)
                    if self.assistantRequests.count > 512, let oldest = self.assistantRequests.min() { self.assistantRequests.remove(oldest) }
                }
            }
            if let window {
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.requestOwners[request] = window
                    if self.requestOwners.count > 512, let first = self.requestOwners.keys.min() { self.requestOwners.removeValue(forKey: first) }
                }
            }
            willSend?(request)
        }
    }
    private var assistantRequests: Set<UInt64> = []
    func isAssistantRequest(_ id: UInt64) -> Bool { assistantRequests.contains(id) }
    func refreshAssistantTranslation() {
        Task { for model in models.values where model.assistantPresented { model.refreshAssistantTranslation() } }
    }
    private func receive(_ envelope: IncomingEnvelope, pixels: FramePixels?) {
        guard !stopping else { return }
        if let request = envelope.requestID, let navigation = sessionRequests[request] {
            let terminal: Bool
            switch envelope.message {
            case .navigationSessionState, .sessionUnavailable, .error, .blocked: terminal = true
            case .navigated: terminal = navigation
            default: terminal = false
            }
            if terminal {
                sessionRequests.removeValue(forKey: request)
                if sessionReplies.count < 512 { sessionReplies.append(envelope) }
                if !navigation {
                    if case .blocked = envelope.message {} else { return }
                }
            }
        }
        if let request = envelope.requestID, assistantRequests.contains(request) {
            switch envelope.message {
            case .assistantResult, .assistantUnavailable, .translation, .translationUnavailable, .error:
                for model in models.values { model.apply(envelope, pixels: pixels) }
                assistantRequests.remove(request); return
            default: break
            }
        }
        if case .navigated(let url) = envelope.message, url == "about:downloads" || url.hasPrefix("about:downloads?") {
            Task { await downloads.connect() }
        }
        if let request = envelope.requestID, contextRequests.remove(request) != nil {
            if contextReplies.count < 64 { contextReplies.append(envelope) }
            if case .error(let message) = envelope.message { notice = message; return }
        }
        let owned = envelope.requestID.flatMap { requests.removeValue(forKey: $0) }
        if let owned {
            if owned.1, replies.count < 64 { replies.append(envelope) }
            if case .error(let message) = envelope.message {
                if case .resize(let id, _) = owned.0, !windows.contains(where: { $0.id == id }) { return }
                notice = message; return
            }
        }
        switch envelope.message {
        case .browserContexts(let state):
            contexts = state.contexts; canManageContexts = !restoringProfiles && contextPreferences.error == nil
            if case .created(let id) = state.event { contextKeys[id] = pendingProfileKey ?? UUID() }
            for model in models.values {
                if let context = contextForWindow(model.windowID) { model.updateContext(context) }
            }
            if !restoringProfiles { persistProfiles() }
            onContextsChanged?()
        case .contextsUnavailable:
            canManageContexts = false; canManageWindows = false; canPlaceTabs = false; tabDragging.cancel(); notice = "Browser context state unavailable"
            for model in models.values { model.workspaceFailed("Browser context state unavailable") }
            return
        case .windowState(let state):
            guard Set(state.windows.map(\.id)) == Set(contexts.flatMap(\.windows)), state.windows.allSatisfy({ window in
                guard let context = contextForWindow(window.id) else { return false }
                let groups = Set(context.groups.map(\.id))
                return window.tabs.allSatisfy { $0.groupID.map(groups.contains) != false }
            }) else {
                canManageWindows = false; canPlaceTabs = false; tabDragging.cancel(); canManageContexts = false; notice = "Browser context ownership unavailable"
                for model in models.values { model.workspaceFailed("Browser context ownership unavailable") }
                return
            }
            let transfer: (UInt64, UInt64, UInt64)?
            switch state.event {
            case .moved(let tab, let from, let to), .placed(let tab, let from, let to): transfer = (tab, from, to)
            default: transfer = nil
            }
            if let (tab, from, to) = transfer, from != to, let source = models[from], let destination = models[to] {
                source.transferLocalTabState(tab, to: destination)
            }
            windows = state.windows; canManageWindows = true; canPlaceTabs = state.tabPlacement == true
            if !canPlaceTabs { tabDragging.cancel() }
            let ids = Set(windows.map(\.id))
            windowKeys = windowKeys.filter { ids.contains($0.key) }
            let tabs = Set(windows.flatMap(\.tabs).map(\.id))
            unrestoredHistory = unrestoredHistory.filter { tabs.contains($0.key) }
            for id in Array(models.keys).filter({ !ids.contains($0) }) {
                if let model = models.removeValue(forKey: id) { Task { await model.stop() } }
                onWindowClosed?(id)
            }
            for window in windows {
                let created = models[window.id] == nil
                if created { models[window.id] = BrowserModel(appearance: appearance, workspace: self, windowID: window.id) }
                guard let model = models[window.id] else { continue }
                if let context = contextForWindow(window.id) { model.updateContext(context) }
                model.prepareManagedSession(connected)
                model.updateWindowTabs(window.tabs)
                if created {
                    onWindowCreated?(model)
                    Task { _ = try? await self.send(.unit("ListTabGroups"), owner: model) }
                }
            }
            if case .moved(let tab, _, let destination) = state.event { models[destination]?.select(tab) }
            if case .placed(let tab, let source, let destination) = state.event, source != destination { models[destination]?.select(tab) }
            synchronizePreferences()
        case .windowsUnavailable:
            canManageWindows = false; canPlaceTabs = false; tabDragging.cancel(); notice = "Browser window state unavailable"
            for model in models.values { model.workspaceFailed("Browser window state unavailable") }
        case .error:
            if let request = envelope.requestID, let owner = requestOwners[request] {
                models[owner]?.apply(envelope, pixels: pixels); return
            }
        default: break
        }
        for model in models.values { model.apply(envelope, pixels: pixels) }
        switch envelope.message {
        case .navigated(let url):
            if url != "about:credits" { captureArmed = true }
            scheduleSessionSave(arm: false)
        case .windowState(let state):
            switch state.event {
            case .snapshot, .resized: scheduleSessionSave(arm: false)
            default: scheduleSessionSave()
            }
        case .browserContexts(let state):
            if case .snapshot = state.event { scheduleSessionSave(arm: false) }
            else { scheduleSessionSave() }
        case .groupChanged, .groupAssigned, .groupClosed: scheduleSessionSave()
        case .viewportState:
            scheduleSessionSave(arm: false)
        default: break
        }
    }
    private func command(_ action: WindowAction, tab: UInt64? = nil, wait: Bool = true) async -> BrowserWindowState? {
        guard connected, !stopping else { return nil }
        do {
            let request = try await send(.window(action), tab: tab) { [weak self] id in
                DispatchQueue.main.async { [weak self] in self?.requests[id] = (action, wait) }
            }
            if !wait { return nil }
            let deadline = Date().addingTimeInterval(5)
            while connected, !stopping, !Task.isCancelled, Date() < deadline {
                if let index = replies.firstIndex(where: { reply in
                    guard reply.requestID == request else { return false }
                    if case .open = action { return true }
                    return reply.tabID == nil
                }) {
                    let reply = replies.remove(at: index)
                    let message = reply.message
                    if case .open(let window, _) = action, case .opened(let tab, _) = message,
                       reply.tabID == tab, windows.contains(where: { $0.id == window && $0.tabs.contains(where: { $0.id == tab }) }) {
                        return BrowserWindowState(windows: windows, event: .opened(tab, window))
                    }
                    if case .windowState(let state) = message { return state }
                    return nil
                }
                try? await Task.sleep(for: .milliseconds(10))
            }
            notice = "The browser window action did not complete"
        } catch { notice = error.localizedDescription }
        return nil
    }
    func refreshWindows() async { if await command(.list) != nil { notice = nil } }
    func dismissNotice() { notice = nil }
    func resizeWindow(_ id: UInt64, viewport: WindowViewport) async {
        guard canManageWindows, windows.contains(where: { $0.id == id }), viewport.valid else { return }
        _ = await command(.resize(id, viewport), wait: false)
    }
    func createWindow(openTab: Bool = true, contextID: UInt64? = nil, viewport: WindowViewport = WindowViewport(width: 900, height: 600, deviceScale: 1, backingScale: 1)) async -> UInt64? {
        guard canManageWindows, !busy else { return nil }; busy = true; defer { busy = false }
        notice = nil
        let context = contextID ?? contextForWindow(activeWindowID)?.id ?? 1
        guard let state = await command(.createInContext(context, viewport)), case .created(let id) = state.event else { return nil }
        if openTab { _ = try? await send(.window(.open(id, "about:credits"))) }
        onActivateWindow?(id)
        return id
    }
    func moveTab(_ tab: UInt64, to window: UInt64) async -> Bool {
        guard canManageWindows, !busy else { return false }; busy = true; defer { busy = false }
        notice = nil
        guard let state = await command(.move(window), tab: tab), case .moved(let moved, _, let destination) = state.event, moved == tab, destination == window else { return false }
        onActivateWindow?(window); return true
    }
    func placeTab(_ placement: BrowserTabPlacement) async -> Bool {
        guard canPlaceTabs, !busy, !restoringSession, placement.valid(windows: windows, contexts: contexts) else { return false }
        busy = true; defer { busy = false }
        notice = nil
        guard let state = await command(.place(placement.source, placement.destination, placement.before, placement.group), tab: placement.tab),
              case .placed(let tab, let source, let destination) = state.event,
              tab == placement.tab, source == placement.source, destination == placement.destination else { return false }
        if source != destination { onActivateWindow?(destination) }
        return true
    }
    func tabStep(_ tab: UInt64, in window: UInt64, step: Int) -> BrowserTabPlacement? {
        guard canPlaceTabs, !busy, !restoringSession, let state = windows.first(where: { $0.id == window }),
              let context = contextForWindow(window), step == -1 || step == 1 else { return nil }
        let ordered = state.tabs.filter { $0.groupID == nil } + context.groups.flatMap { group in state.tabs.filter { $0.groupID == group.id } }
        guard let index = ordered.firstIndex(where: { $0.id == tab }), ordered.indices.contains(index + step) else { return nil }
        return BrowserTabPlacement.resolve(tab: tab, source: window, destination: window, target: .tab(ordered[index + step].id),
            after: step > 0, windows: windows, contexts: contexts)
    }
    func moveTabToNewWindow(_ tab: UInt64) async {
        guard let source = windows.first(where: { $0.tabs.contains { $0.id == tab } }), let context = contextForWindow(source.id),
              let id = await createWindow(openTab: false, contextID: context.id) else { return }
        _ = await moveTab(tab, to: id)
    }
    func closeWindow(_ id: UInt64) async -> Bool {
        while busy, connected, !stopping, !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(10))
        }
        guard canManageWindows, !stopping, !Task.isCancelled, windows.contains(where: { $0.id == id }) else { return false }
        busy = true; defer { busy = false }
        guard let state = await command(.close(id)), case .closed(let closed) = state.event else { return false }
        return closed == id
    }
    func activateWindow(_ id: UInt64) { if windows.contains(where: { $0.id == id }) { onActivateWindow?(id) } }
    func contextForWindow(_ id: UInt64) -> BrowserContextSummary? { contexts.first { $0.windows.contains(id) } }
    private func persistProfiles() {
        guard contextPreferences.error == nil else { return }
        for context in contexts where contextKeys[context.id] == nil { contextKeys[context.id] = UUID() }
        let profiles = contexts.map { BrowserContextPreferences.Profile(id: contextKeys[$0.id]!, name: $0.name) }
        do {
            if profiles != contextPreferences.profiles { try contextPreferences.save(profiles) }
            contextKeys = contextKeys.filter { id, _ in contexts.contains { $0.id == id } }
        } catch { notice = error.localizedDescription; canManageContexts = false }
    }
    private func contextCommand(_ action: ContextAction) async -> BrowserContextState? {
        guard connected, !stopping else { return nil }
        do {
            let request = try await send(.browserContext(action)) { [weak self] id in
                DispatchQueue.main.async { [weak self] in self?.contextRequests.insert(id) }
            }
            let deadline = Date().addingTimeInterval(5)
            while connected, !stopping, !Task.isCancelled, Date() < deadline {
                if let index = contextReplies.firstIndex(where: { $0.requestID == request && $0.tabID == nil }) {
                    if case .browserContexts(let state) = contextReplies.remove(at: index).message { return state }
                    return nil
                }
                try? await Task.sleep(for: .milliseconds(10))
            }
            notice = "The browser profile action did not complete"
        } catch { notice = error.localizedDescription }
        return nil
    }
    func createContext(_ name: String) async -> UInt64? {
        guard canManageContexts, !busy else { return nil }; busy = true; defer { busy = false }
        pendingProfileKey = UUID(); defer { pendingProfileKey = nil }
        guard let state = await contextCommand(.create(name)), case .created(let id) = state.event else { return nil }
        notice = nil; return id
    }
    func renameContext(_ id: UInt64, name: String) async -> Bool {
        guard canManageContexts, !busy else { return false }; busy = true; defer { busy = false }
        guard let state = await contextCommand(.rename(id, name)), case .renamed(let renamed) = state.event else { return false }
        notice = nil; return renamed == id
    }
    func closeContext(_ id: UInt64) async -> Bool {
        guard canManageContexts, !busy, id != 1 else { return false }; busy = true; defer { busy = false }
        guard let state = await contextCommand(.close(id)), case .closed(let closed) = state.event else { return false }
        notice = nil; return closed == id
    }
    func refreshContexts() async { if await contextCommand(.list) != nil { await refreshWindows() } }
    func synchronizePreferences() {
        guard connected, !stopping, preferenceTask == nil else { return }
        preferenceTask = Task {
            defer { preferenceTask = nil }
            while connected, !stopping, !Task.isCancelled, sentPreferences != appearance.resolved {
                guard let tab = windows.flatMap(\.tabs).first?.id else { return }
                let preferences = appearance.resolved
                do { _ = try await send(.displayPreferences(preferences), tab: tab); sentPreferences = preferences }
                catch { notice = error.localizedDescription; return }
            }
        }
    }
    func stop() async {
        guard !finishing, !stopping else { return }; finishing = true
        saveTask?.cancel(); await saveTask?.value; saveTask = nil
        if sessionReady, captureArmed, !restoringSession { await saveSession() }
        guard !stopping else { return }; stopping = true; connected = false; canManageWindows = false; canPlaceTabs = false; tabDragging.cancel()
        canManageContexts = false; contextRequests = []; contextReplies = []
        assistantRequests.removeAll()
        sessionRequests = [:]; sessionReplies = []
        preferenceTask?.cancel(); replies = []; requests = [:]; requestOwners = [:]
        for model in models.values { await model.stop() }
        await downloads.stop()
        await session.stop()
    }

    var canRestoreSession: Bool {
        sessionReady && connected && canManageWindows && canManageContexts && !busy && !restoringSession && !sessionRestored && sessionPreferences.error == nil && sessionPreferences.saved != nil && windows.count == 1 && models[windows[0].id].map { $0.tabs.count <= 1 && $0.tabs.allSatisfy { $0.url == nil || $0.url == "about:credits" } } == true
    }
    func setRememberSession(_ value: Bool) {
        sessionPreferences.setRemember(value)
        if value { scheduleSessionSave() }
        else { saveRevision &+= 1; saveTask?.cancel() }
    }
    func scheduleSessionSave(arm: Bool = true) {
        guard sessionReady, !restoringSession, !stopping, !finishing else { return }
        if arm { captureArmed = true }
        saveRevision &+= 1
        guard captureArmed, sessionPreferences.remember, sessionPreferences.error == nil, saveTask == nil else { return }
        saveTask = Task {
            defer {
                saveTask = nil
                if Task.isCancelled, sessionPreferences.remember { scheduleSessionSave(arm: false) }
            }
            while !Task.isCancelled, connected, !stopping, sessionPreferences.remember {
                let revision = saveRevision
                try? await Task.sleep(for: .milliseconds(300))
                guard !Task.isCancelled else { return }
                if revision != saveRevision { continue }
                await saveSession()
                if revision == saveRevision { return }
            }
        }
    }
    private func sessionCall(_ message: BrowserCommand, tab: UInt64, owner: BrowserModel, navigation: Bool = false) async -> BrowserMessage? {
        do {
            let request = try await send(message, tab: tab, owner: owner) { [weak self] id in
                DispatchQueue.main.async { [weak self] in self?.sessionRequests[id] = navigation }
            }
            defer { sessionRequests.removeValue(forKey: request); sessionReplies.removeAll { $0.requestID == request } }
            let deadline = Date().addingTimeInterval(navigation ? 15 : 5)
            while connected, !stopping, !Task.isCancelled, Date() < deadline {
                if let index = sessionReplies.firstIndex(where: { $0.requestID == request && $0.tabID == tab }) {
                    return sessionReplies.remove(at: index).message
                }
                try? await Task.sleep(for: .milliseconds(10))
            }
            if !Task.isCancelled { sessionPreferences.status = "A session action did not complete." }
        } catch { sessionPreferences.status = error.localizedDescription }
        return nil
    }
    private func inspectSession(_ tab: UInt64, model: BrowserModel) async -> NavigationSessionState? {
        guard case .navigationSessionState(let state) = await sessionCall(.navigationSession(.inspect), tab: tab, owner: model),
              state.context.tab_id == tab else { return nil }
        return state
    }
    private func saveSession() async {
        guard connected, !stopping, !restoringSession, sessionPreferences.remember, sessionPreferences.error == nil, !windows.isEmpty else { return }
        let revision = saveRevision
        let snapshot = windows
        var profiles: [SavedBrowserSession.Profile] = []
        for context in contexts {
            guard let key = contextKeys[context.id] else { return }
            let groups = context.groups.map { SavedBrowserSession.Group(name: $0.name, color: $0.color, collapsed: $0.collapsed) }
            var savedWindows: [SavedBrowserSession.Window] = []
            for window in snapshot where context.windows.contains(window.id) {
                guard let model = models[window.id] else { return }
                var tabs: [SavedBrowserSession.Tab] = []
                for tab in window.tabs {
                    guard let state = await inspectSession(tab.id, model: model), !Task.isCancelled else {
                        sessionPreferences.status = "Could not save every tab. The previous session was kept."; return
                    }
                    let history: NavigationHistory
                    if state.history.current.url == nil, let previous = unrestoredHistory[tab.id] { history = previous }
                    else { history = state.history; unrestoredHistory.removeValue(forKey: tab.id) }
                    tabs.append(.init(history: history, group: tab.groupID.flatMap { id in context.groups.firstIndex { $0.id == id } }))
                }
                let windowKey = windowKeys[window.id] ?? UUID(); windowKeys[window.id] = windowKey
                savedWindows.append(.init(key: windowKey, tabs: tabs, selected: window.tabs.firstIndex { $0.id == model.selected }, frame: onCaptureWindowFrame?(window.id)))
            }
            profiles.append(.init(key: key, groups: groups, windows: savedWindows))
        }
        guard revision == saveRevision, !Task.isCancelled, !restoringSession, sessionPreferences.remember,
              let active = windowKeys[activeWindowID], snapshot.contains(where: { $0.id == activeWindowID }) else { return }
        do { try sessionPreferences.save(.init(version: 1, profiles: profiles, active: active)) }
        catch { sessionPreferences.status = error.localizedDescription }
    }
    func restoreSavedSession() async {
        guard canRestoreSession, let saved = sessionPreferences.saved else { return }
        guard saved.profiles.allSatisfy({ profile in contextKeys.values.contains(profile.key) }) else {
            sessionPreferences.status = "A saved profile is unavailable. The previous session was kept."; return
        }
        restoringSession = true
        saveTask?.cancel(); await saveTask?.value; saveTask = nil
        busy = true
        var complete = true
        var restoredWindows: [UUID: UInt64] = [:]
        defer {
            busy = false; restoringSession = false; sessionRestored = true; captureArmed = true
            sessionPreferences.status = complete ? "Session restored. Pages were reviewed again." : "Some pages could not be restored. Their saved URLs were kept."
            if complete { scheduleSessionSave(arm: false) }
        }
        let original = windows.map(\.id)
        for profile in saved.profiles {
            guard let context = contextKeys.first(where: { $0.value == profile.key })?.key else { complete = false; return }
            // A named profile can retain empty groups without an open window.
            if profile.windows.isEmpty, !profile.groups.isEmpty {
                guard let state = await command(.createInContext(context, .init(width: 900, height: 600, deviceScale: 1, backingScale: 1))),
                      case .created(let window) = state.event, let model = models[window] else { complete = false; return }
                for group in profile.groups {
                    guard let id = await model.createTabGroup(name: group.name, color: group.color, tab: nil) else { complete = false; return }
                    if group.collapsed { _ = await model.setTabGroupCollapsed(id, collapsed: true) }
                }
                _ = await command(.close(window))
            }
            var groupIDs: [UInt64] = []
            for savedWindow in profile.windows {
                guard let state = await command(.createInContext(context, .init(width: 900, height: 600, deviceScale: 1, backingScale: 1))),
                      case .created(let window) = state.event, let model = models[window] else { complete = false; return }
                restoredWindows[savedWindow.key] = window; windowKeys[window] = savedWindow.key
                if let frame = savedWindow.frame { onRestoreWindowFrame?(window, frame) }
                if groupIDs.isEmpty {
                    for group in profile.groups {
                        guard let id = await model.createTabGroup(name: group.name, color: group.color, tab: nil) else { complete = false; return }
                        groupIDs.append(id)
                    }
                }
                var tabIDs: [UInt64] = []
                for savedTab in savedWindow.tabs {
                    guard let state = await command(.open(window, nil)), case .opened(let tab, _) = state.event else { complete = false; return }
                    tabIDs.append(tab); model.select(tab)
                    var history = savedTab.history
                    if !history.current.was_post, let url = history.current.url {
                        model.address = url
                        guard case .navigated = await sessionCall(.values("Navigate", ["url": .string(url)]), tab: tab, owner: model, navigation: true) else {
                            unrestoredHistory[tab] = history; complete = false
                            if let group = savedTab.group { _ = await model.setTabGroup(groupIDs[group], tab: tab) }
                            continue
                        }
                    }
                    guard let live = await inspectSession(tab, model: model) else { complete = false; unrestoredHistory[tab] = history; continue }
                    if !history.current.was_post { history.entries[history.cursor] = live.history.current }
                    guard case .navigationSessionState = await sessionCall(.navigationSession(.restore(live.context, history)), tab: tab, owner: model) else { complete = false; unrestoredHistory[tab] = savedTab.history; continue }
                    if let group = savedTab.group, !(await model.setTabGroup(groupIDs[group], tab: tab)) { complete = false }
                    _ = try? await send(.unit("GetHistoryState"), tab: tab, owner: model)
                }
                if let selected = savedWindow.selected { model.select(tabIDs[selected]) }
            }
            if let window = profile.windows.first.flatMap({ restoredWindows[$0.key] }), let model = models[window] {
                for (index, group) in profile.groups.enumerated() where group.collapsed {
                    if !(await model.setTabGroupCollapsed(groupIDs[index], collapsed: true)) { complete = false }
                }
            }
        }
        for id in original { _ = await command(.close(id)) }
        if let active = restoredWindows[saved.active] { activeWindowID = active; onActivateWindow?(active) }
    }
}

struct BrowserWindowCommands: Commands {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var workspace: BrowserWorkspace
    var body: some Commands {
        CommandGroup(after: .newItem) {
            Button(BrowserStrings.text("New Window")) { Task { await workspace.createWindow() } }
                .keyboardShortcut("n", modifiers: .command).disabled(!workspace.canManageWindows || workspace.busy)
        }
        CommandGroup(after: .windowArrangement) {
            Divider()
            ForEach(workspace.windows) { window in
                Button(BrowserStrings.format("Window %llu", window.id)) { workspace.activateWindow(window.id) }
            }
        }
    }
}
struct TabWindowMenu: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var workspace: BrowserWorkspace
    let tab: UInt64
    let window: UInt64
    var body: some View {
        Menu(BrowserStrings.text("Move to Window")) {
            Button(BrowserStrings.text("New Window")) { Task { await workspace.moveTabToNewWindow(tab) } }
            ForEach(workspace.windows.filter { $0.id != window && workspace.contextForWindow($0.id)?.id == workspace.contextForWindow(window)?.id }) { destination in
                Button(BrowserStrings.format("Window %llu", destination.id)) { Task { await workspace.moveTab(tab, to: destination.id) } }
            }
        }.disabled(!workspace.canManageWindows || workspace.busy)
    }
}
struct BrowserWorkspaceNotice: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var workspace: BrowserWorkspace
    var body: some View {
        if workspace.restoringSession {
            HStack { ProgressView().controlSize(.small); Text(BrowserStrings.text("Restoring saved windows and tabs…")) }
                .font(.caption).padding(8).accessibilityIdentifier("session-progress")
            Divider()
        }
        if let notice = workspace.notice {
            HStack {
                Text(BrowserStrings.text(notice)).accessibilityIdentifier("window-notice")
                Spacer()
                if !workspace.canManageWindows { Button(BrowserStrings.text("Retry")) { Task { await workspace.refreshWindows() } }.accessibilityIdentifier("retry-windows").chromeFocusable("retry-windows", activate: { Task { await workspace.refreshWindows() } }) }
                Button { workspace.dismissNotice() } label: { Image(systemName: "xmark") }
                    .accessibilityLabel(BrowserStrings.text("Dismiss window notice")).accessibilityIdentifier("dismiss-window-notice")
                    .chromeFocusable("dismiss-window-notice", activate: { workspace.dismissNotice() })
            }.font(.caption).padding(.horizontal, 12).padding(.vertical, 6)
            Divider()
        }
    }
}
