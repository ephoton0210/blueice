// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

struct TextRange: Codable, Equatable, Sendable {
    let location: UInt32
    let length: UInt32
    var nsRange: NSRange { NSRange(location: Int(location), length: Int(length)) }
    init(_ range: NSRange) {
        location = UInt32(range.location)
        length = UInt32(range.length)
    }
    func valid(length: UInt32) -> Bool { UInt64(location) + UInt64(self.length) <= UInt64(length) }
    static func replacement(_ range: NSRange) -> TextRange? {
        guard range.location != NSNotFound, range.location >= 0, range.length >= 0,
              UInt64(range.location) + UInt64(range.length) <= 65_536 else { return nil }
        return TextRange(range)
    }
}

struct TextInputContext: Encodable, Equatable, Sendable {
    let version: UInt32
    let frame_source: UInt64
    let document_generation: UInt64
    let focus_generation: UInt64
}

enum TextMovement: String, Encodable, Sendable {
    case backward = "Backward", forward = "Forward", wordBackward = "WordBackward", wordForward = "WordForward"
    case beginning = "Beginning", end = "End", lineBeginning = "LineBeginning", lineEnd = "LineEnd", up = "Up", down = "Down"
}

enum TextInputAction: Encodable, Sendable {
    case replace(String, TextRange?), compose(String, TextRange, TextRange?)
    case finishComposition, cancelComposition, select(TextRange), selectAll
    case move(TextMovement, Bool), delete(Bool), pointer(Double, Double, Bool, UInt8)

    private struct Key: CodingKey {
        let stringValue: String
        var intValue: Int? { nil }
        init(_ value: String) { stringValue = value }
        init?(stringValue: String) { self.init(stringValue) }
        init?(intValue: Int) { return nil }
    }
    func encode(to encoder: Encoder) throws {
        func unit(_ name: String) throws { var value = encoder.singleValueContainer(); try value.encode(name) }
        func fields(_ name: String) -> KeyedEncodingContainer<Key> {
            var root = encoder.container(keyedBy: Key.self)
            return root.nestedContainer(keyedBy: Key.self, forKey: Key(name))
        }
        switch self {
        case .finishComposition: try unit("FinishComposition")
        case .cancelComposition: try unit("CancelComposition")
        case .selectAll: try unit("SelectAll")
        case .replace(let text, let range):
            var value = fields("Replace"); try value.encode(text, forKey: Key("text")); try value.encode(range, forKey: Key("replacement"))
        case .compose(let text, let selection, let range):
            var value = fields("Compose"); try value.encode(text, forKey: Key("text"))
            try value.encode(selection, forKey: Key("selection")); try value.encode(range, forKey: Key("replacement"))
        case .select(let range): var value = fields("Select"); try value.encode(range, forKey: Key("range"))
        case .move(let direction, let extend):
            var value = fields("Move"); try value.encode(direction, forKey: Key("direction")); try value.encode(extend, forKey: Key("extend"))
        case .delete(let forward): var value = fields("Delete"); try value.encode(forward, forKey: Key("forward"))
        case .pointer(let x, let y, let extend, let count):
            var value = fields("Pointer"); try value.encode(x, forKey: Key("x")); try value.encode(y, forKey: Key("y"))
            try value.encode(extend, forKey: Key("extend")); try value.encode(count, forKey: Key("click_count"))
        }
    }
}

struct TextControlState: Decodable, Sendable {
    struct Caret: Decodable, Sendable { let offset: UInt32; let bounds: PageNode.Bounds }
    let node_id: UInt64
    let text: String?
    let text_length: UInt32
    let protected: Bool
    let writable: Bool
    let multiline: Bool
    let selection: TextRange
    let marked: TextRange?
    let bounds: PageNode.Bounds
    let caret: PageNode.Bounds
    let carets: [Caret]
    let selection_rects: [PageNode.Bounds]
}

struct TextInputState: Decodable, Sendable {
    let version: UInt32
    let frame_source: UInt64
    let document_generation: UInt64
    let focus_generation: UInt64
    let frame_generation: UInt64
    let tab_id: UInt64
    let scroll_y: Double
    let focused: TextControlState?
    var context: TextInputContext {
        TextInputContext(version: version, frame_source: frame_source,
                         document_generation: document_generation, focus_generation: focus_generation)
    }

    func validate() throws {
        func validBounds(_ value: PageNode.Bounds) -> Bool {
            [value.x, value.y, value.width, value.height].allSatisfy { $0.isFinite && abs($0) <= 1e9 }
                && value.width >= 0 && value.height >= 0
        }
        guard version == 1, scroll_y.isFinite, abs(scroll_y) <= 1e9 else { throw BrowserFailure.invalid("Invalid native text state.") }
        guard let field = focused else { return }
        guard field.text_length <= 65_536, field.selection.valid(length: field.text_length),
              field.marked?.valid(length: field.text_length) != false, field.carets.count <= 1024,
              field.selection_rects.count <= 65_537, validBounds(field.bounds), validBounds(field.caret),
              field.carets.allSatisfy({ $0.offset <= field.text_length && validBounds($0.bounds) }),
              field.selection_rects.allSatisfy(validBounds),
              field.protected ? field.text == nil : field.text?.utf16.count == Int(field.text_length) else {
            throw BrowserFailure.invalid("Invalid native text control.")
        }
        if let text = field.text {
            let units = Array(text.utf16)
            func scalarBoundary(_ offset: Int) -> Bool {
                offset == 0 || offset == units.count || !(0xD800...0xDBFF).contains(units[offset - 1])
                    || !(0xDC00...0xDFFF).contains(units[offset])
            }
            for range in [field.selection, field.marked].compactMap({ $0 }) {
                guard scalarBoundary(Int(range.location)), scalarBoundary(Int(range.location + range.length)) else {
                    throw BrowserFailure.invalid("Native text range splits a Unicode scalar.")
                }
            }
        }
    }

    func viewRect(_ rect: PageNode.Bounds, viewport: CGSize, image: CGSize) -> CGRect {
        guard image.width > 0, image.height > 0 else { return .zero }
        return CGRect(x: rect.x * viewport.width / image.width, y: (rect.y - scroll_y) * viewport.height / image.height,
                      width: rect.width * viewport.width / image.width, height: rect.height * viewport.height / image.height)
    }
}
