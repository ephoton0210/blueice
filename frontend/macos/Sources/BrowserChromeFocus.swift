// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

struct BrowserChromeTargets: PreferenceKey {
    static var defaultValue: [String] { [] }
    static func reduce(value: inout [String], nextValue: () -> [String]) { value += nextValue() }
}

struct BrowserChromeIntent: Equatable {
    let serial: UInt64
    let target: String
}

private struct BrowserChromeBindingKey: EnvironmentKey {
    static var defaultValue: FocusState<String?>.Binding? { nil }
}
private struct BrowserChromeNavigationKey: EnvironmentKey {
    static var defaultValue: BrowserChromeFocus? { nil }
}

extension EnvironmentValues {
    var browserChromeBinding: FocusState<String?>.Binding? {
        get { self[BrowserChromeBindingKey.self] }
        set { self[BrowserChromeBindingKey.self] = newValue }
    }
    var browserChromeNavigation: BrowserChromeFocus? {
        get { self[BrowserChromeNavigationKey.self] }
        set { self[BrowserChromeNavigationKey.self] = newValue }
    }
}

private struct ChromeFocusable: ViewModifier {
    @Environment(\.browserChromeBinding) private var binding
    @Environment(\.isEnabled) private var enabled
    @Environment(\.browserChromeNavigation) private var navigation
    let id: String
    let field: Bool
    let activate: (() -> Void)?
    @ViewBuilder func body(content: Content) -> some View {
        Group {
            if let binding {
                if field {
                    content.focused(binding, equals: id)
                } else {
                    content.focusable(interactions: [.activate, .edit]).focused(binding, equals: id)
                        .focusEffectDisabled()
                        .onKeyPress(keys: [.space, .return], phases: .down) { key in
                            guard enabled, binding.wrappedValue == id,
                                  key.modifiers.intersection([.command, .control, .option, .shift]).isEmpty,
                                  let navigation, navigation.targets.contains(id), let window = navigation.window,
                                  window.isKeyWindow, window.attachedSheet == nil, let activate else { return .ignored }
                            return navigation.activate(id, action: activate) ? .handled : .ignored
                        }
                        .overlay {
                            if let navigation { ChromeFocusRing(navigation: navigation, binding: binding, id: id) }
                        }
                }
            } else { content }
        }.modifier(ChromeTarget(id: id))
    }
}

private struct ChromeTarget: ViewModifier {
    @Environment(\.isEnabled) private var enabled
    @Environment(\.browserChromeNavigation) private var navigation
    @State private var instance = UUID()
    let id: String
    func body(content: Content) -> some View {
        content.preference(key: BrowserChromeTargets.self, value: enabled ? [id] : [])
            .onAppear { navigation?.setAvailable(id, instance: instance, enabled: enabled) }
            .onChange(of: enabled) { _, value in navigation?.setAvailable(id, instance: instance, enabled: value) }
            .onDisappear { navigation?.setAvailable(id, instance: instance, enabled: nil) }
    }
}

extension View {
    func chromeFocusable(_ id: String, field: Bool = false, activate: (() -> Void)? = nil) -> some View {
        modifier(ChromeFocusable(id: id, field: field, activate: activate))
    }
    func chromeTarget(_ id: String) -> some View { modifier(ChromeTarget(id: id)) }
}

private struct ChromeFocusRing: View {
    @ObservedObject var navigation: BrowserChromeFocus
    let binding: FocusState<String?>.Binding
    let id: String
    var body: some View {
        RoundedRectangle(cornerRadius: 4)
            .strokeBorder(navigation.keyboardFocusVisible && binding.wrappedValue == id ? Color.accentColor : .clear, lineWidth: 2)
            .allowsHitTesting(false).accessibilityHidden(true)
    }
}

/// One native window's key loop. Targets come from enabled rendered controls,
/// not DOM semantics, accessibility labels or a parallel model of availability.
@MainActor
final class BrowserChromeFocus: ObservableObject {
    @Published var keyboardFocusVisible = false
    weak var window: BrowserWindow?
    weak var model: BrowserModel?
    weak var findField: NSSearchField?
    private(set) var targets: [String] = []
    private var renderedOrder: [String] = []
    private var availability: [String: Bool] = [:]
    private var instances: [String: [UUID: Bool]] = [:]
    var readFocus: (() -> String?)?
    var writeFocus: ((String?) -> Void)?
    private var focusTask: Task<Void, Never>?
    private var recoveryTask: Task<Void, Never>?
    private var pending: UUID?
    private var pendingKeys: [NSEvent] = []
    private var pendingTarget: String?
    private var ownerTab: UInt64?
    private var ownerEpoch: UInt64?
    private var ownerReady: Bool?
    private weak var inputModel: BrowserModel?
    private weak var inputWindow: BrowserWindow?
    private var lastTarget: String?
    private var commandSerial: UInt64?
    private var inputEpoch: UInt64 = 0
    var isSettled: Bool { pending == nil }

