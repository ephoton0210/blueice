// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

@main
struct BlueIceApp: App {
    @NSApplicationDelegateAdaptor(BrowserAppDelegate.self) private var delegate
    var body: some Scene {
        Settings { Text("Browser settings are available from the toolbar.").padding() }
    }
}

@MainActor
final class BrowserAppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private let model = BrowserModel()
    private var browserWindow: NSWindow?
    private var terminating = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1120, height: 800),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "BlueIce"
        window.identifier = NSUserInterfaceItemIdentifier("browser-window")
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.contentView = NSHostingView(rootView: BrowserView(model: model))
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
