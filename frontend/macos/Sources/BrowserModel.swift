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
    @Published private(set) var ready = false
    @Published private(set) var image: CGImage?
    @Published private(set) var generation: UInt64 = 0
    @Published private(set) var history = HistoryState()
    @Published private(set) var representation: PageRepresentation?
    @Published private(set) var accessibilityEpoch: UInt64 = 0
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
                    self.status = message; self.ready = false; self.representation = nil
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
            frames = frames.filter { live.contains($0.key) }
            histories = histories.filter { live.contains($0.key) }
            representations = representations.filter { live.contains($0.key) }
            documentEpochs = documentEpochs.filter { live.contains($0.key) }
            representationRequests = representationRequests.filter { live.contains($0.key) }
            if selected == nil || !live.contains(selected!) { selected = list.first?.id }
            showSelected()
        case .opened(let id):
            selected = id
            Task { await send(.unit("ListTabs")); await navigate("about:credits", tab: id) }
        case .closed:
            Task { await send(.unit("ListTabs")) }
        case .navigated(let url):
            if let tab {
                documentEpochs[tab, default: 0] += 1
                representations.removeValue(forKey: tab)
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
        case .blocked(let reason):
            if tab == selected { status = "Navigation blocked: " + reason }
        case .error(let message):
            if tab == nil || tab == selected { status = message }
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
    }

    func select(_ tab: UInt64) {
        selected = tab
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
        action(.values("Click", ["x": .number(visible.midX), "y": .number(visible.midY)]), tab: snapshot.tabID)
        return true
    }

    func navigateAddress() {
        var url = address.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !url.isEmpty, let selected else { return }
        if !url.contains(":") { url = "https://" + url }
        Task { await navigate(url, tab: selected) }
    }

    func reload() {
        guard let selected, let url = tabs.first(where: { $0.id == selected })?.url else { return }
        Task { await navigate(url, tab: selected) }
    }

    private func navigate(_ url: String, tab: UInt64) async {
        if tab == selected { status = "Loading…" }
        await send(.values("Navigate", ["url": .string(url)]), tab: tab)
        await resize()
    }

    private func send(_ command: BrowserCommand, tab: UInt64? = nil) async {
        guard ready else { return }
        do { try await session.send(command, tab: tab) }
        catch { status = error.localizedDescription }
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

    func stop() async { ready = false; representation = nil; representations.removeAll(); representationRequests.removeAll(); resizeTask?.cancel(); await session.stop() }
}