    func setAvailable(_ id: String, instance: UUID, enabled: Bool?) {
        if let enabled { instances[id, default: [:]][instance] = enabled }
        else {
            instances[id]?.removeValue(forKey: instance)
            if instances[id]?.isEmpty == true { instances.removeValue(forKey: id) }
        }
        let available = instances[id]?.values.contains(true) == true
        guard availability[id] != available else { return }
        availability[id] = available
        updateTargets(renderedOrder)
    }

    func updateTargets(_ incoming: [String]) {
        let old = targets
        let current = readFocus?() ?? pendingTarget ?? (keyboardFocusVisible ? lastTarget : nil)
        var seen: Set<String> = []
        renderedOrder = incoming.filter { seen.insert($0).inserted }
        // Native split panes can retain an old preference after a pane is
        // removed. Actual control lifecycle and enabled state fence that cache.
        targets = renderedOrder.filter { availability[$0] != false }
        guard targets != old else { return }
        guard let current, old.contains(current), !targets.contains(current),
              acceptsInput, window?.firstResponder !== window?.pageInput,
              let index = old.firstIndex(of: current), !targets.isEmpty else { return }
        // A removed control keeps its position in the loop. Prefer its next
        // surviving neighbor, including one before it when the loop wraps.
        let survivors = Array(old.dropFirst(index + 1)) + Array(old.prefix(index))
        let tab = model?.selected, epoch = model?.accessibilityEpoch, ready = model?.ready
        recoveryTask?.cancel()
        recoveryTask = Task { @MainActor [weak self, weak window, weak model] in
            await Task.yield()
            guard let self, let window, let model, !Task.isCancelled, self.acceptsInput,
                  self.window === window, self.model === model,
                  model.selected == tab, model.accessibilityEpoch == epoch, model.ready == ready,
                  self.lastTarget == current,
                  let next = survivors.first(where: self.targets.contains) ?? self.targets.first else { return }
            self.request(next, event: nil)
        }
    }

    func detach() {
        discardQueuedInput()
        window = nil; model = nil; findField = nil
        readFocus = nil; writeFocus = nil
        lastTarget = nil; commandSerial = nil; ownerTab = nil; ownerEpoch = nil; ownerReady = nil
        inputModel = nil; inputWindow = nil
        targets.removeAll(); renderedOrder.removeAll(); availability.removeAll(); instances.removeAll()
    }

    func cancelPending() {
        recoveryTask?.cancel(); recoveryTask = nil
        focusTask?.cancel(); focusTask = nil; pending = nil; pendingTarget = nil; pendingKeys.removeAll()
    }

    func discardQueuedInput() { inputEpoch &+= 1; cancelPending() }

    var acceptsInput: Bool {
        guard let window, let model else { return false }
        return window.pageInput?.model === model && window.isKeyWindow
            && window.attachedSheet == nil && !model.downloadsPresented && model.groupEditor == nil
            && model.profileEditor == nil && !model.resubmissionPresented && !model.printErrorPresented
            && !model.fileInputErrorPresented && !model.permissionsErrorPresented
    }

    private var ownsInput: Bool {
        guard let window, let model else { return false }
        return acceptsInput && inputModel === model && inputWindow === window
            && model.selected == ownerTab && model.accessibilityEpoch == ownerEpoch && model.ready == ownerReady
    }

    func activate(_ target: String, action: @escaping () -> Void) -> Bool {
        guard acceptsInput, pending == nil, targets.contains(target), readFocus?() == target,
              let window, let model else { return false }
        let token = UUID(), tab = model.selected, epoch = model.accessibilityEpoch, input = inputEpoch, ready = model.ready
        ownerTab = tab; ownerEpoch = epoch; ownerReady = ready
        inputModel = model; inputWindow = window
        pending = token; pendingTarget = target
        // SwiftUI can deliver onKeyPress while updating a focus region. The
        // original action runs after that transaction, with later keys held.
        focusTask = Task { @MainActor [weak self, weak window, weak model] in
            await Task.yield()
            guard let self, let window, let model, self.pending == token,
                  self.ownsInput, self.readFocus?() == target, self.targets.contains(target) else {
                if self?.pending == token { self?.cancelPending() }; return
            }
            let keys = self.pendingKeys; self.pendingKeys.removeAll()
            self.pending = nil; self.pendingTarget = nil; self.focusTask = nil
            action()
            for key in keys {
                guard self.window === window, self.model === model, self.acceptsInput,
                      self.inputEpoch == input, model.selected == tab, model.accessibilityEpoch == epoch,
                      model.ready == ready else { return }
                window.sendEvent(key)
            }
        }
        return true
    }

    func command(_ intent: BrowserChromeIntent) {
        guard let model, model.chromeFocusIntent == intent, commandSerial != intent.serial else { return }
        commandSerial = intent.serial
        request(intent.target, event: nil, selectText: true)
    }

