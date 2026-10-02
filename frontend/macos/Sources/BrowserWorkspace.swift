// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine
import SwiftUI

@MainActor
final class BrowserWorkspace: ObservableObject {
    let session = BrowserSession()
    let appearance: BrowserAppearance
    @Published private(set) var windows: [BrowserWindowSummary] = []
    @Published private(set) var contexts: [BrowserContextSummary] = []
    @Published private(set) var canManageContexts = false
    @Published var activeWindowID: UInt64 = 1
    let contextPreferences: BrowserContextPreferences
    private var contextKeys: [UInt64: UUID] = [1: BrowserContextPreferences.defaultKey]
    private var restoringProfiles = true
    private var pendingProfileKey: UUID?
    private var contextRequests: Set<UInt64> = []
    private var contextReplies: [IncomingEnvelope] = []
    @Published private(set) var canManageWindows = false
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
    private var replies: [IncomingEnvelope] = []
    private var requests: [UInt64: (WindowAction, Bool)] = [:]
    private var requestOwners: [UInt64: UInt64] = [:]
    private var preferenceObservation: AnyCancellable?
    private var preferenceTask: Task<Void, Never>?
    private var sentPreferences: DisplayPreferences?
    var processID: Int32? { session.processID }

    init(appearance: BrowserAppearance? = nil, contextDefaults: UserDefaults? = nil) {
        self.appearance = appearance ?? BrowserAppearance()
        self.contextPreferences = BrowserContextPreferences(defaults: contextDefaults)
        models[1] = BrowserModel(appearance: self.appearance, workspace: self, windowID: 1)
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
                    self.connected = false; self.canManageWindows = false
                    for model in self.models.values { model.workspaceFailed(message) }
                    Task { await self.session.stop() }
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
            await models[1]?.openInitialPage()
        } catch {
            connected = false; canManageWindows = false
            for model in models.values { model.workspaceFailed(error.localizedDescription) }
            await session.stop()
        }
    }
    func send(_ command: BrowserCommand, tab: UInt64? = nil, owner: BrowserModel? = nil, willSend: (@Sendable (UInt64) -> Void)? = nil) async throws -> UInt64 {
        guard connected, !stopping else { throw BrowserFailure.invalid("Browser workspace is closed") }
        let window = owner?.windowID
        let scopedWindow = window.flatMap { id in tab.map { _ in BrowserCommand.window(.command(id, command)) } } ?? command
        let outbound = owner.map { BrowserCommand.browserContext(.command($0.contextID, scopedWindow)) } ?? scopedWindow
        return try await session.send(outbound, tab: tab) { [weak self] request in
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
    private func receive(_ envelope: IncomingEnvelope, pixels: FramePixels?) {
        guard !stopping else { return }
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
            canManageContexts = false; canManageWindows = false; notice = "Browser context state unavailable"
            for model in models.values { model.workspaceFailed("Browser context state unavailable") }
            return
        case .windowState(let state):
            guard Set(state.windows.map(\.id)) == Set(contexts.flatMap(\.windows)), state.windows.allSatisfy({ window in
                guard let context = contextForWindow(window.id) else { return false }
                let groups = Set(context.groups.map(\.id))
                return window.tabs.allSatisfy { $0.groupID.map(groups.contains) != false }
            }) else {
                canManageWindows = false; canManageContexts = false; notice = "Browser context ownership unavailable"
                for model in models.values { model.workspaceFailed("Browser context ownership unavailable") }
                return
            }
            if case .moved(let tab, let from, let to) = state.event, from != to, let source = models[from], let destination = models[to] {
                source.transferLocalTabState(tab, to: destination)
            }
            windows = state.windows; canManageWindows = true
            let ids = Set(windows.map(\.id))
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
            synchronizePreferences()
        case .windowsUnavailable:
            canManageWindows = false; notice = "Browser window state unavailable"
            for model in models.values { model.workspaceFailed("Browser window state unavailable") }
        case .error:
            if let request = envelope.requestID, let owner = requestOwners[request] {
                models[owner]?.apply(envelope, pixels: pixels); return
            }
        default: break
        }
        for model in models.values { model.apply(envelope, pixels: pixels) }
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
                if let index = replies.firstIndex(where: { $0.requestID == request && $0.tabID == nil }) {
                    let message = replies.remove(at: index).message
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
        guard !stopping else { return }; stopping = true; connected = false; canManageWindows = false
        canManageContexts = false; contextRequests = []; contextReplies = []
        preferenceTask?.cancel(); replies = []; requests = [:]; requestOwners = [:]
        for model in models.values { await model.stop() }
        await session.stop()
    }
}

struct BrowserWindowCommands: Commands {
    @ObservedObject var workspace: BrowserWorkspace
    var body: some Commands {
        CommandGroup(after: .newItem) {
            Button("New Window") { Task { await workspace.createWindow() } }
                .keyboardShortcut("n", modifiers: .command).disabled(!workspace.canManageWindows || workspace.busy)
        }
        CommandGroup(after: .windowArrangement) {
            Divider()
            ForEach(workspace.windows) { window in
                Button("Window \(window.id)") { workspace.activateWindow(window.id) }
            }
        }
    }
}
struct TabWindowMenu: View {
    @ObservedObject var workspace: BrowserWorkspace
    let tab: UInt64
    let window: UInt64
    var body: some View {
        Menu("Move to Window") {
            Button("New Window") { Task { await workspace.moveTabToNewWindow(tab) } }
            ForEach(workspace.windows.filter { $0.id != window && workspace.contextForWindow($0.id)?.id == workspace.contextForWindow(window)?.id }) { destination in
                Button("Window \(destination.id)") { Task { await workspace.moveTab(tab, to: destination.id) } }
            }
        }.disabled(!workspace.canManageWindows || workspace.busy)
    }
}
struct BrowserWorkspaceNotice: View {
    @ObservedObject var workspace: BrowserWorkspace
    var body: some View {
        if let notice = workspace.notice {
            HStack {
                Text(notice).accessibilityIdentifier("window-notice")
                Spacer()
                if !workspace.canManageWindows { Button("Retry") { Task { await workspace.refreshWindows() } }.accessibilityIdentifier("retry-windows") }
                Button { workspace.dismissNotice() } label: { Image(systemName: "xmark") }
                    .accessibilityLabel("Dismiss window notice").accessibilityIdentifier("dismiss-window-notice")
            }.font(.caption).padding(.horizontal, 12).padding(.vertical, 6)
            Divider()
        }
    }
}
