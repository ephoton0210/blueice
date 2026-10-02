// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Combine
import SwiftUI

@MainActor
final class BrowserAppearance: ObservableObject {
    enum Appearance: String, CaseIterable { case system, light, dark
        var title: String { rawValue.capitalized } }
    enum Contrast: String, CaseIterable { case system, standard, increased
        var title: String { rawValue.capitalized } }
    enum Motion: String, CaseIterable { case system, full, reduced
        var title: String { switch self { case .system: return "System"; case .full: return "No reduction"; case .reduced: return "Reduce" } } }
    @Published private(set) var appearance: Appearance
    @Published private(set) var contrast: Contrast
    @Published private(set) var motion: Motion
    @Published private(set) var resolved = DisplayPreferences()
    private let defaults: UserDefaults
    private let systemPreferences: @MainActor () -> DisplayPreferences
    private var optionsObservation: AnyCancellable?
    private var appearanceObservation: NSKeyValueObservation?

    init(defaults: UserDefaults? = nil, systemPreferences: (@MainActor () -> DisplayPreferences)? = nil,
         notificationCenter: NotificationCenter? = nil, observeApplication: Bool = true) {
        let store = defaults ?? Self.preferenceStore()
        self.defaults = store
        self.systemPreferences = systemPreferences ?? Self.readSystem
        appearance = Appearance(rawValue: store.string(forKey: "browser.appearance") ?? "") ?? .system
        contrast = Contrast(rawValue: store.string(forKey: "browser.contrast") ?? "") ?? .system
        motion = Motion(rawValue: store.string(forKey: "browser.motion") ?? "") ?? .system
        refreshSystem()
        optionsObservation = (notificationCenter ?? NSWorkspace.shared.notificationCenter)
            .publisher(for: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification)
            .sink { [weak self] _ in Task { @MainActor in self?.refreshSystem() } }
        if observeApplication {
            appearanceObservation = NSApplication.shared.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
                Task { @MainActor in self?.refreshSystem() }
            }
        }
    }
    static func preferenceStore() -> UserDefaults {
        let args = ProcessInfo.processInfo.arguments
        if let i = args.firstIndex(of: "--preferences-domain"), args.indices.contains(i + 1),
           !args[i + 1].isEmpty, args[i + 1].utf8.count <= 255, let store = UserDefaults(suiteName: args[i + 1]) { return store }
        return .standard
    }
    private static func readSystem() -> DisplayPreferences {
        DisplayPreferences(dark: NSApplication.shared.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua,
                           highContrast: NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast,
                           reducedMotion: NSWorkspace.shared.accessibilityDisplayShouldReduceMotion)
    }
    func refreshSystem() {
        let system = systemPreferences()
        let next = DisplayPreferences(dark: appearance == .system ? system.dark : appearance == .dark,
                                      highContrast: contrast == .system ? system.highContrast : contrast == .increased,
                                      reducedMotion: motion == .system ? system.reducedMotion : motion == .reduced)
        if next != resolved { resolved = next }
    }
    func setAppearance(_ value: Appearance) { guard appearance != value else { return }; appearance = value; defaults.set(value.rawValue, forKey: "browser.appearance"); refreshSystem() }
    func setContrast(_ value: Contrast) { guard contrast != value else { return }; contrast = value; defaults.set(value.rawValue, forKey: "browser.contrast"); refreshSystem() }
    func setMotion(_ value: Motion) { guard motion != value else { return }; motion = value; defaults.set(value.rawValue, forKey: "browser.motion"); refreshSystem() }
    func restoreSystem() { setAppearance(.system); setContrast(.system); setMotion(.system) }
    var windowAppearance: NSAppearance? {
        if appearance == .system && contrast == .system { return nil }
        let name: NSAppearance.Name = resolved.highContrast
            ? (resolved.dark ? .accessibilityHighContrastDarkAqua : .accessibilityHighContrastAqua)
            : (resolved.dark ? .darkAqua : .aqua)
        return NSAppearance(named: name)
    }
    var summary: String {
        "\(resolved.dark ? "Dark" : "Light") appearance · \(resolved.highContrast ? "Increased" : "Standard") contrast · \(resolved.reducedMotion ? "Reduced motion" : "No motion reduction")"
    }
}

struct BrowserAppearanceSettingsView: View {
    @ObservedObject var settings: BrowserAppearance
    var body: some View {
        Form {
            Picker("Appearance", selection: Binding(get: { settings.appearance }, set: settings.setAppearance)) {
                ForEach(BrowserAppearance.Appearance.allCases, id: \.self) { Text($0.title).tag($0) }
            }.accessibilityIdentifier("appearance-choice")
            Picker("Contrast", selection: Binding(get: { settings.contrast }, set: settings.setContrast)) {
                ForEach(BrowserAppearance.Contrast.allCases, id: \.self) { Text($0.title).tag($0) }
            }.accessibilityIdentifier("contrast-choice")
            Picker("Motion", selection: Binding(get: { settings.motion }, set: settings.setMotion)) {
                ForEach(BrowserAppearance.Motion.allCases, id: \.self) { Text($0.title).tag($0) }
            }.accessibilityIdentifier("motion-choice")
            Text("Pages can follow these preferences. System follows your current macOS settings.")
                .font(.caption).foregroundStyle(settings.resolved.highContrast ? .primary : .secondary)
            Text(settings.summary).font(.caption).accessibilityIdentifier("display-preferences")
            Button("Restore System Settings", action: settings.restoreSystem).accessibilityIdentifier("restore-display-system")
        }
        .padding(24).frame(width: 460)
        .transaction { if settings.resolved.reducedMotion { $0.animation = nil; $0.disablesAnimations = true } }
        .background(AppearanceWindow(settings: settings).frame(width: 0, height: 0).accessibilityHidden(true))
    }
}

struct AppearanceWindow: NSViewRepresentable {
    @ObservedObject var settings: BrowserAppearance
    func makeNSView(context: Context) -> Host { let view = Host(); view.settings = settings; return view }
    func updateNSView(_ view: Host, context: Context) { view.settings = settings; view.applyAppearance() }
    final class Host: NSView {
        weak var settings: BrowserAppearance?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); applyAppearance() }
        func applyAppearance() {
            guard let window, let settings else { return }
            let next = settings.windowAppearance
            if window.appearance?.name != next?.name { window.appearance = next }
        }
    }
}

struct BrowserAppearanceCommands: Commands {
    @ObservedObject var settings: BrowserAppearance
    var body: some Commands {
        CommandGroup(after: .toolbar) {
            Menu("Appearance") {
                Toggle("Use System Appearance", isOn: Binding(get: { settings.appearance == .system }, set: { if $0 { settings.setAppearance(.system) } }))
                Toggle("Light Appearance", isOn: Binding(get: { settings.appearance == .light }, set: { if $0 { settings.setAppearance(.light) } }))
                Toggle("Dark Appearance", isOn: Binding(get: { settings.appearance == .dark }, set: { if $0 { settings.setAppearance(.dark) } }))
            }
        }
    }
}
