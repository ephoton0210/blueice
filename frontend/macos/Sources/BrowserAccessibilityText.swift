// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

private struct AccessibilityTextKey: CodingKey {
    let stringValue: String
    var intValue: Int? { nil }
    init(_ value: String) { stringValue = value }
    init?(stringValue: String) { self.init(stringValue) }
    init?(intValue: Int) { return nil }
}

struct AccessibilityTextContext: Codable, Equatable, Sendable {
    let version: UInt32
    let frame_source: UInt64
    let document_generation: UInt64
    let frame_generation: UInt64
    let node_id: UInt64
}

struct AccessibilityDelivery: Codable, Equatable, Sendable {
    let version: UInt32
    let frame_source: UInt64
    let document_generation: UInt64
    let revision: UInt64
    var valid: Bool { version == 1 && document_generation > 0 }
}

struct AccessibilityRevealReply: Decodable, Sendable {
    let context: AccessibilityTextContext
    let bounds: PageNode.Bounds
    var valid: Bool {
        context.version == 1 && context.node_id > 0 && context.document_generation > 0 && context.frame_generation > 0
            && [bounds.x, bounds.y, bounds.width, bounds.height].allSatisfy { $0.isFinite && abs($0) <= 1e9 }
            && bounds.width > 0 && bounds.height > 0
    }
}

enum AccessibilityTextAction: Encodable, Sendable {
    case inspect, bounds(TextRange), lineForIndex(UInt32), rangeForLine(UInt32), rangeForIndex(UInt32)
    case rangeForPosition(Double, Double), select(TextRange), replaceSelection(String), setValue(String), scrollToRange(TextRange)
    var mutates: Bool {
        switch self { case .select, .replaceSelection, .setValue, .scrollToRange: return true; default: return false }
    }
    var changesFocus: Bool {
        switch self { case .select, .replaceSelection, .setValue: return true; default: return false }
    }
    func encode(to encoder: Encoder) throws {
        if case .inspect = self { var value = encoder.singleValueContainer(); try value.encode("Inspect"); return }
        var root = encoder.container(keyedBy: AccessibilityTextKey.self)
        func fields(_ name: String) -> KeyedEncodingContainer<AccessibilityTextKey> {
            root.nestedContainer(keyedBy: AccessibilityTextKey.self, forKey: AccessibilityTextKey(name))
        }
        switch self {
        case .inspect: break
        case .bounds(let range): var value = fields("Bounds"); try value.encode(range, forKey: AccessibilityTextKey("range"))
        case .select(let range): var value = fields("Select"); try value.encode(range, forKey: AccessibilityTextKey("range"))
        case .scrollToRange(let range): var value = fields("ScrollToRange"); try value.encode(range, forKey: AccessibilityTextKey("range"))
        case .lineForIndex(let index): var value = fields("LineForIndex"); try value.encode(index, forKey: AccessibilityTextKey("index"))
        case .rangeForIndex(let index): var value = fields("RangeForIndex"); try value.encode(index, forKey: AccessibilityTextKey("index"))
        case .rangeForLine(let line): var value = fields("RangeForLine"); try value.encode(line, forKey: AccessibilityTextKey("line"))
        case .rangeForPosition(let x, let y):
            var value = fields("RangeForPosition"); try value.encode(x, forKey: AccessibilityTextKey("x")); try value.encode(y, forKey: AccessibilityTextKey("y"))
        case .replaceSelection(let text): var value = fields("ReplaceSelection"); try value.encode(text, forKey: AccessibilityTextKey("text"))
        case .setValue(let text): var value = fields("SetValue"); try value.encode(text, forKey: AccessibilityTextKey("text"))
        }
    }
}

