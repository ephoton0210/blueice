// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

enum BrowserFailure: Error, LocalizedError, Sendable {
    case invalid(String)
    var errorDescription: String? { if case .invalid(let text) = self { return text }; return nil }
}

enum JSONValue: Encodable, Sendable {
    case string(String), unsigned(UInt64), number(Double), boolean(Bool), null
    func encode(to encoder: Encoder) throws {
        var value = encoder.singleValueContainer()
        switch self {
        case .string(let text): try value.encode(text)
        case .unsigned(let number): try value.encode(number)
        case .number(let number): try value.encode(number)
        case .boolean(let boolean): try value.encode(boolean)
        case .null: try value.encodeNil()
        }
    }
}

enum BrowserCommand: Encodable, Sendable {
    case unit(String), values(String, [String: JSONValue])
    case textInput(TextInputContext, TextInputAction)
    case find(UInt64, TextInputContext, FindAction)
    case contextMenuLink(PageMenuContext, PageMenuLinkAction)
    case viewport(Double, Double, Double)
    func encode(to encoder: Encoder) throws {
        switch self {
        case .unit(let name):
            var value = encoder.singleValueContainer()
            try value.encode(name)
        case .values(let name, let fields):
            var value = encoder.container(keyedBy: MessageKey.self)
            try value.encode(fields, forKey: MessageKey(name))
        case .textInput(let context, let action):
            var root = encoder.container(keyedBy: MessageKey.self)
            var value = root.nestedContainer(keyedBy: MessageKey.self, forKey: MessageKey("TextInput"))
            try value.encode(context, forKey: MessageKey("context"))
            try value.encode(action, forKey: MessageKey("action"))
        case .find(let tab, let context, let action):
            var root = encoder.container(keyedBy: MessageKey.self)
            var value = root.nestedContainer(keyedBy: MessageKey.self, forKey: MessageKey("Find"))
            try value.encode(tab, forKey: MessageKey("tab_id"))
            try value.encode(context.frame_source, forKey: MessageKey("frame_source"))
            try value.encode(context.document_generation, forKey: MessageKey("document_generation"))
            try value.encode(action, forKey: MessageKey("action"))
        case .contextMenuLink(let context, let action):
            var root = encoder.container(keyedBy: MessageKey.self)
            var value = root.nestedContainer(keyedBy: MessageKey.self, forKey: MessageKey("ContextMenuLink"))
            try value.encode(context, forKey: MessageKey("context"))
            try value.encode(action, forKey: MessageKey("action"))
        case .viewport(let width, let height, let scale):
            var root = encoder.container(keyedBy: MessageKey.self)
            var message = root.nestedContainer(keyedBy: MessageKey.self, forKey: MessageKey("SetViewport"))
            var value = message.nestedContainer(keyedBy: MessageKey.self, forKey: MessageKey("viewport"))
            try value.encode(width, forKey: MessageKey("width")); try value.encode(height, forKey: MessageKey("height"))
            try value.encode(scale, forKey: MessageKey("device_scale"))
        }
    }
}

enum PageMenuLinkAction: String, Encodable, Sendable { case copy = "Copy", open = "Open", newTab = "OpenInNewTab" }

struct ViewportState: Decodable, Sendable {
    let tabID: UInt64
    let frameSource: UInt64
    let frameGeneration: UInt64
    let width: Double
    let height: Double
    let deviceScale: Double
    let zoom: Double
    let cssWidth: Double
    let cssHeight: Double
    let pixelWidth: Int
    let pixelHeight: Int
    enum CodingKeys: String, CodingKey {
        case tabID = "tab_id", frameSource = "frame_source", frameGeneration = "frame_generation"
        case width, height, deviceScale = "device_scale", zoom, cssWidth = "css_width", cssHeight = "css_height"
        case pixelWidth = "pixel_width", pixelHeight = "pixel_height"
    }
    var valid: Bool {
        tabID > 0 && [width, height, deviceScale, zoom, cssWidth, cssHeight].allSatisfy(\.isFinite)
            && (1...4096).contains(width) && (1...4096).contains(height) && (1...4).contains(deviceScale)
            && (0.25...5).contains(zoom) && abs(cssWidth * zoom - width) < 1e-6 && abs(cssHeight * zoom - height) < 1e-6
            && (1...4096).contains(pixelWidth) && (1...4096).contains(pixelHeight)
            && Double(pixelWidth) == ceil(width * deviceScale) && Double(pixelHeight) == ceil(height * deviceScale)
    }
}

struct PageMenuContext: Codable, Equatable, Sendable {
    let tabID: UInt64
    let frameSource: UInt64
    let documentGeneration: UInt64
    let frameGeneration: UInt64
    let x: Double
    let y: Double
    enum CodingKeys: String, CodingKey {
        case tabID = "tab_id", frameSource = "frame_source", documentGeneration = "document_generation"
        case frameGeneration = "frame_generation", x, y
    }
    var valid: Bool { tabID > 0 && x.isFinite && y.isFinite && (0..<16384).contains(x) && (0..<16384).contains(y) }
}

struct PageContextMenu: Decodable, Sendable {
    let context: PageMenuContext
    let linkURL: String?
    let input: TextInputState?
    enum CodingKeys: String, CodingKey { case context, linkURL = "link_url", input }
    static func validLink(_ text: String) -> Bool {
        text.utf8.count <= 8192 && URL(string: text)?.scheme.map { ["http", "https", "about"].contains($0) } == true
    }
    var valid: Bool {
        guard context.valid, linkURL.map(Self.validLink) != false else { return false }
        guard let input else { return true }
        return (try? input.validate()) != nil && input.focused != nil && input.tab_id == context.tabID
            && input.frame_source == context.frameSource && input.document_generation == context.documentGeneration
            && input.frame_generation == context.frameGeneration
    }
}