    func buffer(_ event: NSEvent) -> Bool {
        guard pending != nil else { return false }
        guard ownsInput else { cancelPending(); return false }
        guard pendingKeys.count < 512 else { cancelPending(); return true }
        pendingKeys.append(event)
        return true
    }

    func tab(_ event: NSEvent) -> Bool {
        guard event.keyCode == 48,
              event.modifierFlags.intersection([.command, .control, .option]).isEmpty,
              let window, acceptsInput,
              window.firstResponder !== window.pageInput || model?.ready == false,
              (window.firstResponder as? NSTextView)?.hasMarkedText() != true else { return false }
        let current: String?
        if let field = findField, field.currentEditor() === window.firstResponder { current = "find-query" }
        else { current = readFocus?() }
        return move(from: current, backwards: event.modifierFlags.contains(.shift), event: event)
    }

    func leavePage(_ direction: FocusDirection) -> Bool {
        move(from: "page", backwards: direction == .backward, event: nil)
    }

    private func move(from current: String?, backwards: Bool, event: NSEvent?) -> Bool {
        guard !targets.isEmpty, let window, let model, window.pageInput?.model === model,
              acceptsInput, writeFocus != nil else { return false }
        let index = current.flatMap { targets.firstIndex(of: $0) }
        let next = index.map { ($0 + (backwards ? targets.count - 1 : 1)) % targets.count }
            ?? (backwards ? targets.count - 1 : 0)
        request(targets[next], event: event)
        return true
    }

    private func request(_ target: String, event: NSEvent?, selectText: Bool = false) {
        guard acceptsInput, let window, let model else { return }
        keyboardFocusVisible = true
        cancelPending()
        let token = UUID(), tab = model.selected, epoch = model.accessibilityEpoch, input = inputEpoch, ready = model.ready
        pending = token; pendingTarget = target; lastTarget = target
        ownerTab = tab; ownerEpoch = epoch; ownerReady = ready
        inputModel = model; inputWindow = window
        writeFocus?(target == "page" || target == "find-query" ? nil : target)
        focusTask = Task { @MainActor [weak self, weak window, weak model] in
            await Task.yield()
            guard let self else { return }
            guard let window, let model else {
                if self.pending == token { self.cancelPending() }; return
            }
            let deadline = Date().addingTimeInterval(2)
            while !Task.isCancelled, self.pending == token,
                  self.window === window, self.model === model,
                  self.acceptsInput, self.inputEpoch == input, model.selected == tab,
                  model.ready == ready, model.accessibilityEpoch == epoch, Date() < deadline {
                // Explicit Find can precede the SwiftUI transaction which
                // inserts its native search field and publishes its target.
                guard self.targets.contains(target) else {
                    try? await Task.sleep(for: .milliseconds(10)); continue
                }
                let focused: Bool
                if target == "page" {
                    if let event { focused = window.pageInput?.enterFromChrome(event) == true }
                    else { focused = window.makeFirstResponder(window.pageInput) }
                } else if target == "find-query" {
                    focused = self.findField?.window === window && window.makeFirstResponder(self.findField)
                } else {
                    focused = self.readFocus?() == target && window.firstResponder !== window.pageInput
                }
                if focused {
                    if selectText, target == "address" || target == "find-query" {
                        (window.firstResponder as? NSTextView)?.selectAll(nil)
                    }
                    self.pending = nil; self.pendingTarget = nil; self.focusTask = nil
                    let keys = self.pendingKeys; self.pendingKeys.removeAll()
                    window.pageInput?.refreshTextInput()
                    for key in keys {
                        // Each preceding key can open a sheet, transfer a tab,
                        // change the document or supersede this focus request.
                        guard self.window === window, self.model === model,
                              self.acceptsInput, self.inputEpoch == input,
                              model.selected == tab, model.accessibilityEpoch == epoch,
                              model.ready == ready else { return }
                        window.sendEvent(key)
                    }
                    return
                }
                try? await Task.sleep(for: .milliseconds(10))
            }
            if self.pending == token { self.cancelPending() }
        }
    }
}

struct BrowserChromeWindow: NSViewRepresentable {
    let navigation: BrowserChromeFocus
    let model: BrowserModel
    let binding: FocusState<String?>.Binding
    func makeNSView(context: Context) -> Anchor {
        let view = Anchor(); view.navigation = navigation
        return view
    }
    func updateNSView(_ view: Anchor, context: Context) {
        navigation.model = model
        navigation.readFocus = { binding.wrappedValue }
        navigation.writeFocus = { binding.wrappedValue = $0 }
        view.attach()
    }
    static func dismantleNSView(_ view: Anchor, coordinator: ()) { view.navigation?.detach() }
    final class Anchor: NSView {
        weak var navigation: BrowserChromeFocus?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); attach() }
        func attach() {
            guard let window = window as? BrowserWindow, let navigation else { return }
            navigation.window = window; window.chromeFocus = navigation
        }
    }
}