struct AccessibilityTextState: Decodable, Sendable {
    let document: Bool?
    struct Style: Decodable, Sendable {
        let font_size_px: Double
        let bold: Bool
        let italic: Bool
        let color: [UInt8]
    }
    let text: String?
    let text_length: UInt32
    let protected: Bool
    let writable: Bool
    let multiline: Bool
    let focused: Bool
    let selection: TextRange?
    let marked: TextRange?
    let visible_range: TextRange
    let insertion_line: UInt32?
    let line_count: UInt32
    let style: Style
    var valid: Bool {
        text_length <= (document == true ? 2_097_152 : 65_536) && (document != true || !protected && !writable && marked == nil)
            && (protected ? text == nil : text?.utf16.count == Int(text_length))
            && visible_range.valid(length: text_length) && selection.map { $0.valid(length: text_length) } != false
            && marked.map { $0.valid(length: text_length) } != false && (focused || selection == nil && marked == nil && insertion_line == nil)
            && line_count > 0 && line_count <= (document == true ? 2_097_153 : 65_537) && insertion_line.map { $0 < line_count } != false
            && style.font_size_px.isFinite && style.font_size_px > 0 && style.color.count == 4
    }
    func substring(_ range: NSRange) -> String? {
        guard !protected, let text, let requested = TextRange.replacement(range, limit: document == true ? 2_097_152 : 65_536), requested.valid(length: text_length),
              range.location <= text.utf16.count else { return nil }
        let units = Array(text.utf16)
        let end = range.location + range.length
        func boundary(_ index: Int) -> Bool { index == units.count || !(0xDC00...0xDFFF).contains(units[index]) }
        guard boundary(range.location), boundary(end) else { return nil }
        return (text as NSString).substring(with: range)
    }
}

enum AccessibilityTextResult: Decodable, Sendable {
    case state(AccessibilityTextState), bounds(PageNode.Bounds?), index(UInt32?), range(TextRange?)
    init(from decoder: Decoder) throws {
        let root = try decoder.container(keyedBy: AccessibilityTextKey.self)
        guard root.allKeys.count == 1, let key = root.allKeys.first else { throw BrowserFailure.invalid("Invalid accessibility text result.") }
        switch key.stringValue {
        case "State":
            let state = try root.decode(AccessibilityTextState.self, forKey: key)
            guard state.valid else { throw BrowserFailure.invalid("Invalid accessibility text state.") }; self = .state(state)
        case "Bounds":
            let bounds = try root.decodeIfPresent(PageNode.Bounds.self, forKey: key)
            if let bounds, ![bounds.x, bounds.y, bounds.width, bounds.height].allSatisfy(\.isFinite) || bounds.width < 0 || bounds.height < 0 {
                throw BrowserFailure.invalid("Invalid accessibility text bounds.")
            }; self = .bounds(bounds)
        case "Index":
            let index = try root.decodeIfPresent(UInt32.self, forKey: key)
            guard index.map({ $0 <= 2_097_152 }) != false else { throw BrowserFailure.invalid("Invalid accessibility text index.") }; self = .index(index)
        case "Range":
            let range = try root.decodeIfPresent(TextRange.self, forKey: key)
            guard range.map({ $0.valid(length: 2_097_152) }) != false else { throw BrowserFailure.invalid("Invalid accessibility text range.") }; self = .range(range)
        default: throw BrowserFailure.invalid("Unknown accessibility text result.")
        }
    }
}
struct AccessibilityTextReply: Decodable, Sendable {
    let context: AccessibilityTextContext
    let result: AccessibilityTextResult
    var valid: Bool { context.version == 1 && context.node_id > 0 && context.frame_generation > 0 && context.document_generation > 0 }
}

private final class AccessibilityReadDeadline: @unchecked Sendable {
    private let connection: BrowserConnection
    private let lock = NSLock()
    private var active = true
    init(_ connection: BrowserConnection) { self.connection = connection }
    func expire() { lock.withLock { if active { connection.interrupt() } } }
    func cancel() { lock.withLock { active = false } }
}

