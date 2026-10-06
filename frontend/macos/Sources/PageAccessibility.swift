// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

enum PageAccessibilityAction: Equatable { case press, focus, select, deselect, reveal }

@MainActor
final class PageAccessibilityTree: NSObject, @MainActor NSAccessibilityCustomRotorItemSearchDelegate {
    private weak var view: NSView?
    private let perform: (PageRepresentation, UInt64, PageNode, PageAccessibilityAction) -> Bool
    private let text: ((PageRepresentation, UInt64, PageNode, AccessibilityTextAction) -> AccessibilityTextResult?)?
    private(set) var elements: [UInt64: PageAccessibilityElement] = [:]
    private var snapshot: PageRepresentation?
    private var epoch: UInt64 = 0
    private var tabID: UInt64?
    private var frameSource: UInt64?
    private var documentGeneration: UInt64?
    private var focusedID: UInt64?
    private var imageSize = CGSize.zero
    private var readingID: UInt64?
    private var announcementDocument: String?
    private var announcementRevision: UInt64 = 0
    private let isActive: () -> Bool
    private let announce: (String, LivePoliteness) -> Void
    private let acknowledge: (PageRepresentation, UInt64) -> Void
    private(set) var rotors: [NSAccessibilityCustomRotor] = []

    init(view: NSView, text: ((PageRepresentation, UInt64, PageNode, AccessibilityTextAction) -> AccessibilityTextResult?)? = nil,
         isActive: (() -> Bool)? = nil, announce: ((String, LivePoliteness) -> Void)? = nil,
         acknowledge: ((PageRepresentation, UInt64) -> Void)? = nil,
         perform: @escaping (PageRepresentation, UInt64, PageNode, PageAccessibilityAction) -> Bool) {
        self.view = view
        self.text = text
        self.perform = perform
        self.acknowledge = acknowledge ?? { _, _ in }
        self.isActive = isActive ?? { [weak view] in NSApp?.isActive == true && view?.window?.isKeyWindow == true }
        self.announce = announce ?? { text, politeness in
            guard let application = NSApp else { return }
            NSAccessibility.post(element: application, notification: .announcementRequested,
                userInfo: [.announcement: text, .priority: politeness == .assertive ? NSAccessibilityPriorityLevel.high.rawValue : NSAccessibilityPriorityLevel.low.rawValue])
        }
        super.init()
    }

    func update(_ value: PageRepresentation?, epoch: UInt64, imageSize: CGSize, tab: UInt64? = nil) {
        guard let view else { return }
        let documentTab = value?.tabID ?? tab
        let sameDocument = self.epoch == epoch && tabID == documentTab
            && (value == nil || frameSource == value?.frameSource && documentGeneration == value?.accessibility?.document_generation)
        let changed = self.epoch != epoch || snapshot?.generation != value?.generation || snapshot?.tabID != value?.tabID
        let previousFocus = focusedID
        if !sameDocument {
            focusedID = nil
            readingID = nil
            announcementDocument = nil
            rotors = [.heading, .headingLevel1, .headingLevel2, .headingLevel3, .headingLevel4,
                      .headingLevel5, .headingLevel6, .link, .image, .list].map {
                NSAccessibilityCustomRotor(rotorType: $0, itemSearchDelegate: self)
            }
            rotors.append(NSAccessibilityCustomRotor(label: "Buttons", itemSearchDelegate: self))
            for element in elements.values { element.invalidate() }
            elements.removeAll()
        }
        snapshot = value
        tabID = documentTab
        if let value { frameSource = value.frameSource; documentGeneration = value.accessibility?.document_generation }
        self.epoch = epoch
        self.imageSize = imageSize
        let hidden = Set(value?.accessibility?.hidden_nodes ?? [])
        let nodes = value?.nodes.filter { !hidden.contains($0.id) } ?? []
        let ids = Set(nodes.map(\.id))
        for id in Array(elements.keys) where value != nil && !ids.contains(id) { elements.removeValue(forKey: id)?.invalidate() }
        let names = Dictionary(uniqueKeysWithValues: (value?.accessibility?.names ?? []).map { ($0.node_id, $0) })
        for original in nodes {
            var node = original
            if let correction = names[node.id] { node.name = correction.name }
            let element = elements[node.id] ?? PageAccessibilityElement(tree: self, node: node)
            element.node = node
            elements[node.id] = element
        }
        for node in nodes {
            guard let element = elements[node.id] else { continue }
            element.setAccessibilityParent(node.parent.flatMap { elements[$0] } ?? view as Any)
            element.setAccessibilityChildren(node.children.compactMap { elements[$0] })
        }
        let roots = nodes.filter { node in node.parent.map { elements[$0] == nil } ?? true }.compactMap { elements[$0.id] }
        view.setAccessibilityChildren(roots)
        view.setAccessibilityVisibleChildren(roots.filter { isVisible($0.node) })
        if changed && value != nil { NSAccessibility.post(element: view, notification: .layoutChanged) }
        if changed {
            for node in nodes {
                if let element = elements[node.id] { NSAccessibility.post(element: element, notification: .valueChanged) }
            }
        }
        if let value { focusedID = value.nodes.first(where: { $0.state.focused })?.id }
        if previousFocus != focusedID { readingID = nil }
        if let readingID, elements[readingID] == nil { self.readingID = nil }
        deliverAnnouncements(value)
        if let focused = value?.nodes.first(where: { $0.state.focused }), (!sameDocument || previousFocus != focused.id),
           let element = elements[focused.id], view.window?.firstResponder === view {
            NSAccessibility.post(element: element, notification: .focusedUIElementChanged)
        }
    }

