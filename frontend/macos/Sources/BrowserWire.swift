// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

enum BrowserFailure: Error, LocalizedError, Sendable {
    case invalid(String)
    var errorDescription: String? { if case .invalid(let text) = self { return text }; return nil }
}

enum JSONValue: Encodable, Sendable {
    case string(String), unsigned(UInt64), number(Double), null
    func encode(to encoder: Encoder) throws {
        var value = encoder.singleValueContainer()
        switch self {
        case .string(let text): try value.encode(text)
        case .unsigned(let number): try value.encode(number)
        case .number(let number): try value.encode(number)
        case .null: try value.encodeNil()
        }
    }
}

enum BrowserCommand: Encodable, Sendable {
    case unit(String), values(String, [String: JSONValue])
    func encode(to encoder: Encoder) throws {
        switch self {
        case .unit(let name):
            var value = encoder.singleValueContainer()
            try value.encode(name)
        case .values(let name, let fields):
            var value = encoder.container(keyedBy: MessageKey.self)
            try value.encode(fields, forKey: MessageKey(name))
        }
    }
}

private struct MessageKey: CodingKey {
    let stringValue: String
    var intValue: Int? { nil }
    init(_ value: String) { stringValue = value }
    init?(stringValue: String) { self.init(stringValue) }
    init?(intValue: Int) { return nil }
}

struct BrowserTab: Decodable, Identifiable, Sendable {
    let id: UInt64
    var url: String?
}

struct FrameNotice: Decodable, Sendable {
    let path: String
    let width: Int
    let height: Int
    let generation: UInt64
    enum CodingKeys: String, CodingKey { case path = "shm_path", width, height, generation }
}

struct HistoryState: Decodable, Sendable {
    var back = false
    var forward = false
    enum CodingKeys: String, CodingKey { case back = "can_go_back", forward = "can_go_forward" }
}

enum BrowserMessage: Decodable, Sendable {
    case hello(UInt32), tabs([BrowserTab]), opened(UInt64), closed(UInt64)
    case navigated(String), history(HistoryState), frame(FrameNotice)
    case blocked(String), error(String), unknown

    init(from decoder: Decoder) throws {
        guard let object = try? decoder.container(keyedBy: MessageKey.self), let key = object.allKeys.first else {
            self = .unknown
            return
        }
        struct Hello: Decodable { let protocol_version: UInt32 }
        struct Tab: Decodable { let tab_id: UInt64 }
        struct Navigation: Decodable { let url: String }
        struct Blocked: Decodable { let reason: String }
        struct Failure: Decodable { let message: String }
        switch key.stringValue {
        case "Hello": self = .hello(try object.decode(Hello.self, forKey: key).protocol_version)
        case "Tabs": self = .tabs(try object.decode([BrowserTab].self, forKey: key))
        case "TabOpened": self = .opened(try object.decode(Tab.self, forKey: key).tab_id)
        case "TabClosed": self = .closed(try object.decode(Tab.self, forKey: key).tab_id)
        case "Navigated": self = .navigated(try object.decode(Navigation.self, forKey: key).url)
        case "HistoryState": self = .history(try object.decode(HistoryState.self, forKey: key))
        case "FrameReady": self = .frame(try object.decode(FrameNotice.self, forKey: key))
        case "GatekeeperBlocked": self = .blocked(try object.decode(Blocked.self, forKey: key).reason)
        case "Error": self = .error(try object.decode(Failure.self, forKey: key).message)
        default: self = .unknown
        }
    }
}

struct IncomingEnvelope: Decodable, Sendable {
    let requestID: UInt64?
    let tabID: UInt64?
    let message: BrowserMessage
    enum CodingKeys: String, CodingKey { case requestID = "request_id", tabID = "tab_id", message }
}

enum BrowserWire {
    static let version: UInt32 = 2
    static let maxBytes = 8 * 1024 * 1024

    static func encode(_ command: BrowserCommand, tab: UInt64?, request: UInt64) throws -> Data {
        struct Envelope: Encodable {
            let request_id: UInt64
            let tab_id: UInt64?
            let message: BrowserCommand
        }
        let payload = try JSONEncoder().encode(Envelope(request_id: request, tab_id: tab, message: command))
        guard payload.count > 0, payload.count <= maxBytes else { throw BrowserFailure.invalid("Browser message is too large.") }
        let size = UInt32(payload.count)
        var packet = Data((0..<4).map { UInt8(truncatingIfNeeded: size >> ($0 * 8)) })
        packet.append(payload)
        return packet
    }

    // A partial pipe read never becomes a partial JSON frame.
    static func read(from read: (Int) throws -> Data) throws -> IncomingEnvelope {
        func exact(_ count: Int) throws -> Data {
            var result = Data()
            while result.count < count {
                let part = try read(count - result.count)
                guard !part.isEmpty, part.count <= count - result.count else {
                    throw BrowserFailure.invalid("Core connection ended during a browser message.")
                }
                result.append(part)
            }
            return result
        }
        let prefix = try exact(4)
        let count = prefix.enumerated().reduce(UInt32(0)) { $0 | UInt32($1.element) << ($1.offset * 8) }
        guard count > 0, count <= maxBytes else { throw BrowserFailure.invalid("Invalid browser message length.") }
        return try JSONDecoder().decode(IncomingEnvelope.self, from: exact(Int(count)))
    }
}
