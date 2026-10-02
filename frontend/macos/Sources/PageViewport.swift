// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

struct PageViewport: NSViewRepresentable {
    @ObservedObject var model: BrowserModel
    @EnvironmentObject var editingMenu: NativeEditingMenu

    func makeNSView(context: Context) -> CorePageView {
        let view = CorePageView()
        view.model = model
        view.editingMenu = editingMenu
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
        view.refreshTextInput()
    }
}

final class CorePageView: NSView, NSTextInputClient {
    weak var model: BrowserModel?
    weak var editingMenu: NativeEditingMenu?
    var image: CGImage?
    lazy var accessibilityTree = PageAccessibilityTree(view: self) { [weak self] snapshot, epoch, node, _ in
        guard let self, self.model?.accessibilityAction(snapshot, epoch: epoch, node: node) == true else { return false }
        self.window?.makeFirstResponder(self)
        return true
    }
    private var pendingMarked: NSRange?
    private var pendingSelection: NSRange?
    private var pendingContext: TextInputContext?
    private var pendingUnmark: TextInputContext?
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
        if accepted {
            editingMenu?.focus(self)
            NSAccessibility.post(element: accessibilityTree.focusedElement() ?? self, notification: .focusedUIElementChanged)
        }
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
        pendingMarked = nil; pendingSelection = nil; pendingContext = nil
        model?.focusPage(x: point.x * Double(image.width) / bounds.width,
                         y: point.y * Double(image.height) / bounds.height,
                         extend: event.modifierFlags.contains(.shift), clickCount: event.clickCount)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let image, bounds.width > 0, bounds.height > 0 else { return }
        let point = convert(event.locationInWindow, from: nil)
        model?.textInput(.pointer(point.x * Double(image.width) / bounds.width,
                                  point.y * Double(image.height) / bounds.height, true, 1))
    }

    override func scrollWheel(with event: NSEvent) {
        model?.action(.values("Scroll", ["delta_y": .number(-event.scrollingDeltaY * (event.hasPreciseScrollingDeltas ? 1 : 20))]))
    }

    override func keyDown(with event: NSEvent) {
        if !performKeyEquivalent(with: event), inputContext?.handleEvent(event) != true { interpretKeyEvents([event]) }
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard event.modifierFlags.intersection([.command, .option, .control]) == .command,
              window?.firstResponder === self,
              model?.textInputState?.focused != nil else { return false }
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "a": selectAll(nil)
        case "c": copy(nil)
        case "x": cut(nil)
        case "v": paste(nil)
        default: return false
        }
        return true
    }

    func refreshTextInput() {
        let context = model?.textInputState?.context
        if pendingContext != context || model?.textInputBusy != true {
            pendingMarked = nil; pendingSelection = nil; pendingContext = nil
        }
        if pendingUnmark != context || model?.textInputBusy != true { pendingUnmark = nil }
        if window?.firstResponder === self { inputContext?.invalidateCharacterCoordinates() }
    }

    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        guard text.utf16.count <= 65_536,
              replacementRange.location == NSNotFound || TextRange.replacement(replacementRange) != nil else { return }
        pendingMarked = nil; pendingSelection = nil; pendingContext = nil
        pendingUnmark = model?.textInputState?.context
        model?.textInput(.replace(text, TextRange.replacement(replacementRange)))
    }
    override func doCommand(by selector: Selector) {
        let name = NSStringFromSelector(selector)
        let extend = name.contains("AndModifySelection")
        let movement: TextMovement?
        switch name.replacingOccurrences(of: "AndModifySelection", with: "") {
        case "moveLeft:", "moveBackward:": movement = .backward
        case "moveRight:", "moveForward:": movement = .forward
        case "moveWordLeft:", "moveWordBackward:": movement = .wordBackward
        case "moveWordRight:", "moveWordForward:": movement = .wordForward
        case "moveToBeginningOfDocument:": movement = .beginning
        case "moveToEndOfDocument:": movement = .end
        case "moveToBeginningOfLine:", "moveToLeftEndOfLine:": movement = .lineBeginning
        case "moveToEndOfLine:", "moveToRightEndOfLine:": movement = .lineEnd
        case "moveUp:": movement = .up
        case "moveDown:": movement = .down
        default: movement = nil
        }
        if let movement { model?.textInput(.move(movement, extend)); return }
        switch name {
        case "deleteBackward:": model?.textInput(.delete(false))
        case "deleteForward:": model?.textInput(.delete(true))
        case "deleteWordBackward:": model?.textInput(.move(.wordBackward, true)); model?.textInput(.delete(false))
        case "deleteWordForward:": model?.textInput(.move(.wordForward, true)); model?.textInput(.delete(true))
        case "deleteToBeginningOfLine:": model?.textInput(.move(.lineBeginning, true)); model?.textInput(.delete(false))
        case "deleteToEndOfLine:": model?.textInput(.move(.lineEnd, true)); model?.textInput(.delete(true))
        case "insertNewline:", "insertLineBreak:": insertText("\n", replacementRange: NSRange(location: NSNotFound, length: 0))
        case "insertTab:": if model?.textInputState?.focused?.multiline == true { insertText("\t", replacementRange: NSRange(location: NSNotFound, length: 0)) }
        case "cancelOperation:": model?.textInput(.cancelComposition); pendingMarked = nil; pendingSelection = nil
        case "selectAll:": selectAll(nil)
        default: break
        }
    }
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        guard text.utf16.count <= 65_536, let selection = TextRange.replacement(selectedRange),
              selection.valid(length: UInt32(text.utf16.count)),
              replacementRange.location == NSNotFound || TextRange.replacement(replacementRange) != nil else { return }
        let replacing = TextRange.replacement(replacementRange)?.nsRange ?? (hasMarkedText() ? markedRange() : self.selectedRange())
        guard replacing.location != NSNotFound else { return }
        pendingMarked = NSRange(location: replacing.location, length: text.utf16.count)
        pendingUnmark = nil
        pendingSelection = NSRange(location: replacing.location + selectedRange.location, length: selectedRange.length)
        pendingContext = model?.textInputState?.context
        model?.textInput(.compose(text, selection, TextRange.replacement(replacementRange)))
    }
    func unmarkText() {
        pendingUnmark = model?.textInputState?.context
        model?.textInput(.finishComposition); pendingMarked = nil; pendingSelection = nil
    }
    func hasMarkedText() -> Bool { markedRange().location != NSNotFound }
    func markedRange() -> NSRange {
        refreshPendingContext()
        if pendingUnmark != nil { return NSRange(location: NSNotFound, length: 0) }
        return pendingMarked ?? model?.textInputState?.focused?.marked?.nsRange ?? NSRange(location: NSNotFound, length: 0)
    }
    func selectedRange() -> NSRange {
        refreshPendingContext()
        return pendingSelection ?? model?.textInputState?.focused?.selection.nsRange ?? NSRange(location: NSNotFound, length: 0)
    }
    private func refreshPendingContext() {
        if pendingContext != model?.textInputState?.context || model?.textInputBusy != true {
            pendingMarked = nil; pendingSelection = nil; pendingContext = nil
        }
        if pendingUnmark != model?.textInputState?.context || model?.textInputBusy != true { pendingUnmark = nil }
    }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        actualRange?.pointee = NSRange(location: NSNotFound, length: 0)
        guard let field = model?.textInputState?.focused, !field.protected, let text = field.text,
              let checked = TextRange.replacement(range), checked.valid(length: field.text_length) else { return nil }
        let value = text as NSString
        let expanded = value.rangeOfComposedCharacterSequences(for: checked.nsRange)
        actualRange?.pointee = expanded
        return NSAttributedString(string: value.substring(with: expanded))
    }
    func characterIndex(for point: NSPoint) -> Int {
        guard let state = model?.textInputState, let field = state.focused, let window, let image else { return NSNotFound }
        let local = convert(window.convertPoint(fromScreen: point), from: nil)
        let size = CGSize(width: image.width, height: image.height)
        guard state.viewRect(field.bounds, viewport: bounds.size, image: size).contains(local) else { return NSNotFound }
        return field.carets.min {
            func distance(_ caret: TextControlState.Caret) -> Double {
                let rect = state.viewRect(caret.bounds, viewport: bounds.size, image: size)
                return hypot(rect.midX - local.x, rect.midY - local.y)
            }
            return distance($0) < distance($1)
        }.map { Int($0.offset) } ?? NSNotFound
    }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        actualRange?.pointee = NSRange(location: NSNotFound, length: 0)
        refreshPendingContext()
        guard let state = model?.textInputState, let field = state.focused, let window, let image,
              range.location != NSNotFound, range.location >= 0 else { return .zero }
        let pending = pendingMarked.map { range.location >= $0.location && range.location <= NSMaxRange($0) } ?? false
        guard range.location <= Int(field.text_length) || pending else { return .zero }
        let caret = range.location <= Int(field.text_length) ? field.carets.last { Int($0.offset) <= range.location } : nil
        let rect = state.viewRect(caret?.bounds ?? field.caret, viewport: bounds.size,
                                  image: CGSize(width: image.width, height: image.height))
        actualRange?.pointee = NSRange(location: Int(caret?.offset ?? field.selection.location), length: 0)
        return window.convertToScreen(convert(rect, to: nil))
    }

    override func selectAll(_ sender: Any?) { model?.textInput(.selectAll) }
    @objc func copy(_ sender: Any?) {
        guard model?.textInputBusy != true, let field = model?.textInputState?.focused, !field.protected,
              field.selection.length > 0, let text = field.text else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString((text as NSString).substring(with: field.selection.nsRange), forType: .string)
    }
    @objc func cut(_ sender: Any?) {
        guard model?.textInputState?.focused?.writable == true, model?.textInputState?.focused?.protected == false,
              model?.textInputBusy != true else { return }
        copy(sender); model?.textInput(.replace("", nil))
    }
    @objc func paste(_ sender: Any?) {
        guard let text = NSPasteboard.general.string(forType: .string), text.utf16.count <= 65_536 else { return }
        model?.textInput(.replace(text, nil))
    }

    override func resignFirstResponder() -> Bool {
        if hasMarkedText() { unmarkText() }
        editingMenu?.focus(nil)
        return super.resignFirstResponder()
    }
}
