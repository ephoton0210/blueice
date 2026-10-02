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
    private let session = BrowserSession()
    private var frames: [UInt64: FramePixels] = [:]
    private var histories: [UInt64: HistoryState] = [:]
    private var viewport = CGSize(width: 1024, height: 640)
    private var resizeTask: Task<Void, Never>?

    func start() async {
        let arguments = ProcessInfo.processInfo.arguments
        let executable: URL
        if let option = arguments.firstIndex(of: "--core-exe") {
            guard arguments.indices.contains(option + 1) else { status = "--core-exe requires a core executable path."; return }
            executable = URL(fileURLWithPath: arguments[option + 1])
        } else {
            executable = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/blueice-core")
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
                Task { @MainActor [weak self] in self?.status = message; self?.ready = false }
            }
        }
        do {
            try await session.start(executable: executable)
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
            if selected == nil || !live.contains(selected!) { selected = list.first?.id }
            showSelected()
        case .opened(let id):
            selected = id
            Task { await send(.unit("ListTabs")); await navigate("about:credits", tab: id) }
        case .closed:
            Task { await send(.unit("ListTabs")) }
        case .navigated(let url):
            if let id = tab, let index = tabs.firstIndex(where: { $0.id == id }) { tabs[index].url = url }
            if tab == selected { address = url; status = "Ready" }
            Task { await send(.unit("GetHistoryState"), tab: tab) }
        case .history(let value):
            if let tab { histories[tab] = value }
            if tab == selected { history = value }
        case .frame:
            guard let tab, let pixels, pixels.generation > (frames[tab]?.generation ?? 0) else { return }
            frames[tab] = pixels
            if tab == selected { showFrame(pixels) }
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

    func stop() async { ready = false; resizeTask?.cancel(); await session.stop() }
}
