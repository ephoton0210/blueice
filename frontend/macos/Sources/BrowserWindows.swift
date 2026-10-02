// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

struct WindowViewport: Codable, Sendable, Equatable {
    let width: Double
    let height: Double
    let deviceScale: Double
    let backingScale: Double?
    enum CodingKeys: String, CodingKey { case width, height, deviceScale = "device_scale", backingScale = "backing_scale" }
    var valid: Bool {
        [width, height, deviceScale].allSatisfy(\.isFinite) && (1...4096).contains(width) && (1...4096).contains(height)
            && (1...4).contains(deviceScale) && backingScale.map { $0.isFinite && (1...4).contains($0) } != false
            && ceil(width * deviceScale) <= 4096 && ceil(height * deviceScale) <= 4096
    }
}

enum WindowAction: Encodable, Sendable {
    case list, create(WindowViewport), resize(UInt64, WindowViewport), close(UInt64), move(UInt64), open(UInt64, String?)
    indirect case command(UInt64, BrowserCommand)
    private enum Keys: String, CodingKey { case create = "Create", resize = "Resize", command = "Command" }
    private struct Scoped: Encodable { let window_id: UInt64; let message: BrowserCommand }
    private struct Display: Encodable { let window_id: UInt64?; let viewport: WindowViewport }
    func encode(to encoder: Encoder) throws {
        switch self {
        case .command(let id, let message):
            var value = encoder.container(keyedBy: Keys.self)
            try value.encode(Scoped(window_id: id, message: message), forKey: .command)
        case .list: var value = encoder.singleValueContainer(); try value.encode("List")
        case .create(let viewport):
            var value = encoder.container(keyedBy: Keys.self)
            try value.encode(Display(window_id: nil, viewport: viewport), forKey: .create)
        case .resize(let id, let viewport):
            var value = encoder.container(keyedBy: Keys.self)
            try value.encode(Display(window_id: id, viewport: viewport), forKey: .resize)
        case .close(let id): try BrowserCommand.values("Close", ["window_id": .unsigned(id)]).encode(to: encoder)
        case .move(let id): try BrowserCommand.values("MoveTab", ["window_id": .unsigned(id)]).encode(to: encoder)
        case .open(let id, let url): try BrowserCommand.values("OpenTab", ["window_id": .unsigned(id), "url": url.map(JSONValue.string) ?? .null]).encode(to: encoder)
        }
    }
}

struct BrowserWindowSummary: Decodable, Sendable, Identifiable {
    let id: UInt64
    let viewport: WindowViewport
    let tabs: [BrowserTab]
}

enum BrowserWindowEvent: Decodable, Sendable {
    case snapshot, created(UInt64), resized(UInt64), closed(UInt64), moved(UInt64, UInt64, UInt64), opened(UInt64, UInt64), tabClosed(UInt64, UInt64)
    private enum Keys: String, CodingKey { case Created, Resized, Closed, TabMoved, TabOpened, TabClosed }
    private struct Window: Decodable { let window_id: UInt64 }
    private struct Tab: Decodable { let tab_id: UInt64; let window_id: UInt64 }
    private struct Move: Decodable { let tab_id: UInt64; let from_window: UInt64; let to_window: UInt64 }
    init(from decoder: Decoder) throws {
        if let value = try? decoder.singleValueContainer().decode(String.self), value == "Snapshot" { self = .snapshot; return }
        let value = try decoder.container(keyedBy: Keys.self)
        guard value.allKeys.count == 1, let key = value.allKeys.first else { throw BrowserFailure.invalid("Invalid window event") }
        switch key {
        case .Created: self = .created(try value.decode(Window.self, forKey: key).window_id)
        case .Resized: self = .resized(try value.decode(Window.self, forKey: key).window_id)
        case .Closed: self = .closed(try value.decode(Window.self, forKey: key).window_id)
        case .TabMoved: let move = try value.decode(Move.self, forKey: key); self = .moved(move.tab_id, move.from_window, move.to_window)
        case .TabOpened: let tab = try value.decode(Tab.self, forKey: key); self = .opened(tab.tab_id, tab.window_id)
        case .TabClosed: let tab = try value.decode(Tab.self, forKey: key); self = .tabClosed(tab.tab_id, tab.window_id)
        }
    }
}
struct BrowserWindowState: Decodable, Sendable {
    let windows: [BrowserWindowSummary]
    let event: BrowserWindowEvent
    var valid: Bool {
        guard windows.count <= 64, windows.allSatisfy({ $0.id > 0 && $0.viewport.valid }), Set(windows.map(\.id)).count == windows.count else { return false }
        let tabs = windows.flatMap(\.tabs)
        guard tabs.allSatisfy({ $0.id > 0 && $0.groupID != 0 }), Set(tabs.map(\.id)).count == tabs.count else { return false }
        func contains(_ tab: UInt64, _ window: UInt64) -> Bool { windows.first { $0.id == window }?.tabs.contains { $0.id == tab } == true }
        switch event {
        case .snapshot: return true
        case .created(let id), .resized(let id): return windows.contains { $0.id == id }
        case .closed(let id): return id > 0 && !windows.contains { $0.id == id }
        case .moved(let tab, let from, let to): return tab > 0 && from > 0 && to > 0 && contains(tab, to)
        case .opened(let tab, let window): return contains(tab, window)
        case .tabClosed(let tab, let window): return tab > 0 && windows.contains { $0.id == window } && !tabs.contains { $0.id == tab }
        }
    }
}
