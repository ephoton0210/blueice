// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

struct PageViewport: NSViewRepresentable {
    @ObservedObject var model: BrowserModel

    func makeNSView(context: Context) -> CorePageView {
        let view = CorePageView()
        view.model = model
        view.setAccessibilityElement(true)
        view.setAccessibilityRole(.group)
        view.setAccessibilityLabel("Browser page")
        view.setAccessibilityIdentifier("page")
        return view
    }

    func updateNSView(_ view: CorePageView, context: Context) {
        view.image = model.image
        view.setAccessibilityValue(model.image.map { "Rendered \($0.width) × \($0.height), frame \(model.generation)" } ?? "No rendered page")
        view.accessibilityTree.update(model.representation, epoch: model.accessibilityEpoch,
                                      imageSize: model.image.map { CGSize(width: $0.width, height: $0.height) } ?? .zero, tab: model.selected)
        view.needsDisplay = true
    }
}

final class CorePageView: NSView, NSTextInputClient {
    weak var model: BrowserModel?
    var image: CGImage?
    lazy var accessibilityTree = PageAccessibilityTree(view: self) { [weak self] snapshot, epoch, node, _ in
        guard let self, self.model?.accessibilityAction(snapshot, epoch: epoch, node: node) == true else { return false }
        self.window?.makeFirstResponder(self)
        return true
    }
    private var marked = NSAttributedString(string: "")
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.white.setFill()
        bounds.fill()
        if let image {
            NSImage(cgImage: image, size: bounds.size).draw(in: bounds, from: .zero,
                operation: .copy, fraction: 1, respectFlipped: true, hints: [.interpolation: NSImageInterpolation.none])
        }
    }

    override func layout() { super.layout(); reportSize() }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); reportSize() }
    override func viewDidChangeBackingProperties() { super.viewDidChangeBackingProperties(); reportSize() }
    private func reportSize() { model?.viewportChanged(convertToBacking(bounds).size) }

    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted { NSAccessibility.post(element: accessibilityTree.focusedElement() ?? self, notification: .focusedUIElementChanged) }
        return accepted
    }

    override func accessibilityHitTest(_ point: NSPoint) -> Any? {
        accessibilityTree.hitTest(point) ?? self
    }
    override var accessibilityFocusedUIElement: Any? {
        accessibilityTree.focusedElement() ?? self
    }

    override func mouseDown(with event: NSEvent) {
        guard let image, bounds.width > 0, bounds.height > 0 else { return }
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        model?.action(.values("Click", ["x": .number(point.x * Double(image.width) / bounds.width),
                                       "y": .number(point.y * Double(image.height) / bounds.height)]))
    }

    override func scrollWheel(with event: NSEvent) {
        model?.action(.values("Scroll", ["delta_y": .number(-event.scrollingDeltaY * (event.hasPreciseScrollingDeltas ? 1 : 20))]))
    }

    override func keyDown(with event: NSEvent) { interpretKeyEvents([event]) }
    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        unmarkText()
        if !text.isEmpty { model?.action(.values("InsertText", ["text": .string(text)])) }
    }
    override func doCommand(by selector: Selector) {
        if selector == #selector(NSResponder.deleteBackward(_:)) { model?.action(.unit("DeleteBackward")) }
        else if selector == #selector(NSResponder.insertNewline(_:)) { model?.action(.values("InsertText", ["text": .string("\n")])) }
    }
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        marked = (string as? NSAttributedString) ?? NSAttributedString(string: string as? String ?? "")
    }
    func unmarkText() { marked = NSAttributedString(string: "") }
    func hasMarkedText() -> Bool { marked.length > 0 }
    func markedRange() -> NSRange { NSRange(location: hasMarkedText() ? 0 : NSNotFound, length: marked.length) }
    func selectedRange() -> NSRange { NSRange(location: NSNotFound, length: 0) }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? { nil }
    func characterIndex(for point: NSPoint) -> Int { NSNotFound }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        window?.convertToScreen(convert(NSRect(x: 0, y: 0, width: 1, height: 20), to: nil)) ?? .zero
    }
}
