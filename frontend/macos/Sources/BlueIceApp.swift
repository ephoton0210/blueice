// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

@main
struct BlueIceApp: App {
    @NSApplicationDelegateAdaptor(BrowserAppDelegate.self) private var delegate
    var body: some Scene {
        Settings { BrowserAppearanceSettingsView(settings: delegate.model.appearance) }
            .commands {
                NativeEditingCommands(menu: delegate.editingMenu, model: delegate.model)
                BrowserAppearanceCommands(settings: delegate.model.appearance)
                BrowserTabGroupCommands(model: delegate.model)
            }
    }
}

@MainActor
final class BrowserAppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    let model = BrowserModel()
    private var browserWindow: NSWindow?
    private var terminating = false
    let editingMenu = NativeEditingMenu()

    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = BrowserWindow(contentRect: NSRect(x: 0, y: 0, width: 1120, height: 800),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "BlueIce"
        window.identifier = NSUserInterfaceItemIdentifier("browser-window")
        window.isReleasedWhenClosed = false
        window.collectionBehavior = [.fullScreenPrimary]
        window.delegate = self
        window.contentView = NSHostingView(rootView: BrowserView(model: model).environmentObject(editingMenu))
        window.center()
        browserWindow = window
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        Task { await model.start() }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        NSApp.terminate(nil)
        return false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !terminating else { return .terminateLater }
        terminating = true
        Task { await model.stop(); sender.reply(toApplicationShouldTerminate: true) }
        return .terminateLater
    }
}
