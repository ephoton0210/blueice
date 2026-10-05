// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

enum PageAccessibilityAction: Equatable { case press, focus, select, deselect }

@MainActor
final class PageAccessibilityTree {
    private weak var view: NSView?
    private let perform: (PageRepresentation, UInt64, PageNode, PageAccessibilityAction) -> Bool
    private let text: ((PageRepresentation, UInt64, PageNode, AccessibilityTextAction) -> AccessibilityTextResult?)?
    private(set) var elements: [UInt64: PageAccessibilityElement] = [:]
    private var snapshot: PageRepresentation?
    private var epoch: UInt64 = 0
    private var tabID: UInt64?
    private var frameSource: UInt64?
    private var focusedID: UInt64?
    private var imageSize = CGSize.zero

    init(view: NSView, text: ((PageRepresentation, UInt64, PageNode, AccessibilityTextAction) -> AccessibilityTextResult?)? = nil,
         perform: @escaping (PageRepresentation, UInt64, PageNode, PageAccessibilityAction) -> Bool) {
        self.view = view
        self.text = text
        self.perform = perform
    }

    func update(_ value: PageRepresentation?, epoch: UInt64, imageSize: CGSize, tab: UInt64? = nil) {
        guard let view else { return }
        let documentTab = value?.tabID ?? tab
        let sameDocument = self.epoch == epoch && tabID == documentTab && (value == nil || frameSource == value?.frameSource)
        let changed = self.epoch != epoch || snapshot?.generation != value?.generation || snapshot?.tabID != value?.tabID
        let previousFocus = focusedID
        if !sameDocument {
            focusedID = nil
            for element in elements.values { element.invalidate() }
            elements.removeAll()
        }
        snapshot = value
        tabID = documentTab
        if let value { frameSource = value.frameSource }
        self.epoch = epoch
        self.imageSize = imageSize
        let ids = Set(value?.nodes.map(\.id) ?? [])
        for id in Array(elements.keys) where value != nil && !ids.contains(id) { elements.removeValue(forKey: id)?.invalidate() }
        for node in value?.nodes ?? [] {
            let element = elements[node.id] ?? PageAccessibilityElement(tree: self, node: node)
            element.node = node
            elements[node.id] = element
        }
        for node in value?.nodes ?? [] {
            guard let element = elements[node.id] else { continue }
            element.setAccessibilityParent(node.parent.flatMap { elements[$0] } ?? view as Any)
            element.setAccessibilityChildren(node.children.compactMap { elements[$0] })
        }
        let roots = value?.nodes.filter { $0.parent == nil }.compactMap { elements[$0.id] } ?? []
        view.setAccessibilityChildren(roots)
        view.setAccessibilityVisibleChildren(roots.filter { isVisible($0.node) })
        if changed && value != nil { NSAccessibility.post(element: view, notification: .layoutChanged) }
        if changed {
            for node in value?.nodes ?? [] {
                if let element = elements[node.id] { NSAccessibility.post(element: element, notification: .valueChanged) }
            }
        }
        if let value { focusedID = value.nodes.first(where: { $0.state.focused })?.id }
        if let focused = value?.nodes.first(where: { $0.state.focused }), (!sameDocument || previousFocus != focused.id),
           let element = elements[focused.id], view.window?.firstResponder === view {
            NSAccessibility.post(element: element, notification: .focusedUIElementChanged)
        }
    }

    func frame(for node: PageNode) -> NSRect {
        guard let view, let window = view.window, let snapshot else { return .zero }
        let rect = snapshot.viewRect(for: node, viewport: view.bounds.size, image: imageSize)
        return window.convertToScreen(view.convert(rect, to: nil))
    }

    func isVisible(_ node: PageNode) -> Bool {
        guard let view, let snapshot, node.opacity > 0, node.occludedFraction < 1 else { return false }
        return snapshot.viewRect(for: node, viewport: view.bounds.size, image: imageSize).intersection(view.bounds).isEmpty == false
    }

    func contains(_ element: PageAccessibilityElement) -> Bool {
        snapshot != nil && elements[element.node.id] === element
    }

    func focusedElement() -> PageAccessibilityElement? {
        guard let view, view.window?.firstResponder === view,
              let id = snapshot?.nodes.first(where: { $0.state.focused })?.id else { return nil }
        return elements[id]
    }

    func hitTest(_ point: NSPoint) -> PageAccessibilityElement? {
        snapshot?.nodes.reversed().first(where: { isVisible($0) && frame(for: $0).contains(point) })
            .flatMap { elements[$0.id] }
    }

    func allows(_ element: PageAccessibilityElement, _ action: PageAccessibilityAction) -> Bool {
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
        return perform(snapshot, epoch, element.node, action)
    }

    func textResult(_ element: PageAccessibilityElement, _ action: AccessibilityTextAction) -> AccessibilityTextResult? {
        guard contains(element), let snapshot, element.node.role == .textBox, !element.node.state.fileInput,
              !element.node.state.disabled, !element.node.occluded,
              element.node.state.nativeTextInput || element.node.state.nativeFocusable else { return nil }
        let result = text?(snapshot, epoch, element.node, action)
        if result != nil && action.changesFocus, let view {
            view.window?.makeFirstResponder(view)
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
final class PageAccessibilityElement: NSAccessibilityElement {
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
        guard (0...65_536).contains(index) else { return NSRange(location: NSNotFound, length: 0) }
        return textRange(.rangeForIndex(UInt32(index)))
    }
    override func accessibilityRange(forLine line: Int) -> NSRange {
        guard (0...65_536).contains(line) else { return NSRange(location: NSNotFound, length: 0) }
        return textRange(.rangeForLine(UInt32(line)))
    }
    override func accessibilityLine(for index: Int) -> Int {
        guard (0...65_536).contains(index), case .index(let line) = tree?.textResult(self, .lineForIndex(UInt32(index))) else { return NSNotFound }
        return line.map(Int.init) ?? NSNotFound
    }
    override func accessibilityRange(for position: NSPoint) -> NSRange {
        guard let point = tree?.documentPoint(position) else { return NSRange(location: NSNotFound, length: 0) }
        return textRange(.rangeForPosition(point.x, point.y))
    }
    override func accessibilityFrame(for range: NSRange) -> NSRect {
        guard let range = TextRange.replacement(range), case .bounds(let bounds) = tree?.textResult(self, .bounds(range)), let bounds else { return .zero }
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
        guard let range = TextRange.replacement(range) else { return }; _ = tree?.textResult(self, .select(range))
    }
    override func setAccessibilitySelectedTextRanges(_ ranges: [NSValue]?) {
        guard let ranges, ranges.count == 1 else { return }; setAccessibilitySelectedTextRange(ranges[0].rangeValue)
    }
    override func setAccessibilityVisibleCharacterRange(_ range: NSRange) {
        guard let range = TextRange.replacement(range) else { return }; _ = tree?.textResult(self, .scrollToRange(range))
    }
}