// AppKit AX callbacks are synchronous. A private broker avoids waiting on the
// MainActor's normal reader; the core still owns all text, edits and geometry.
final class BrowserAccessibilityTextSession {
    private let connection: BrowserConnection
    private let tab: UInt64
    private let context: UInt64
    private let window: UInt64
    private var request = UInt64.random(in: (1 << 48)..<(1 << 63))
    private var closed = false
    init(runtime: URL, tab: UInt64, context: UInt64, window: UInt64) throws {
        self.tab = tab; self.context = context; self.window = window
        connection = try BrowserConnection(socketPath: runtime.appendingPathComponent("browser.sock").path)
        do {
            guard case .hello(BrowserWire.version) = try exchange(.values("Hello", ["protocol_version": .unsigned(UInt64(BrowserWire.version))]), scoped: false) else {
                throw BrowserFailure.invalid("Accessibility connection handshake failed.")
            }
        } catch { close(); throw error }
    }
    deinit { close() }
    func close() {
        guard !closed else { return }; closed = true; connection.interrupt(); connection.close()
    }
    private func exchange(_ command: BrowserCommand, scoped: Bool = true) throws -> BrowserMessage {
        guard !closed else { throw BrowserFailure.invalid("Accessibility connection is closed.") }
        request += 1
        let wrapped = scoped ? BrowserCommand.browserContext(.command(context, .window(.command(window, command)))) : command
        let deadline = AccessibilityReadDeadline(connection)
        let timeout = DispatchWorkItem { deadline.expire() }
        DispatchQueue.global().asyncAfter(deadline: .now() + 2, execute: timeout)
        defer { deadline.cancel(); timeout.cancel() }
        try connection.output.write(contentsOf: BrowserWire.encode(wrapped, tab: scoped ? tab : nil, request: request))
        while true {
            let reply = try BrowserWire.read { count in
                var bytes = [UInt8](repeating: 0, count: count)
                let read = Darwin.read(self.connection.input.fileDescriptor, &bytes, count)
                guard read >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
                return Data(bytes.prefix(read))
            }
            guard reply.requestID == request else { continue }
            guard !scoped || reply.tabID == tab else { throw BrowserFailure.invalid("Accessibility reply belongs to another tab.") }
            if case .error(let message) = reply.message { throw BrowserFailure.invalid(message) }
            // Mutations broadcast their new frame before the AX reply. These
            // notifications belong to the normal reader, not this exchange.
            if scoped {
                switch reply.message {
                case .accessibilityTextState, .accessibilityRevealed, .accessibilityAcknowledged: break
                case .frame, .viewportState, .displayPreferences: continue
                default: throw BrowserFailure.invalid("Unexpected accessibility reply.")
                }
            }
            return reply.message
        }
    }
    func acknowledge(_ delivery: AccessibilityDelivery) throws {
        do {
            guard case .accessibilityAcknowledged(let reply) = try exchange(.accessibilityAcknowledge(delivery)),
                  reply.valid, reply.frame_source == delivery.frame_source,
                  reply.document_generation == delivery.document_generation, reply.revision >= delivery.revision else {
                throw BrowserFailure.invalid("Stale accessibility consumption reply.")
            }
        } catch { close(); throw error }
    }
    func reveal(_ context: AccessibilityTextContext) throws -> PageNode.Bounds {
        do {
            guard case .accessibilityRevealed(let reply) = try exchange(.accessibilityReveal(context)),
                  reply.valid, reply.context.version == context.version, reply.context.node_id == context.node_id,
                  reply.context.frame_source == context.frame_source, reply.context.document_generation == context.document_generation,
                  reply.context.frame_generation > context.frame_generation else {
                throw BrowserFailure.invalid("Stale accessibility navigation reply.")
            }
            return reply.bounds
        } catch { close(); throw error }
    }
    func perform(_ textContext: AccessibilityTextContext, action: AccessibilityTextAction, document: Bool = false) throws -> AccessibilityTextResult {
        do {
            guard case .accessibilityTextState(let reply) = try exchange(.accessibilityText(textContext, action)),
                  reply.context.version == textContext.version, reply.context.frame_source == textContext.frame_source,
                  reply.context.document_generation == textContext.document_generation, reply.context.node_id == textContext.node_id,
                  action.mutates ? reply.context.frame_generation > textContext.frame_generation : reply.context.frame_generation == textContext.frame_generation else {
                throw BrowserFailure.invalid("Stale accessibility text reply.")
            }
            switch reply.result {
            case .state(let state): guard (state.document == true) == document else { throw BrowserFailure.invalid("Mismatched accessibility text domain.") }
            case .index(let index): guard index.map({ $0 <= (document ? 2_097_152 : 65_536) }) != false else { throw BrowserFailure.invalid("Invalid accessibility index domain.") }
            case .range(let range): guard range.map({ $0.valid(length: document ? 2_097_152 : 65_536) }) != false else { throw BrowserFailure.invalid("Invalid accessibility range domain.") }
            case .bounds: break
            }
            switch (action, reply.result) {
            case (.inspect, .state), (.select, .state), (.replaceSelection, .state), (.setValue, .state), (.scrollToRange, .state),
                 (.bounds, .bounds), (.lineForIndex, .index), (.rangeForIndex, .range), (.rangeForLine, .range), (.rangeForPosition, .range): break
            default: throw BrowserFailure.invalid("Accessibility text reply has the wrong result type.")
            }
            return reply.result
        } catch { close(); throw error } // A timed-out write is never retried.
    }
}