    private func deliverAnnouncements(_ value: PageRepresentation?) {
        guard let value, let stream = value.accessibility else { return }
        let document = "\(value.frameSource):\(value.tabID):\(epoch):\(stream.document_generation)"
        if announcementDocument != document {
            announcementDocument = document; announcementRevision = stream.revision
        } else {
            guard stream.revision >= announcementRevision else { return }
            let pending = stream.announcements.filter { $0.sequence > announcementRevision }
            announcementRevision = stream.revision
            if !pending.isEmpty && isActive() { for item in pending { announce(item.text, item.politeness) } }
        }
        if stream.delivery_version == 1 && stream.acknowledged_revision < stream.revision {
            acknowledge(value, epoch)
        }
    }

    func rotor(_ rotor: NSAccessibilityCustomRotor, resultFor parameters: NSAccessibilityCustomRotor.SearchParameters) -> NSAccessibilityCustomRotor.ItemResult? {
        guard rotors.contains(where: { $0 === rotor }), let snapshot else { return nil }
        var origin: Int?
        if let current = parameters.currentItem {
            guard let element = current.targetElement as? PageAccessibilityElement, contains(element),
                  let index = snapshot.nodes.firstIndex(where: { $0.id == element.node.id }) else { return nil }
            origin = index
        }
        let indexes: [Int]
        switch parameters.searchDirection {
        case .next: indexes = Array(((origin.map { $0 + 1 }) ?? 0)..<snapshot.nodes.count)
        case .previous: indexes = Array((0..<(origin ?? snapshot.nodes.count)).reversed())
        @unknown default: return nil
        }
        for index in indexes {
            guard let element = elements[snapshot.nodes[index].id] else { continue }
            let node = element.node
            guard rotorMatches(rotor.type, node.role), node.opacity > 0,
                  node.bounds.width > 0, node.bounds.height > 0,
                  parameters.filterString.isEmpty || node.name?.range(of: parameters.filterString, options: [.caseInsensitive, .diacriticInsensitive]) != nil else { continue }
            let result = NSAccessibilityCustomRotor.ItemResult(targetElement: element)
            result.customLabel = node.name
            return result
        }
        return nil
    }

    private func rotorMatches(_ type: NSAccessibilityCustomRotor.RotorType, _ role: PageRole) -> Bool {
        switch (type, role) {
        case (.heading, .heading), (.link, .link), (.image, .image), (.list, .list), (.custom, .button): return true
        case (.headingLevel1, .heading(1)), (.headingLevel2, .heading(2)), (.headingLevel3, .heading(3)),
             (.headingLevel4, .heading(4)), (.headingLevel5, .heading(5)), (.headingLevel6, .heading(6)): return true
        default: return false
        }
    }

    func clearReadingFocus() { readingID = nil }
    func clearReadingFocus(_ element: PageAccessibilityElement) {
        if elements[element.node.id] === element && readingID == element.node.id { readingID = nil }
    }

    func frame(for node: PageNode) -> NSRect {
        guard let view, let window = view.window, let snapshot else { return .zero }
        let rect = snapshot.viewRect(for: node, viewport: view.bounds.size, image: imageSize)
        return window.convertToScreen(view.convert(rect, to: nil))
    }

    func isVisible(_ node: PageNode) -> Bool {
        guard elements[node.id] != nil, let view, let snapshot, node.opacity > 0, node.occludedFraction < 1 else { return false }
        return snapshot.viewRect(for: node, viewport: view.bounds.size, image: imageSize).intersection(view.bounds).isEmpty == false
    }

    func contains(_ element: PageAccessibilityElement) -> Bool {
        snapshot != nil && elements[element.node.id] === element
    }

