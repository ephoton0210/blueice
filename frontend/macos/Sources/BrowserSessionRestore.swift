// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import SwiftUI

struct SessionDocument: Codable, Equatable, Sendable {
    let tab_id: UInt64
    let frame_source: UInt64
    let document_generation: UInt64
}
struct NavigationEntry: Codable, Equatable, Sendable {
    let url: String?
    let was_post: Bool
    var valid: Bool {
        guard let url else { return !was_post }
        guard !url.isEmpty, url.utf8.count <= 4096, !url.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else { return false }
        if ["about:credits", "about:settings", "about:assistant", "about:downloads"].contains(url) || url.hasPrefix("about:downloads?") { return !was_post }
        guard let parts = URLComponents(string: url), ["http", "https"].contains(parts.scheme?.lowercased() ?? ""),
              parts.host?.isEmpty == false, parts.user == nil, parts.password == nil else { return false }
        return true
    }
}
struct NavigationHistory: Codable, Equatable, Sendable {
    var entries: [NavigationEntry]
    let cursor: Int
    let zoom: Double
    var valid: Bool { (1...128).contains(entries.count) && entries.indices.contains(cursor) && zoom.isFinite && (0.25...5).contains(zoom) && entries.allSatisfy(\.valid) }
    var current: NavigationEntry { entries[cursor] }
}
struct NavigationSessionState: Decodable, Sendable {
    let context: SessionDocument
    let history: NavigationHistory
    var valid: Bool { context.tab_id > 0 && history.valid }
}
enum NavigationSessionAction: Encodable, Sendable {
    case inspect, restore(SessionDocument, NavigationHistory)
    private enum Keys: String, CodingKey { case Restore }
    private struct Restoration: Encodable { let context: SessionDocument; let history: NavigationHistory }
    func encode(to encoder: Encoder) throws {
        switch self {
        case .inspect: var box = encoder.singleValueContainer(); try box.encode("Inspect")
        case .restore(let context, let history):
            var box = encoder.container(keyedBy: Keys.self); try box.encode(Restoration(context: context, history: history), forKey: .Restore)
        }
    }
}
struct SavedBrowserSession: Codable, Equatable {
    struct Group: Codable, Equatable {
        let name: String; let color: String; let collapsed: Bool
        var valid: Bool { BrowserTabGroup.validName(name) && BrowserTabGroup.validColor(color) }
    }
    struct Tab: Codable, Equatable { let history: NavigationHistory; let group: Int? }
    struct Frame: Codable, Equatable {
        let x: Double; let y: Double; let width: Double; let height: Double
        var valid: Bool { [x,y,width,height].allSatisfy(\.isFinite) && abs(x) <= 100000 && abs(y) <= 100000 && (640...10000).contains(width) && (400...10000).contains(height) }
    }
    struct Window: Codable, Equatable {
        let key: UUID; let tabs: [Tab]; let selected: Int?; let frame: Frame?
    }
    struct Profile: Codable, Equatable { let key: UUID; let groups: [Group]; let windows: [Window] }
    let version: Int
    let profiles: [Profile]
    let active: UUID
    var valid: Bool {
        guard version == 1, (1...16).contains(profiles.count), Set(profiles.map(\.key)).count == profiles.count else { return false }
        let windows = profiles.flatMap(\.windows); let tabs = windows.flatMap(\.tabs)
        guard (1...64).contains(windows.count), tabs.count <= 256, Set(windows.map(\.key)).count == windows.count,
              windows.contains(where: { $0.key == active }), profiles.flatMap(\.groups).count <= 128 else { return false }
        var bytes = 0
        for profile in profiles {
            guard profile.groups.allSatisfy(\.valid) else { return false }
            for window in profile.windows {
                guard window.frame?.valid != false, (window.tabs.isEmpty ? window.selected == nil : window.selected.map(window.tabs.indices.contains) == true) else { return false }
                for tab in window.tabs {
                    guard tab.history.valid, tab.group.map(profile.groups.indices.contains) != false else { return false }
                    bytes += tab.history.entries.reduce(0) { $0 + ($1.url?.utf8.count ?? 0) }
                    guard bytes <= 1024 * 1024 else { return false }
                }
            }
        }
        return true
    }
}
@MainActor
final class BrowserSessionPreferences: ObservableObject {
    private let defaults: UserDefaults
    @Published private(set) var remember: Bool
    @Published private(set) var reopen: Bool
    @Published private(set) var saved: SavedBrowserSession?
    @Published private(set) var error: String?
    @Published var status: String?
    var localizedStatus: String {
        if let saved, let status {
            let windows = saved.profiles.flatMap(\.windows).count
            let tabs = saved.profiles.flatMap(\.windows).flatMap(\.tabs).count
            if status == "Saved \(windows) windows and \(tabs) tabs." {
                return BrowserStrings.format("Saved %llu windows and %llu tabs.", UInt64(windows), UInt64(tabs))
            }
        }
        return BrowserStrings.text(error ?? status ?? " ")
    }
    init(defaults: UserDefaults? = nil) {
        self.defaults = defaults ?? BrowserAppearance.preferenceStore()
        remember = self.defaults.bool(forKey: "browser.session.remember")
        reopen = self.defaults.bool(forKey: "browser.session.reopen")
        if self.defaults.object(forKey: "browser.session.archive") != nil {
            guard let data = self.defaults.data(forKey: "browser.session.archive"), data.count <= 2 * 1024 * 1024,
                  let saved = try? JSONDecoder().decode(SavedBrowserSession.self, from: data), saved.valid else {
                error = "Saved session is unavailable. Forget it to enable saving again."; return
            }
            self.saved = saved
        }
    }
    func setRemember(_ value: Bool) {
        remember = value; defaults.set(value, forKey: "browser.session.remember")
        if !value {
            saved = nil; error = nil; status = nil
            defaults.removeObject(forKey: "browser.session.archive"); setReopen(false)
            // Forget must finish flushing the removal before normal termination.
            defaults.synchronize()
        }
    }
    func setReopen(_ value: Bool) { reopen = value && remember; defaults.set(reopen, forKey: "browser.session.reopen") }
    func save(_ session: SavedBrowserSession) throws {
        guard remember, error == nil, session.valid else { throw BrowserFailure.invalid(error ?? "The session exceeds its save limits.") }
        let data = try JSONEncoder().encode(session)
        guard data.count <= 2 * 1024 * 1024 else { throw BrowserFailure.invalid("The session exceeds its save limits.") }
        defaults.set(data, forKey: "browser.session.archive"); saved = session
        defaults.synchronize()
        status = "Saved \(session.profiles.flatMap(\.windows).count) windows and \(session.profiles.flatMap(\.windows).flatMap(\.tabs).count) tabs."
    }
}
struct BrowserSessionSettingsView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var workspace: BrowserWorkspace
    @ObservedObject var preferences: BrowserSessionPreferences
    var body: some View {
        GroupBox(BrowserStrings.text("Windows and tabs")) {
            VStack(alignment: .leading, spacing: 8) {
                Toggle(BrowserStrings.text("Remember windows and tabs"), isOn: Binding(get: { preferences.remember }, set: workspace.setRememberSession)).accessibilityIdentifier("session-remember")
                Toggle(BrowserStrings.text("Reopen saved session on startup"), isOn: Binding(get: { preferences.reopen }, set: preferences.setReopen))
                    .disabled(!preferences.remember).accessibilityIdentifier("session-reopen")
                Text(BrowserStrings.text("Saves visited URLs, history, groups, selected tabs, window positions and zoom. Pages are reviewed again when reopened. Form contents, passwords, selected files and POST bodies are not saved.")).font(.caption)
                // Keep the switches stationary when the first save completes.
                Text(verbatim: preferences.localizedStatus)
                    .font(.caption).accessibilityIdentifier("session-status")
                    .accessibilityHidden(preferences.error == nil && preferences.status == nil)
                HStack {
                    Button(BrowserStrings.text("Restore Last Session")) { Task { await workspace.restoreSavedSession() } }.disabled(!workspace.canRestoreSession).accessibilityIdentifier("session-restore")
                    Button(BrowserStrings.text("Forget Session and Stop Remembering")) { workspace.setRememberSession(false) }.accessibilityIdentifier("session-forget")
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
struct BrowserSessionCommands: Commands {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var workspace: BrowserWorkspace
    var body: some Commands {
        CommandGroup(after: .newItem) {
            Button(BrowserStrings.text("Restore Last Session")) { Task { await workspace.restoreSavedSession() } }.disabled(!workspace.canRestoreSession)
        }
    }
}
