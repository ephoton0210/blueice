// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

enum PageAccessibilityAction { case press, focus }

@MainActor
final class PageAccessibilityTree {
    private weak var view: NSView?
    private let perform: (PageRepresentation, UInt64, PageNode, PageAccessibilityAction) -> Bool
    private(set) var elements: [UInt64: PageAccessibilityElement] = [:]
    private var snapshot: PageRepresentation?
    private var epoch: UInt64 = 0
    private var tabID: UInt64?
    private var frameSource: UInt64?
    private var focusedID: UInt64?
    private var imageSize = CGSize.zero

    init(view: NSView, perform: @escaping (PageRepresentation, UInt64, PageNode, PageAccessibilityAction) -> Bool) {
        self.view = view
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
        switch (action, element.node.role) {
        case (.press, .link), (.press, .button): return true
        case (.press, .textBox), (.focus, .textBox): return element.node.state.nativeTextInput || element.node.state.nativeFocusable
        case (.press, .checkBox): return element.node.state.nativeFocusable
        default: return false
        }
    }

    func act(_ element: PageAccessibilityElement, _ action: PageAccessibilityAction) -> Bool {
        guard allows(element, action), let snapshot else { return false }
        return perform(snapshot, epoch, element.node, action)
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
        case .comboBox: return .comboBox
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
        switch node.role {
        case .checkBox: return node.state.checked.map { NSNumber(value: $0) }
        case .textBox, .slider, .comboBox: return node.state.protected ? nil : node.state.value
        case .heading, .paragraph: return node.name
        default: return nil
        }
    }
    override func accessibilityFrame() -> NSRect { tree?.frame(for: node) ?? .zero }
    override func isAccessibilityEnabled() -> Bool { tree?.contains(self) == true && !node.state.disabled }
    override func isAccessibilityRequired() -> Bool { node.state.required }
    override func isAccessibilitySelected() -> Bool { node.state.selected || node.state.radio && node.state.checked == true }
    override func isAccessibilityFocused() -> Bool { tree?.focusedElement() === self }
    override func setAccessibilityFocused(_ focused: Bool) {
        if focused { _ = tree?.act(self, .focus) }
    }
    override func accessibilityPerformPress() -> Bool { tree?.act(self, .press) ?? false }
    override func isAccessibilitySelectorAllowed(_ selector: Selector) -> Bool {
        if selector == #selector(accessibilityPerformPress) { return tree?.allows(self, .press) ?? false }
        if selector == #selector(setAccessibilityFocused(_:)) { return tree?.allows(self, .focus) ?? false }
        // A native text field role does not imply DOM value/selection mutation support.
        if selector == #selector(setAccessibilityValue(_:)) { return false }
        return super.isAccessibilitySelectorAllowed(selector)
    }
}
