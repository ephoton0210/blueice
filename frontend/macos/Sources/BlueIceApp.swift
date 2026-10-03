// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

@main
struct BlueIceApp: App {
    @NSApplicationDelegateAdaptor(BrowserAppDelegate.self) private var delegate
    var body: some Scene {
        Settings {
            VStack(spacing: 0) {
                BrowserAppearanceSettingsView(settings: delegate.model.appearance)
                BrowserSessionSettingsView(workspace: delegate.workspace, preferences: delegate.workspace.sessionPreferences)
                    .padding([.horizontal, .bottom], 24).frame(width: 460)
            }
        }
            .commands {
                BrowserActiveCommands(delegate: delegate)
                BrowserAppearanceCommands(settings: delegate.workspace.appearance)
                BrowserWindowCommands(workspace: delegate.workspace)
                BrowserSessionCommands(workspace: delegate.workspace)
            }
    }
}

struct BrowserActiveCommands: Commands {
    @ObservedObject var delegate: BrowserAppDelegate
    var body: some Commands {
        NativeEditingCommands(menu: delegate.editingMenu, model: delegate.model)
        BrowserTabGroupCommands(model: delegate.model)
        BrowserProfileCommands(model: delegate.model, workspace: delegate.workspace)
        BrowserDownloadsCommands(model: delegate.model)
        BrowserPrintingCommands(model: delegate.model)
        BrowserAssistantCommands(model: delegate.model)
    }
}

@MainActor
final class BrowserAppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate, ObservableObject {
    let workspace: BrowserWorkspace
    @Published private(set) var model: BrowserModel
    private var browserWindows: [UInt64: NSWindow] = [:]
    private var cascadePoint = NSPoint.zero
    private var terminating = false
    let editingMenu = NativeEditingMenu()
    override init() {
        let workspace = BrowserWorkspace()
        self.workspace = workspace
        model = workspace.models[1]!
        super.init()
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        workspace.onWindowCreated = { [weak self] model in self?.openWindow(model) }
        workspace.onWindowClosed = { [weak self] id in
            guard let self else { return }
            let window = self.browserWindows.removeValue(forKey: id)
            window?.delegate = nil; window?.close()
            if self.browserWindows.isEmpty { NSApp.terminate(nil) }
            else if self.model.windowID == id, let next = self.browserWindows.keys.sorted().first, let model = self.workspace.models[next] {
                self.model = model; self.browserWindows[next]?.makeKeyAndOrderFront(nil)
            }
        }
        workspace.onActivateWindow = { [weak self] id in self?.browserWindows[id]?.makeKeyAndOrderFront(nil) }
        workspace.onCaptureWindowFrame = { [weak self] id in
            guard let frame = self?.browserWindows[id]?.frame else { return nil }
            return .init(x: frame.origin.x, y: frame.origin.y, width: frame.width, height: frame.height)
        }
        workspace.onRestoreWindowFrame = { [weak self] id, saved in
            guard let window = self?.browserWindows[id] else { return }
            let rect = NSRect(x: saved.x, y: saved.y, width: saved.width, height: saved.height)
            let screen = NSScreen.screens.max { left, right in
                let a = left.visibleFrame.intersection(rect), b = right.visibleFrame.intersection(rect)
                return (a.isNull ? 0 : a.width * a.height) < (b.isNull ? 0 : b.width * b.height)
            }
            window.setFrame(window.constrainFrameRect(rect, to: screen ?? NSScreen.main), display: true)
        }
        workspace.onContextsChanged = { [weak self] in
            guard let self else { return }
            for (id, window) in self.browserWindows { if let model = self.workspace.models[id] { window.title = self.title(model) } }
        }
        openWindow(model)
        NSApp.activate(ignoringOtherApps: true)
        Task { await workspace.start() }
    }
    private func openWindow(_ model: BrowserModel) {
        guard browserWindows[model.windowID] == nil else { return }
        let initial = model.windowID == 1
        let window = BrowserWindow(contentRect: NSRect(x: 0, y: 0, width: initial ? 1120 : 900, height: initial ? 800 : 650),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = title(model)
        window.identifier = NSUserInterfaceItemIdentifier(initial ? "browser-window" : "browser-window-\(model.windowID)")
        window.isReleasedWhenClosed = false; window.collectionBehavior = [.fullScreenPrimary]; window.delegate = self
        window.contentView = NSHostingView(rootView: BrowserView(model: model).environmentObject(editingMenu))
        window.center()
        if initial { cascadePoint = NSPoint(x: window.frame.minX + 32, y: window.frame.maxY - 32) }
        else { cascadePoint = window.cascadeTopLeft(from: cascadePoint) }
        browserWindows[model.windowID] = window
        window.makeKeyAndOrderFront(nil)
    }
    func windowDidBecomeKey(_ notification: Notification) {
        guard let window = notification.object as? NSWindow,
              let id = browserWindows.first(where: { $0.value === window })?.key,
              let active = workspace.models[id] else { return }
        if model !== active { model = active }
        if workspace.activeWindowID != id { workspace.activeWindowID = id }
    }
    func windowDidMove(_ notification: Notification) { workspace.scheduleSessionSave() }
    func windowDidResize(_ notification: Notification) { workspace.scheduleSessionSave() }
    private func title(_ model: BrowserModel) -> String {
        let profile = model.profileName == "Default" ? "BlueIce" : "BlueIce · \(model.profileName)"
        return model.windowID == 1 ? profile : "\(profile) · Window \(model.windowID)"
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if browserWindows.count <= 1 { NSApp.terminate(nil); return false }
        guard let id = browserWindows.first(where: { $0.value === sender })?.key else { return true }
        Task { await workspace.closeWindow(id) }
        return false
    }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !terminating else { return .terminateLater }
        terminating = true
        Task { await workspace.stop(); sender.reply(toApplicationShouldTerminate: true) }
        return .terminateLater
    }
}
