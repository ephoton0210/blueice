// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

enum ContextAction: Encodable, Sendable {
    case list, create(String), rename(UInt64, String), close(UInt64)
    indirect case command(UInt64, BrowserCommand)
    private enum Keys: String, CodingKey { case Command }
    private struct Scoped: Encodable { let context_id: UInt64; let message: BrowserCommand }
    func encode(to encoder: Encoder) throws {
        switch self {
        case .list: var value = encoder.singleValueContainer(); try value.encode("List")
        case .create(let name): try BrowserCommand.values("Create", ["name": .string(name)]).encode(to: encoder)
        case .rename(let id, let name): try BrowserCommand.values("Rename", ["context_id": .unsigned(id), "name": .string(name)]).encode(to: encoder)
        case .close(let id): try BrowserCommand.values("Close", ["context_id": .unsigned(id)]).encode(to: encoder)
        case .command(let id, let message):
            var value = encoder.container(keyedBy: Keys.self); try value.encode(Scoped(context_id: id, message: message), forKey: .Command)
        }
    }
}

struct BrowserContextSummary: Decodable, Sendable, Identifiable {
    let id: UInt64
    let name: String
    let windows: [UInt64]
    let groups: [BrowserTabGroup]
    static func normalizedName(_ name: String) -> String { name.trimmingCharacters(in: .whitespacesAndNewlines).precomposedStringWithCanonicalMapping }
    static func validName(_ name: String) -> Bool {
        let text = normalizedName(name)
        return !text.isEmpty && text.utf8.count <= 256 && !text.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
    }
}

enum BrowserContextEvent: Decodable, Sendable {
    case snapshot, created(UInt64), renamed(UInt64), closed(UInt64)
    private enum Keys: String, CodingKey { case Created, Renamed, Closed }
    private struct Identity: Decodable { let context_id: UInt64 }
    init(from decoder: Decoder) throws {
        if let text = try? decoder.singleValueContainer().decode(String.self), text == "Snapshot" { self = .snapshot; return }
        let value = try decoder.container(keyedBy: Keys.self)
        guard value.allKeys.count == 1, let key = value.allKeys.first else { throw BrowserFailure.invalid("Invalid browser context event") }
        let id = try value.decode(Identity.self, forKey: key).context_id
        switch key { case .Created: self = .created(id); case .Renamed: self = .renamed(id); case .Closed: self = .closed(id) }
    }
}
struct BrowserContextState: Decodable, Sendable {
    let contexts: [BrowserContextSummary]
    let event: BrowserContextEvent
    var valid: Bool {
        guard (1...16).contains(contexts.count), contexts.contains(where: { $0.id == 1 }),
              contexts.allSatisfy({ $0.id > 0 && BrowserContextSummary.validName($0.name) && $0.name == BrowserContextSummary.normalizedName($0.name) }),
              Set(contexts.map(\.id)).count == contexts.count,
              Set(contexts.map { $0.name.lowercased() }).count == contexts.count else { return false }
        let windows = contexts.flatMap(\.windows); let groups = contexts.flatMap(\.groups)
        guard windows.count <= 64, windows.allSatisfy({ $0 > 0 }), Set(windows).count == windows.count,
              groups.allSatisfy(\.valid), Set(groups.map(\.id)).count == groups.count else { return false }
        switch event {
        case .snapshot: return true
        case .created(let id), .renamed(let id): return contexts.contains { $0.id == id }
        case .closed(let id): return id > 1 && !contexts.contains { $0.id == id }
        }
    }
}

/// Only profile names and persistent logical keys are stored here. Runtime
/// context/window/tab IDs and page/POST/editing data are never preferences.
@MainActor
final class BrowserContextPreferences {
    struct Profile: Codable, Identifiable, Equatable { let id: UUID; var name: String }
    private struct Catalog: Codable { let version: Int; let profiles: [Profile] }
    static let defaultKey = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!
    private let defaults: UserDefaults
    private(set) var profiles: [Profile] = [Profile(id: defaultKey, name: "Default")]
    private(set) var error: String?
    init(defaults: UserDefaults? = nil) {
        self.defaults = defaults ?? BrowserAppearance.preferenceStore()
        guard self.defaults.object(forKey: "browser.contexts") != nil else { return }
        guard let data = self.defaults.data(forKey: "browser.contexts") else { error = "Saved profile list is unavailable"; return }
        guard data.count <= 16384, let catalog = try? JSONDecoder().decode(Catalog.self, from: data), catalog.version == 1,
              Self.valid(catalog.profiles) else { error = "Saved profile list is unavailable"; return }
        profiles = catalog.profiles
    }
    private static func valid(_ profiles: [Profile]) -> Bool {
        (1...16).contains(profiles.count) && profiles.first?.id == defaultKey && Set(profiles.map(\.id)).count == profiles.count
            && Set(profiles.map { $0.name.lowercased() }).count == profiles.count
            && profiles.allSatisfy { BrowserContextSummary.validName($0.name) && $0.name == BrowserContextSummary.normalizedName($0.name) }
    }
    func save(_ profiles: [Profile]) throws {
        guard error == nil, Self.valid(profiles) else { throw BrowserFailure.invalid(error ?? "Invalid saved profile list") }
        let data = try JSONEncoder().encode(Catalog(version: 1, profiles: profiles))
        defaults.set(data, forKey: "browser.contexts"); self.profiles = profiles
    }
}