    func focusedElement() -> PageAccessibilityElement? {
        guard snapshot != nil, let view, view.window?.firstResponder === view,
              let id = readingID ?? snapshot?.nodes.first(where: { $0.state.focused })?.id else { return nil }
        return elements[id]
    }

    func hitTest(_ point: NSPoint) -> PageAccessibilityElement? {
        snapshot?.nodes.reversed().first(where: { isVisible($0) && frame(for: $0).contains(point) })
            .flatMap { elements[$0.id] }
    }

    func allows(_ element: PageAccessibilityElement, _ action: PageAccessibilityAction) -> Bool {
        if action == .reveal || action == .focus && [.heading, .link, .image, .list, .custom].contains(where: { rotorMatches($0, element.node.role) }) {
            return contains(element) && element.node.opacity > 0 && element.node.bounds.width > 0 && element.node.bounds.height > 0
        }
        guard snapshot != nil, elements[element.node.id] === element, !element.node.state.disabled, isVisible(element.node),
              !element.node.occluded else { return false }
        if element.node.role == .option, [.press, .select, .deselect].contains(action) {
            return element.node.parent.flatMap { elements[$0] }?.node.state.selectList == true
        }
        switch (action, element.node.role) {
        case (.press, .link), (.press, .button): return true
        case (.press, .textBox), (.focus, .textBox): return element.node.state.nativeTextInput || element.node.state.nativeFocusable
        case (.press, .checkBox), (.press, .comboBox), (.focus, .comboBox): return element.node.state.nativeFocusable
        default: return false
        }
    }

    func act(_ element: PageAccessibilityElement, _ action: PageAccessibilityAction) -> Bool {
        guard allows(element, action), let snapshot else { return false }
        let reading = action == .reveal || action == .focus && ![.textBox, .comboBox].contains(element.node.role)
        guard perform(snapshot, epoch, element.node, reading ? .reveal : action) else { return false }
        readingID = reading ? element.node.id : nil
        return true
    }

    func textResult(_ element: PageAccessibilityElement, _ action: AccessibilityTextAction) -> AccessibilityTextResult? {
        guard contains(element), let snapshot, !element.node.state.fileInput,
              !element.node.state.disabled, !element.node.occluded,
              element.node.supportsDocumentText || element.node.role == .textBox
                && (element.node.state.nativeTextInput || element.node.state.nativeFocusable) else { return nil }
        let result = text?(snapshot, epoch, element.node, action)
        if result != nil && action.changesFocus, let view {
            readingID = element.node.supportsDocumentText ? element.node.id : nil
            view.window?.makeFirstResponder(view)
            NSAccessibility.post(element: element, notification: .selectedTextChanged)
        }
        return result
    }

    func screenFrame(_ bounds: PageNode.Bounds) -> NSRect {
        guard let view, let window = view.window, let snapshot, imageSize.width > 0, imageSize.height > 0 else { return .zero }
        let rect = NSRect(x: bounds.x * view.bounds.width / imageSize.width,
            y: (bounds.y - snapshot.scrollY) * view.bounds.height / imageSize.height,
            width: bounds.width * view.bounds.width / imageSize.width, height: bounds.height * view.bounds.height / imageSize.height)
        return window.convertToScreen(view.convert(rect, to: nil))
    }

    func documentPoint(_ point: NSPoint) -> NSPoint? {
        guard let view, let window = view.window, let snapshot, imageSize.width > 0, imageSize.height > 0,
              view.bounds.width > 0, view.bounds.height > 0, point.x.isFinite, point.y.isFinite else { return nil }
        let local = view.convert(window.convertFromScreen(NSRect(origin: point, size: .zero)).origin, from: nil)
        guard view.bounds.contains(local) else { return nil }
        return NSPoint(x: local.x * imageSize.width / view.bounds.width,
            y: local.y * imageSize.height / view.bounds.height + snapshot.scrollY)
    }
}

@MainActor
final class PageAccessibilityElement: NSAccessibilityElement, @MainActor NSAccessibilityElementProtocol {
    fileprivate var node: PageNode
    private weak var tree: PageAccessibilityTree?

    init(tree: PageAccessibilityTree, node: PageNode) {
        self.tree = tree
        self.node = node
        super.init()
        setAccessibilityElement(true)
        setAccessibilityIdentifier("page-node-\(node.id)")
    }

    fileprivate func invalidate() {
        tree = nil
        setAccessibilityChildren([])
        setAccessibilityParent(nil)
    }