struct PageContextLink: Decodable, Sendable {
    let context: PageMenuContext
    let url: String
    var valid: Bool { context.valid && PageContextMenu.validLink(url) }
}

enum FindAction: Encodable, Sendable {
    case update(String, Bool), next(Bool), close
    func encode(to encoder: Encoder) throws {
        switch self {
        case .close:
            var value = encoder.singleValueContainer(); try value.encode("Close")
        case .update(let query, let sensitive):
            var root = encoder.container(keyedBy: MessageKey.self)
            var value = root.nestedContainer(keyedBy: MessageKey.self, forKey: MessageKey("Update"))
            try value.encode(query, forKey: MessageKey("query")); try value.encode(sensitive, forKey: MessageKey("case_sensitive"))
        case .next(let backwards):
            var root = encoder.container(keyedBy: MessageKey.self)
            var value = root.nestedContainer(keyedBy: MessageKey.self, forKey: MessageKey("Next"))
            try value.encode(backwards, forKey: MessageKey("backwards"))
        }
    }
}

struct FindState: Decodable, Sendable {
    let tabID: UInt64
    let frameSource: UInt64
    let documentGeneration: UInt64
    let revision: UInt64
    let query: String
    let caseSensitive: Bool
    let matchCount: UInt32
    let activeMatch: UInt32?
    let wrapped: Bool
    let limited: Bool
    let rects: [PageNode.Bounds]
    enum CodingKeys: String, CodingKey {
        case tabID = "tab_id", frameSource = "frame_source", documentGeneration = "document_generation"
        case revision, query, caseSensitive = "case_sensitive", matchCount = "match_count", activeMatch = "active_match", wrapped, limited, rects
    }
    var valid: Bool {
        tabID > 0 && query.utf8.count <= 1024 && matchCount <= 10000 && rects.count <= 1024
            && ((matchCount == 0 && activeMatch == nil && rects.isEmpty)
                || (matchCount > 0 && activeMatch.map { $0 > 0 && $0 <= matchCount } == true))
            && rects.allSatisfy { [$0.x, $0.y, $0.width, $0.height].allSatisfy(\.isFinite) && $0.width > 0 && $0.height > 0 }
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

struct FormResubmission: Decodable, Sendable {
    let confirmationID: UInt64
    let url: String
    enum CodingKeys: String, CodingKey { case confirmationID = "confirmation_id", url }
}

enum BrowserMessage: Decodable, Sendable {
    case hello(UInt32), tabs([BrowserTab]), opened(UInt64, String?), closed(UInt64)
    case navigationStarted, navigated(String), history(HistoryState), frame(FrameNotice)
    case blocked(String), error(String), representation(PageRepresentation), representationUnavailable, unknown
    case textInputState(TextInputState), textInputUnavailable
    case formResubmission(FormResubmission), formResubmissionResolved(UInt64)
    case findState(FindState), findUnavailable
    case contextMenu(PageContextMenu), contextLink(PageContextLink), contextUnavailable
    case viewportState(ViewportState), viewportUnavailable

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
        case "FormResubmission": self = .formResubmission(try object.decode(FormResubmission.self, forKey: key))
        case "FormResubmissionResolved":
            struct Resolution: Decodable { let confirmation_id: UInt64 }
            self = .formResubmissionResolved(try object.decode(Resolution.self, forKey: key).confirmation_id)
        case "Hello": self = .hello(try object.decode(Hello.self, forKey: key).protocol_version)
        case "Tabs": self = .tabs(try object.decode([BrowserTab].self, forKey: key))
        case "TabOpened":
            struct Opened: Decodable { let tab_id: UInt64; let url: String? }
            let value = try object.decode(Opened.self, forKey: key)
            self = .opened(value.tab_id, value.url)
        case "TabClosed": self = .closed(try object.decode(Tab.self, forKey: key).tab_id)
        case "NavigationStarted": self = .navigationStarted
        case "Navigated": self = .navigated(try object.decode(Navigation.self, forKey: key).url)
        case "HistoryState": self = .history(try object.decode(HistoryState.self, forKey: key))
        case "FrameReady": self = .frame(try object.decode(FrameNotice.self, forKey: key))
        case "Representation":
            if let value = try? object.decode(PageRepresentation.self, forKey: key) { self = .representation(value) }
            else { self = .representationUnavailable }
        case "GatekeeperBlocked": self = .blocked(try object.decode(Blocked.self, forKey: key).reason)
        case "TextInputState":
            if let value = try? object.decode(TextInputState.self, forKey: key), (try? value.validate()) != nil { self = .textInputState(value) }
            else { self = .textInputUnavailable }
        case "Error": self = .error(try object.decode(Failure.self, forKey: key).message)
        case "FindState":
            if let value = try? object.decode(FindState.self, forKey: key), value.valid { self = .findState(value) }
            else { self = .findUnavailable }
        case "ContextMenu":
            if let value = try? object.decode(PageContextMenu.self, forKey: key), value.valid { self = .contextMenu(value) }
            else { self = .contextUnavailable }
        case "ContextMenuLink":
            if let value = try? object.decode(PageContextLink.self, forKey: key), value.valid { self = .contextLink(value) }
            else { self = .contextUnavailable }
        case "ViewportState":
            if let value = try? object.decode(ViewportState.self, forKey: key), value.valid { self = .viewportState(value) }
            else { self = .viewportUnavailable }
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
