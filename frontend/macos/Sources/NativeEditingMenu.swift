// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Carbon
import Combine
import SwiftUI

@MainActor
final class NativeEditingMenu: ObservableObject {
    struct Source: Identifiable { let id: String; let name: String }
    @Published private(set) var sources: [Source] = []
    @Published private(set) var selected: String?
    private weak var view: CorePageView?
    private var selectionChanged: AnyCancellable?
    private var activeContext: NSTextInputContext? {
        (NSApp?.keyWindow?.firstResponder as? NSView)?.inputContext ?? view?.inputContext
    }

    init() {
        selectionChanged = NotificationCenter.default.publisher(for: NSTextInputContext.keyboardSelectionDidChangeNotification)
            .receive(on: RunLoop.main).sink { [weak self] _ in Task { @MainActor in self?.refresh() } }
        refresh()
    }
    func focus(_ view: CorePageView?) { self.view = view; refresh() }
    private func refresh() {
        let context = activeContext
        let filter: [String: Any] = [kTISPropertyInputSourceCategory as String: kTISCategoryKeyboardInputSource as String,
                                     kTISPropertyInputSourceIsSelectCapable as String: true,
                                     kTISPropertyInputSourceIsEnabled as String: true]
        let available = TISCreateInputSourceList(filter as CFDictionary, false).takeRetainedValue() as! [TISInputSource]
        let identifiers = available.compactMap { source -> String? in
            guard let value = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else { return nil }
            return Unmanaged<CFString>.fromOpaque(value).takeUnretainedValue() as String
        }
        sources = (context?.keyboardInputSources ?? identifiers).compactMap { source in
            NSTextInputContext.localizedName(forInputSource: source).map { Source(id: source, name: $0) }
        }
        selected = context?.selectedKeyboardInputSource
    }
    func choose(_ source: String) {
        guard let context = activeContext, sources.contains(where: { $0.id == source }) else { return }
        context.selectedKeyboardInputSource = source
        refresh()
    }
}

struct NativeEditingCommands: Commands {
    @ObservedObject var menu: NativeEditingMenu
    @ObservedObject var model: BrowserModel
    var body: some Commands {
        CommandGroup(after: .toolbar) {
            Button("Open Location…") { model.requestAddressFocus() }
                .keyboardShortcut("l", modifiers: .command).disabled(!model.ready)
        }
        CommandGroup(after: .textEditing) {
            Menu("Find") {
                Button("Find in Page…") { model.showFind() }
                    .keyboardShortcut("f", modifiers: .command).disabled(!model.ready || model.selected == nil)
                Button("Find Next") { model.findNext() }
                    .keyboardShortcut("g", modifiers: .command).disabled(!model.ready || model.selected == nil)
                Button("Find Previous") { model.findNext(backwards: true) }
                    .keyboardShortcut("g", modifiers: [.command, .shift]).disabled(!model.ready || model.selected == nil)
            }
            Menu("Input Source") {
                ForEach(menu.sources) { source in
                    Button { menu.choose(source.id) } label: {
                        if menu.selected == source.id { Label(source.name, systemImage: "checkmark") }
                        else { Text(source.name) }
                    }.accessibilityIdentifier("input-source." + source.id)
                }
            }.disabled(menu.sources.isEmpty)
        }
    }
}