    override func accessibilityRole() -> NSAccessibility.Role? {
        switch node.role {
        case .heading:
            if #available(macOS 26, *) { return NSAccessibility.Role(rawValue: "AXHeading") }
            return .staticText
        case .link: return .link
        case .button: return .button
        case .textBox: return .textField
        case .checkBox: return node.state.radio ? .radioButton : .checkBox
        case .slider: return .slider
        case .comboBox: return node.state.selectList ? .list : .comboBox
        case .list: return .list
        case .paragraph: return .staticText
        case .image: return .image
        case .option, .listItem, .generic: return .group
        }
    }
    override func accessibilityIdentifier() -> String { "page-node-\(node.id)" }

    override func accessibilitySubrole() -> NSAccessibility.Subrole? {
        node.state.protected && node.role == .textBox ? .secureTextField : nil
    }

    override func accessibilityRoleDescription() -> String? {
        if case .heading(let level) = node.role { return "Heading level \(level)" }
        return super.accessibilityRoleDescription()
    }
    override func accessibilityLabel() -> String? { node.name }
    override func accessibilityValue() -> Any? {
        if node.state.fileInput { return node.state.value }
        switch node.role {
        case .checkBox: return node.state.checked.map { NSNumber(value: $0) }
        case .textBox:
            if node.state.protected { return nil }
            return textState()?.text ?? (tree?.contains(self) == true ? node.state.value : nil)
        case .slider, .comboBox: return node.state.protected ? nil : node.state.value
        case .heading, .paragraph: return node.name
        default: return nil
        }
    }
    override func accessibilitySelectedChildren() -> [Any]? {
        guard node.role == .comboBox, node.state.selectList else { return super.accessibilitySelectedChildren() }
        return accessibilityChildren()?.compactMap { child in
            guard let child = child as? PageAccessibilityElement, child.node.state.selected else { return nil }
            return child
        }
    }
    override func accessibilityFrame() -> NSRect { tree?.frame(for: node) ?? .zero }
    override func isAccessibilityEnabled() -> Bool { tree?.contains(self) == true && !node.state.disabled }
    override func isAccessibilityRequired() -> Bool { node.state.required }
    override func isAccessibilitySelected() -> Bool { node.state.selected || node.state.radio && node.state.checked == true }
    override func setAccessibilitySelected(_ selected: Bool) {
        guard selected != node.state.selected else { return }
        _ = tree?.act(self, selected ? .select : .deselect)
    }
    override func isAccessibilityFocused() -> Bool { tree?.focusedElement() === self }
    override func setAccessibilityFocused(_ focused: Bool) {
        if focused { _ = tree?.act(self, .focus) }
        else { tree?.clearReadingFocus(self) }
    }
    override func accessibilityPerformPress() -> Bool { tree?.act(self, .press) ?? false }
    override func isAccessibilitySelectorAllowed(_ selector: Selector) -> Bool {
        if selector == #selector(setAccessibilitySelected(_:)) { return tree?.allows(self, .select) ?? false }
        if selector == #selector(accessibilityPerformPress) { return tree?.allows(self, .press) ?? false }
        if selector == #selector(setAccessibilityFocused(_:)) { return tree?.allows(self, .focus) ?? false }
        if selector == #selector(setAccessibilityValue(_:)) || selector == #selector(setAccessibilitySelectedText(_:)) {
            return textState()?.writable == true
        }
        if selector == #selector(setAccessibilitySelectedTextRange(_:)) || selector == #selector(setAccessibilitySelectedTextRanges(_:))
            || selector == #selector(setAccessibilityVisibleCharacterRange(_:)) { return textState() != nil }
        let name = NSStringFromSelector(selector)
        if ["setAccessibilityNumberOfCharacters:", "setAccessibilityInsertionPointLineNumber:",
            "setAccessibilitySharedCharacterRange:", "setAccessibilitySharedTextUIElements:"].contains(name) { return false }
        if ["accessibilityNumberOfCharacters", "accessibilityVisibleCharacterRange", "accessibilitySelectedText",
            "accessibilitySelectedTextRange", "accessibilitySelectedTextRanges", "accessibilityInsertionPointLineNumber",
            "accessibilityAttributedStringForRange:", "accessibilityRangeForLine:", "accessibilityStringForRange:",
            "accessibilityRangeForPosition:", "accessibilityRangeForIndex:", "accessibilityFrameForRange:",
            "accessibilityRTFForRange:", "accessibilityStyleRangeForIndex:", "accessibilityLineForIndex:"].contains(name) {
            return textState() != nil
        }
        return super.isAccessibilitySelectorAllowed(selector)
    }

    private func textState() -> AccessibilityTextState? {
        guard case .state(let state) = tree?.textResult(self, .inspect) else { return nil }; return state
    }
    private var textLimit: UInt32 { node.supportsDocumentText ? 2_097_152 : 65_536 }
    private func textRange(_ action: AccessibilityTextAction) -> NSRange {
        guard case .range(let range) = tree?.textResult(self, action), let range else { return NSRange(location: NSNotFound, length: 0) }
        return range.nsRange
    }
    override func accessibilityNumberOfCharacters() -> Int { Int(textState()?.text_length ?? 0) }
    override func accessibilityVisibleCharacterRange() -> NSRange { textState()?.visible_range.nsRange ?? NSRange(location: NSNotFound, length: 0) }
    override func accessibilitySelectedTextRange() -> NSRange { textState()?.selection?.nsRange ?? NSRange(location: NSNotFound, length: 0) }
    override func accessibilitySelectedTextRanges() -> [NSValue]? {
        guard let range = textState()?.selection?.nsRange else { return nil }; return [NSValue(range: range)]
    }
    override func accessibilityInsertionPointLineNumber() -> Int { textState()?.insertion_line.map(Int.init) ?? NSNotFound }
    override func accessibilitySelectedText() -> String? {
        guard let state = textState(), let range = state.selection else { return nil }; return state.substring(range.nsRange)
    }
    override func accessibilityString(for range: NSRange) -> String? { textState()?.substring(range) }
    override func accessibilityAttributedString(for range: NSRange) -> NSAttributedString? {
        guard let state = textState(), let text = state.substring(range) else { return nil }
        let color = state.style.color.map { CGFloat($0) / 255 }
        return NSAttributedString(string: text, attributes: [.accessibilityForegroundColor: NSColor(srgbRed: color[0], green: color[1], blue: color[2], alpha: color[3]).cgColor])
    }
    override func accessibilityRTF(for range: NSRange) -> Data? {
        guard let text = accessibilityString(for: range) else { return nil }
        let value = NSAttributedString(string: text)
        return try? value.data(from: NSRange(location: 0, length: value.length), documentAttributes: [.documentType: NSAttributedString.DocumentType.rtf])
    }
    override func accessibilityRange(for index: Int) -> NSRange {
        guard (0...Int(textLimit)).contains(index) else { return NSRange(location: NSNotFound, length: 0) }
        return textRange(.rangeForIndex(UInt32(index)))
    }
    override func accessibilityRange(forLine line: Int) -> NSRange {
        guard (0...Int(textLimit)).contains(line) else { return NSRange(location: NSNotFound, length: 0) }
        return textRange(.rangeForLine(UInt32(line)))
    }
    override func accessibilityLine(for index: Int) -> Int {
        guard (0...Int(textLimit)).contains(index), case .index(let line) = tree?.textResult(self, .lineForIndex(UInt32(index))) else { return NSNotFound }
        return line.map(Int.init) ?? NSNotFound
    }
    override func accessibilityRange(for position: NSPoint) -> NSRange {
        guard let point = tree?.documentPoint(position) else { return NSRange(location: NSNotFound, length: 0) }
        return textRange(.rangeForPosition(point.x, point.y))
    }
    override func accessibilityFrame(for range: NSRange) -> NSRect {
        guard let range = TextRange.replacement(range, limit: textLimit), case .bounds(let bounds) = tree?.textResult(self, .bounds(range)), let bounds else { return .zero }
        return tree?.screenFrame(bounds) ?? .zero
    }
    override func accessibilityStyleRange(for index: Int) -> NSRange {
        guard let state = textState(), !state.protected, index >= 0, index < Int(state.text_length) else { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: 0, length: Int(state.text_length))
    }
    override func setAccessibilityValue(_ value: Any?) {
        guard let text = value as? String, text.utf16.count <= 65_536 else { return }
        _ = tree?.textResult(self, .setValue(text))
    }
    override func setAccessibilitySelectedText(_ text: String?) {
        guard let text, text.utf16.count <= 65_536 else { return }
        _ = tree?.textResult(self, .replaceSelection(text))
    }
    override func setAccessibilitySelectedTextRange(_ range: NSRange) {
        guard let range = TextRange.replacement(range, limit: textLimit) else { return }; _ = tree?.textResult(self, .select(range))
    }
    override func setAccessibilitySelectedTextRanges(_ ranges: [NSValue]?) {
        guard let ranges, ranges.count == 1 else { return }; setAccessibilitySelectedTextRange(ranges[0].rangeValue)
    }
    override func setAccessibilityVisibleCharacterRange(_ range: NSRange) {
        guard let range = TextRange.replacement(range, limit: textLimit) else { return }; _ = tree?.textResult(self, .scrollToRange(range))
    }
}
