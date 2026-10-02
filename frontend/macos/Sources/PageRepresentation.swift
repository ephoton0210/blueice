// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import CoreGraphics
import Foundation

enum PageRole: Equatable, Decodable, Sendable {
    case heading(UInt8), link, button, textBox, checkBox, slider, comboBox, option, list, listItem, paragraph, image, generic

    init(from decoder: Decoder) throws {
        if let name = try? decoder.singleValueContainer().decode(String.self) {
            switch name {
            case "Link": self = .link
            case "Button": self = .button
            case "TextBox": self = .textBox
            case "CheckBox": self = .checkBox
            case "Slider": self = .slider
            case "ComboBox": self = .comboBox
            case "Option": self = .option
            case "List": self = .list
            case "ListItem": self = .listItem
            case "Paragraph": self = .paragraph
            case "Image": self = .image
            case "Generic": self = .generic
            default: throw BrowserFailure.invalid("Unsupported page accessibility role.")
            }
        } else {
            struct Heading: Decodable { let level: UInt8 }
            struct Object: Decodable { let Heading: Heading }
            let level = try Object(from: decoder).Heading.level
            guard (1...6).contains(level) else { throw BrowserFailure.invalid("Invalid heading level.") }
            self = .heading(level)
        }
    }
}

struct PageNode: Decodable, Sendable {
    struct State: Decodable, Sendable {
        let value: String?
        let checked: Bool?
        let disabled: Bool
        let required: Bool
        let selected: Bool
        let focused: Bool
        let nativeTextInput: Bool
        let protected: Bool
        enum CodingKeys: String, CodingKey {
            case value, checked, disabled, required, selected, focused, protected
            case nativeTextInput = "native_text_input"
        }
        init(from decoder: Decoder) throws {
            let value = try decoder.container(keyedBy: CodingKeys.self)
            self.value = try value.decodeIfPresent(String.self, forKey: .value)
            checked = try value.decodeIfPresent(Bool.self, forKey: .checked)
            disabled = try value.decode(Bool.self, forKey: .disabled)
            required = try value.decode(Bool.self, forKey: .required)
            selected = try value.decode(Bool.self, forKey: .selected)
            focused = try value.decode(Bool.self, forKey: .focused)
            nativeTextInput = try value.decodeIfPresent(Bool.self, forKey: .nativeTextInput) ?? false
            protected = try value.decodeIfPresent(Bool.self, forKey: .protected) ?? false
        }
    }
    struct Bounds: Decodable, Sendable {
        let x: Double
        let y: Double
        let width: Double
        let height: Double
    }
    let id: UInt64
    let parent: UInt64?
    let children: [UInt64]
    let role: PageRole
    let name: String?
    let state: State
    let bounds: Bounds
    let opacity: Double
    let occluded: Bool
    let occludedFraction: Double
    enum CodingKeys: String, CodingKey {
        case id, parent, children, role, name, state, bounds, opacity, occluded
        case occludedFraction = "occluded_fraction"
    }
}

struct PageRepresentation: Decodable, Sendable {
    let frameSource: UInt64
    let generation: UInt64
    let tabID: UInt64
    let url: String?
    let scrollY: Double
    let nodes: [PageNode]
    enum CodingKeys: String, CodingKey {
        case frameSource = "frame_source", tabID = "tab_id", scrollY = "scroll_y"
        case generation, url, nodes
    }

    init(from decoder: Decoder) throws {
        let value = try decoder.container(keyedBy: CodingKeys.self)
        frameSource = try value.decode(UInt64.self, forKey: .frameSource)
        generation = try value.decode(UInt64.self, forKey: .generation)
        tabID = try value.decode(UInt64.self, forKey: .tabID)
        url = try value.decodeIfPresent(String.self, forKey: .url)
        scrollY = try value.decode(Double.self, forKey: .scrollY)
        nodes = try value.decode([PageNode].self, forKey: .nodes)
        try validate()
    }

    private func validate() throws {
        func invalid() -> BrowserFailure { .invalid("Invalid page accessibility tree.") }
        guard scrollY.isFinite, abs(scrollY) <= 1e9, nodes.count <= 20_000 else { throw invalid() }
        var byID: [UInt64: PageNode] = [:]
        for node in nodes {
            let bounds = node.bounds
            guard byID[node.id] == nil, [bounds.x, bounds.y, bounds.width, bounds.height].allSatisfy({ $0.isFinite && abs($0) <= 1e9 }),
                  bounds.width >= 0, bounds.height >= 0, node.opacity.isFinite, (0...1).contains(node.opacity),
                  node.occludedFraction.isFinite, (0...1).contains(node.occludedFraction),
                  Set(node.children).count == node.children.count else { throw invalid() }
            byID[node.id] = node
        }
        for node in nodes {
            if let parent = node.parent, byID[parent]?.children.contains(node.id) != true { throw invalid() }
            for child in node.children where byID[child]?.parent != node.id { throw invalid() }
            // Bounded iteration also rejects cycles without recursive decoding of an untrusted tree.
            var cursor: PageNode? = node
            var depth = 0
            while let current = cursor {
                depth += 1
                guard depth <= 256 else { throw invalid() }
                cursor = current.parent.flatMap { byID[$0] }
            }
        }
    }

    func matches(tab: UInt64, generation: UInt64, source: UInt64, url: String?) -> Bool {
        tabID == tab && self.generation == generation && frameSource == source && self.url == url
    }

    func viewRect(for node: PageNode, viewport: CGSize, image: CGSize) -> CGRect {
        guard image.width > 0, image.height > 0 else { return .zero }
        let xScale = viewport.width / image.width, yScale = viewport.height / image.height
        return CGRect(x: node.bounds.x * xScale, y: (node.bounds.y - scrollY) * yScale,
                      width: node.bounds.width * xScale, height: node.bounds.height * yScale)
    }

    // The protocol derives frame_source from the live frame directory with FNV-1a.
    static func frameSource(directory: String) -> UInt64 {
        directory.utf8.reduce(UInt64(0xcbf29ce484222325)) { ($0 ^ UInt64($1)) &* 0x100000001b3 }
    }
}
