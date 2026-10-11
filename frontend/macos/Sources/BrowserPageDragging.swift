// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Darwin

/// AppKit's nonisolated callback can run on a pasteboard service thread.
/// The payload serializes this publication with source revocation.
final class NativeDragPasteboardProvider: NSObject, NSPasteboardItemDataProvider {
    let payload: NativeDragPayload
    init(payload: NativeDragPayload) { self.payload = payload }
    func pasteboard(_ pasteboard: NSPasteboard?, item: NSPasteboardItem,
                    provideDataForType type: NSPasteboard.PasteboardType) {
        payload.provide(type.rawValue) { _ = item.setData($0, forType: type) }
    }
}

enum NativeDragOperation: String, Codable, Hashable, Sendable {
    case none = "None", copy = "Copy", move = "Move", link = "Link"
    var appKit: NSDragOperation {
        switch self { case .none: return []; case .copy: return .copy; case .move: return .move; case .link: return .link }
    }
    static func allowed(_ mask: NSDragOperation) -> [Self] {
        [.copy, .move, .link].filter { mask.contains($0.appKit) }
    }
}

enum NativeDragItem: Codable, Sendable {
    case text(format: String, data: String), file(SelectedFile)
    private enum Keys: String, CodingKey { case Text, File }
    private struct Text: Codable { let format: String; let data: String }
    func encode(to encoder: Encoder) throws {
        var value = encoder.container(keyedBy: Keys.self)
        switch self {
        case .text(let format, let data): try value.encode(Text(format: format,data: data),forKey: .Text)
        case .file(let file): try value.encode(file,forKey: .File)
        }
    }
    init(from decoder: Decoder) throws {
        let value = try decoder.container(keyedBy: Keys.self)
        guard value.allKeys.count == 1 else { throw BrowserFailure.invalid("Invalid drag item.") }
        if value.contains(.Text) {
            let text = try value.decode(Text.self,forKey: .Text); self = .text(format: text.format,data: text.data)
        } else { self = .file(try value.decode(SelectedFile.self,forKey: .File)) }
    }
    static func valid(_ items: [Self]) -> Bool {
        guard items.count <= 64 else { return false }
        var bytes = 0, formats = Set<String>()
        for item in items {
            switch item {
            case .text(let format,let data):
                guard validFormat(format), format != "Files", formats.insert(format.lowercased()).inserted else { return false }
                bytes += data.utf8.count
            case .file(let file):
                guard SelectedFile.validName(file.name), file.media_type.utf8.count <= 255,
                      file.media_type.contains("/"), file.media_type.utf8.allSatisfy({ (33...126).contains($0) }) else { return false }
                bytes += file.bytes.count
            }
            if bytes > SelectedFile.maximumBytes { return false }
        }
        return true
    }
    static func validFormat(_ format: String) -> Bool {
        !format.isEmpty && format.utf8.count <= 255 && format.utf8.allSatisfy { (32...126).contains($0) }
    }
}

enum NativeDragAction: Encodable, Sendable {
    private struct MessageKey: CodingKey {
        let stringValue: String
        var intValue: Int? { nil }
        init(_ value: String) { stringValue = value }
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { return nil }
    }
    case enter(UInt64, CGPoint, [String], [NativeDragOperation])
    case leave(UInt64), drop(UInt64, CGPoint, [NativeDragItem], expected: NativeDragOperation? = nil)
    case start(UInt64, CGPoint, [NativeDragOperation]), sourceMove(UInt64, CGPoint)
    case end(UInt64, NativeDragOperation, at: CGPoint? = nil)
    var id: UInt64 {
        switch self { case .enter(let id,_,_,_), .leave(let id), .drop(let id,_,_,_), .start(let id,_,_), .sourceMove(let id,_), .end(let id,_,_): return id }
    }
    var point: CGPoint? {
        switch self { case .enter(_,let point,_,_), .drop(_,let point,_,_), .start(_,let point,_), .sourceMove(_,let point): return point; default: return nil }
    }
    func encode(to encoder: Encoder) throws {
        var root = encoder.container(keyedBy: MessageKey.self)
        let name: String
        switch self { case .enter: name = "Enter"; case .leave: name = "Leave"; case .drop: name = "Drop"; case .start: name = "Start"; case .sourceMove: name = "SourceMove"; case .end: name = "End" }
        var value = root.nestedContainer(keyedBy: MessageKey.self,forKey: MessageKey(name))
        try value.encode(id,forKey: MessageKey("drag_id"))
        if let point { try value.encode(["x": Double(point.x), "y": Double(point.y)],forKey: MessageKey("point")) }
        switch self {
        case .enter(_,_,let types,let allowed):
            try value.encode(types,forKey: MessageKey("types")); try value.encode(allowed,forKey: MessageKey("allowed_operations"))
        case .start(_,_,let allowed): try value.encode(allowed,forKey: MessageKey("allowed_operations"))
        case .drop(_,_,let items,let expected):
            try value.encode(items,forKey: MessageKey("items"))
            try value.encodeIfPresent(expected,forKey: MessageKey("expected_operation"))
        case .end(_,let operation,let point):
            try value.encode(operation,forKey: MessageKey("operation"))
            if let point { try value.encode(["x":Double(point.x),"y":Double(point.y)],forKey: MessageKey("point")) }
        case .sourceMove: break
        case .leave: break
        }
    }
}

struct NativeDragMouse: Codable, Equatable, Sendable {
    struct Point: Codable, Equatable, Sendable {
        let x: Double, y: Double
        init(_ point: CGPoint) { x = point.x; y = point.y }
        var cgPoint: CGPoint { CGPoint(x: x,y: y) }
        func valid(_ limit: Double) -> Bool { x.isFinite && y.isFinite && abs(x) <= limit && abs(y) <= limit }
    }
    struct Modifiers: Codable, Equatable, Sendable {
        let alt: Bool, control: Bool, meta: Bool, shift: Bool, caps_lock: Bool
        init(_ flags: NSEvent.ModifierFlags) {
            alt = flags.contains(.option); control = flags.contains(.control); meta = flags.contains(.command)
            shift = flags.contains(.shift); caps_lock = flags.contains(.capsLock)
        }
    }
    let client_point: Point, screen_point: Point
    let modifiers: Modifiers
    let buttons: UInt16
    init(client: CGPoint,screen: CGPoint,flags: NSEvent.ModifierFlags,buttons: Int) {
        client_point = Point(client); screen_point = Point(screen); modifiers = Modifiers(flags)
        self.buttons = UInt16(truncatingIfNeeded: buttons)
    }
    var valid: Bool { client_point.valid(16384) && screen_point.valid(1_000_000) }
    var point: CGPoint { client_point.cgPoint }
    func sameNegotiation(as other: NativeDragMouse?) -> Bool {
        guard let other else { return false }
        // Mouse release changes buttons before prepare/drop. Core rechecks
        // dragover with this final state before exposing any dropped content.
        return client_point == other.client_point && screen_point == other.screen_point && modifiers == other.modifiers
    }
}

struct NativeDragState: Decodable, Sendable {
    let version: UInt32
    let drag_id: UInt64
    let frame_source: UInt64
    let document_generation: UInt64
    let frame_generation: UInt64
    let active: Bool
    let operation: NativeDragOperation
    let allowed_operations: [NativeDragOperation]
    let items: [NativeDragItem]?
    let mouse: NativeDragMouse?
    var valid: Bool {
        version == 1 && drag_id > 0 && allowed_operations.count <= 3
            && !allowed_operations.contains(.none) && Set(allowed_operations).count == allowed_operations.count
            && (operation == .none || allowed_operations.contains(operation))
            && items.map(NativeDragItem.valid) != false
            && mouse?.valid != false
    }
}

struct NativeDragOwner: Equatable, Sendable {
    let tab: UInt64
    let browserContext: UInt64
    let window: UInt64
    let epoch: UInt64
    let context: NativePointerContext
    let cssSize: CGSize
}

/// AppKit callbacks receive exact-point Core decisions from bounded synchronous
/// brokers. Destination negotiation carries metadata; only explicit drops carry bytes.
@MainActor
final class BrowserPageDragging: NSObject, NSDraggingSource {
    private struct Press {
        let event: NSEvent
        let owner: NativeDragOwner
        let point: CGPoint
        let viewSize: CGSize
        let preservesSelection: Bool
    }
    @MainActor private final class Source {
        let id: UInt64
        let owner: NativeDragOwner
        weak var model: BrowserModel?
        let allowed: [NativeDragOperation]
        let payload: NativeDragPayload
        let provider: NativeDragPasteboardProvider
        var session: NSDraggingSession?
        var pasteboardChangeCount: Int?
        var heartbeat: NativeDragHeartbeat?
        var ended = false
        var cancelled = false
        init(id: UInt64,owner: NativeDragOwner,model: BrowserModel,allowed: [NativeDragOperation],payload: NativeDragPayload) {
            self.id = id; self.owner = owner; self.model = model; self.allowed = allowed
            self.payload = payload; provider = NativeDragPasteboardProvider(payload: payload)
        }
    }
    @MainActor private final class Destination {
        let token = UUID()
        let id: UInt64
        weak var model: BrowserModel?
        let sequence: Int
        let owner: NativeDragOwner
        let viewSize: CGSize
        let pasteboard: NSPasteboard
        let changeCount: Int
        let types: [String]
        let allowed: [NativeDragOperation]
        var point: CGPoint
        var mouse: NativeDragMouse
        var decisionMouse: NativeDragMouse?
        var decisionFrame: UInt64?
        var operation: NSDragOperation = []
        var prepared = false
        var ended = false
        init(id: UInt64,info: NSDraggingInfo,model: BrowserModel,owner: NativeDragOwner,viewSize: CGSize,point: CGPoint,types: [String],mouse: NativeDragMouse) {
            self.id = id; self.model = model; sequence = info.draggingSequenceNumber; self.owner = owner; self.viewSize = viewSize
            pasteboard = info.draggingPasteboard; changeCount = pasteboard.changeCount
            self.types = types; allowed = NativeDragOperation.allowed(info.draggingSourceOperationMask); self.point = point; self.mouse = mouse
        }
    }
    private weak var view: CorePageView?
    private var destination: Destination?
    private var press: Press?
    private var source: Source?
    private static let formatMetadata = NSPasteboard.PasteboardType("cc.bravotek.blueice.drag-formats")
    private static let maximumMetadataBytes = 64 * (255 * 2 + 3) + 2
    static var pasteboardTypes: [NSPasteboard.PasteboardType] { [.string,.html,.URL,.fileURL,formatMetadata] }
    init(view: CorePageView) { self.view = view; super.init() }

    /// Geometry and static eligibility are supplied by core. No selected text
    /// is read or copied by the shell until core processes an actual drag start.
    func pressed(_ event: NSEvent) -> Bool {
        press = nil
        guard source == nil, event.clickCount == 1, !event.modifierFlags.contains(.shift),
              let view, let window = view.window, window.isVisible, window.isKeyWindow, window.attachedSheet == nil,
              event.windowNumber == window.windowNumber, let model = view.model, !model.textInputBusy,
              let owner = model.nativeDragOwner, let input = model.textInputState, input.focused?.marked == nil,
              let snapshot = model.representation, snapshot.generation == model.generation else { return false }
        let point = view.convert(event.locationInWindow,from: nil)
        guard view.bounds.contains(point), view.bounds.width > 0, view.bounds.height > 0 else { return false }
        let selected: Bool
        if let document = input.document, document.active, document.selection.length > 0 {
            selected = document.selection_rects.contains { input.viewRect($0,viewport: view.bounds.size,image: owner.cssSize).contains(point) }
        } else if let control = input.focused, !control.protected, control.selection.length > 0 {
            selected = control.selection_rects.contains { input.viewRect($0,viewport: view.bounds.size,image: owner.cssSize).contains(point) }
        } else { selected = false }
        let staticSource = snapshot.nodes.contains {
            $0.state.nativeDragSource && !$0.occluded && snapshot.viewRect(for: $0,viewport: view.bounds.size,image: owner.cssSize).contains(point)
        }
        guard selected || staticSource else { return false }
        let css = CGPoint(x: (point.x-view.bounds.minX)*owner.cssSize.width/view.bounds.width,
                          y: (point.y-view.bounds.minY)*owner.cssSize.height/view.bounds.height)
        press = Press(event: event,owner: owner,point: css,viewSize: view.bounds.size,preservesSelection: selected)
        return selected
    }
    func dragged(_ event: NSEvent) -> Bool {
        if source != nil { return true }
        guard let press, let view, let model = view.model else { return false }
        guard model.nativeDragIsCurrent(press.owner), view.bounds.size == press.viewSize,
              event.windowNumber == view.window?.windowNumber, view.window?.isKeyWindow == true,
              view.window?.attachedSheet == nil else { self.press = nil; return false }
        let start = view.convert(press.event.locationInWindow,from: nil), point = view.convert(event.locationInWindow,from: nil)
        guard hypot(point.x-start.x,point.y-start.y) >= 4 else { return press.preservesSelection }
        self.press = nil
        guard let window = view.window,
              let mouse = mouse(window.convertPoint(toScreen: event.locationInWindow),client: CGPoint(
                x: (point.x-view.bounds.minX)*press.owner.cssSize.width/view.bounds.width,
                y: (point.y-view.bounds.minY)*press.owner.cssSize.height/view.bounds.height),flags: event.modifierFlags),
              let id = model.nextNativeDragID(),
              let state = model.startNativeDrag(.start(id,press.point,[.copy,.move,.link]),owner: press.owner,mouse: mouse),
              state.active, let items = state.items else { return true }
        let writer = NSPasteboardItem()
        var formats: [String] = []
        var values: [String: Data] = [:]
        for item in items {
            guard case .text(let format,let data) = item else {
                Task { _ = await model.nativeDrag(.end(id,.none),owner: press.owner) }; return true
            }
            values[Self.pasteboardType(format).rawValue] = Data(data.utf8)
            formats.append(format)
        }
        let payload = NativeDragPayload(values: values)
        let source = Source(id: id,owner: press.owner,model: model,allowed: state.allowed_operations,payload: payload)
        guard values.isEmpty || writer.setDataProvider(source.provider,forTypes: values.keys.map { NSPasteboard.PasteboardType($0) }) else {
            payload.revoke()
            Task { _ = await model.nativeDrag(.end(id,.none),owner: press.owner) }; return true
        }
        guard let metadata = try? JSONEncoder().encode(formats), writer.setData(metadata,forType: Self.formatMetadata) else {
            payload.revoke()
            Task { _ = await model.nativeDrag(.end(id,.none),owner: press.owner) }; return true
        }
        let dragging = NSDraggingItem(pasteboardWriter: writer)
        let label = items.compactMap { item -> String? in
            if case .text(let format,let text) = item, format == "text/plain" { return String(text.prefix(32)) }; return nil
        }.first ?? "BlueIce"
        let preview = NSImage(size: NSSize(width: 128,height: 24),flipped: true) { rect in
            NSColor.selectedContentBackgroundColor.withAlphaComponent(0.85).setFill(); rect.fill()
            (label as NSString).draw(in: rect.insetBy(dx: 4,dy: 3),withAttributes: [.font: NSFont.systemFont(ofSize: 12),.foregroundColor: NSColor.selectedTextColor])
            return true
        }
        dragging.setDraggingFrame(NSRect(x: point.x-4,y: point.y-12,width: 128,height: 24),contents: preview)
        guard model.retainNativeDragPayload(payload,id: id,owner: press.owner) else {
            payload.revoke()
            Task { _ = await model.nativeDrag(.end(id,.none),owner: press.owner) }; return true
        }
        self.source = source
        let session = view.beginDraggingSession(with: [dragging],event: event,source: self)
        if self.source === source {
            source.session = session
            source.pasteboardChangeCount = source.pasteboardChangeCount ?? session.draggingPasteboard.changeCount
        }
        return true
    }
    func released() -> NSEvent? {
        defer { press = nil }
        guard let press, press.preservesSelection, let view,
              view.model?.nativeDragIsCurrent(press.owner) == true, view.bounds.size == press.viewSize,
              press.event.windowNumber == view.window?.windowNumber else { return nil }
        return press.event
    }
    func cancelSource() {
        press = nil
        guard let source else { return }
        self.source = nil
        finishSource(source,operation: .none,point: nil)
    }
    func draggingSession(_ session: NSDraggingSession,sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        guard let source, !source.cancelled, !source.ended, source.session == nil || source.session === session else { return [] }
        guard source.model?.nativeDragIsCurrent(source.owner) == true else {
            cancelSource(); return []
        }
        return source.allowed.reduce([]) { $0.union($1.appKit) }
    }
    func draggingSession(_ session: NSDraggingSession,willBeginAt screenPoint: NSPoint) {
        if let source, source.session == nil || source.session === session {
            source.session = session
            source.pasteboardChangeCount = session.draggingPasteboard.changeCount
            source.heartbeat?.stop()
            source.heartbeat = NativeDragHeartbeat(modes: [.common,.eventTracking]) { [weak self,weak source] in
                guard let self, let source, self.source === source, let session = source.session,
                      !source.ended, !source.cancelled else { return false }
                // The OS owns release/finish. Do not dispatch a late periodic
                // drag after the physical mouse button has been released.
                if NSEvent.pressedMouseButtons & 1 == 0 { return true }
                self.updateSource(source,screenPoint: session.draggingLocation)
                return self.source === source
            }
        }
    }
    func draggingSession(_ session: NSDraggingSession,movedTo screenPoint: NSPoint) {
        guard let source, source.session === session, !source.ended, !source.cancelled else { return }
        updateSource(source,screenPoint: screenPoint)
    }
    private func updateSource(_ source: Source,screenPoint: NSPoint) {
        guard let model = source.model, let point = sourcePoint(screenPoint,source: source), model.nativeDragIsCurrent(source.owner) else {
            cancelSource(); return
        }
        guard let mouse = mouse(screenPoint,client: point) else { cancelSource(); return }
        var reply: NativeDragState?
        let active = source.payload.validateSource {
            reply = model.moveNativeDrag(.sourceMove(source.id,mouse.point),owner: source.owner,mouse: mouse)
            return reply?.active == true
        }
        if !active {
            source.cancelled = reply?.active == false
            cancelSource()
        }
    }
    func draggingSession(_ session: NSDraggingSession,endedAt screenPoint: NSPoint,operation: NSDragOperation) {
        guard let source, source.session === session else { return }
        self.source = nil; press = nil
        let result = source.cancelled ? .none : (source.allowed.first { $0.appKit == operation } ?? .none)
        let point = sourcePoint(screenPoint,source: source)
        finishSource(source,operation: result,point: point,mouse: point.flatMap { mouse(screenPoint,client: $0) })
    }
    private func sourcePoint(_ screenPoint: NSPoint,source: Source) -> CGPoint? {
        guard let view, view.model === source.model, let window = view.window, view.bounds.width > 0, view.bounds.height > 0 else { return nil }
        let point = view.convert(window.convertPoint(fromScreen: screenPoint),from: nil)
        let css = CGPoint(x: (point.x-view.bounds.minX)*source.owner.cssSize.width/view.bounds.width,
                          y: (point.y-view.bounds.minY)*source.owner.cssSize.height/view.bounds.height)
        return css.x.isFinite && css.y.isFinite && abs(css.x) <= 16384 && abs(css.y) <= 16384 ? css : nil
    }
    private func finishSource(_ source: Source,operation: NativeDragOperation,point: CGPoint?,mouse: NativeDragMouse? = nil) {
        source.ended = true
        source.heartbeat?.stop(); source.heartbeat = nil
        // Successful consumers may request other promised formats after
        // the session ends. AppKit retains the provider for those promises.
        if operation == .none || source.cancelled { revokePayload(source) }
        guard let model = source.model else { return }
        model.releaseNativeDragPayload(source.id)
        Task {
            if !source.cancelled { _ = await model.nativeDrag(.end(source.id,operation,at: point),owner: source.owner,mouse: mouse) }
        }
    }
    private func revokePayload(_ source: Source) {
        source.payload.revoke()
        // A drag pasteboard can be replaced by a later owner. Only clear
        // the exact generation this source originally published.
        if let session = source.session, let count = source.pasteboardChangeCount,
           session.draggingPasteboard.changeCount == count {
            session.draggingPasteboard.clearContents()
            source.pasteboardChangeCount = nil
        }
    }
    private func mouse(_ screen: CGPoint,client: CGPoint,flags: NSEvent.ModifierFlags? = nil) -> NativeDragMouse? {
        guard let primary = NSScreen.screens.first else { return nil }
        // AppKit desktop coordinates grow up from the primary display's
        // bottom-left; web screen coordinates grow down from its top-left.
        let value = NativeDragMouse(client: client,screen: CGPoint(x: screen.x-primary.frame.minX,y: primary.frame.maxY-screen.y),
                                    flags: flags ?? NSEvent.modifierFlags,buttons: NSEvent.pressedMouseButtons)
        return value.valid ? value : nil
    }
    private func mouse(_ info: NSDraggingInfo,view: CorePageView,point: CGPoint) -> NativeDragMouse? {
        guard let window = view.window else { return nil }
        return mouse(window.convertPoint(toScreen: info.draggingLocation),client: point)
    }
    static func pasteboardType(_ format: String) -> NSPasteboard.PasteboardType {
        let format = format.lowercased()
        switch format {
        case "text/plain": return .string
        case "text/html": return .html
        case "text/uri-list": return .URL
        default:
            let encoded = format.utf8.map { String(format: "%02x", Int($0)) }.joined()
            return .init("cc.bravotek.blueice.drag.custom." + encoded)
        }
    }

    func entered(_ info: NSDraggingInfo) -> NSDragOperation {
        if let destination, destination.sequence == info.draggingSequenceNumber { return updated(info) }
        invalidate()
        guard let view, usable(info,view: view), let model = view.model,
              let owner = model.nativeDragOwner, let point = point(info,view: view,owner: owner),
              let mouse = mouse(info,view: view,point: point),
              let id = model.nextNativeDragID() else { return [] }
        let types = Self.advertisedTypes(info.draggingPasteboard)
        guard !types.isEmpty || Self.metadataTypes(info.draggingPasteboard) != nil,
              !NativeDragOperation.allowed(info.draggingSourceOperationMask).isEmpty else { return [] }
        let drag = Destination(id: id,info: info,model: model,owner: owner,viewSize: view.bounds.size,point: point,types: types,mouse: mouse)
        destination = drag; negotiate(mouse,drag: drag,model: model)
        return drag.decisionMouse == mouse && drag.decisionFrame == model.generation ? drag.operation : []
    }
    func updated(_ info: NSDraggingInfo) -> NSDragOperation {
        if destination == nil { return entered(info) }
        guard let view, let model = view.model, let drag = destination,
              current(info,drag: drag,view: view,model: model), let point = point(info,view: view,owner: drag.owner),
              let mouse = mouse(info,view: view,point: point) else {
            invalidate(); return []
        }
        if NativeDragOperation.allowed(info.draggingSourceOperationMask) != drag.allowed {
            invalidate(); return entered(info)
        }
        if drag.point != point || drag.mouse != mouse {
            drag.point = point; drag.mouse = mouse; drag.decisionMouse = nil; drag.operation = []; drag.decisionFrame = nil; drag.prepared = false
        }
        if drag.decisionMouse != mouse || drag.decisionFrame != model.generation { negotiate(mouse,drag: drag,model: model) }
        return drag.decisionMouse == mouse && drag.decisionFrame == model.generation ? drag.operation : []
    }
    func prepare(_ info: NSDraggingInfo) -> Bool {
        guard let view, let model = view.model, let drag = destination,
              current(info,drag: drag,view: view,model: model),
              let point = point(info,view: view,owner: drag.owner),
              mouse(info,view: view,point: point)?.sameNegotiation(as: drag.decisionMouse) == true,
              NativeDragOperation.allowed(info.draggingSourceOperationMask) == drag.allowed, !drag.operation.isEmpty,
              drag.decisionFrame == model.generation else { return false }
        drag.prepared = true; return true
    }
    func perform(_ info: NSDraggingInfo) -> Bool {
        guard let view, let model = view.model, let drag = destination, drag.prepared,
              current(info,drag: drag,view: view,model: model),
              let point = point(info,view: view,owner: drag.owner),
              let mouse = mouse(info,view: view,point: point), mouse.sameNegotiation(as: drag.decisionMouse),
              NativeDragOperation.allowed(info.draggingSourceOperationMask) == drag.allowed,
              drag.decisionFrame == model.generation, !drag.operation.isEmpty else { return false }
        // Payload access is confined to this explicit OS drop callback.
        let items: [NativeDragItem]
        do { items = try Self.droppedItems(drag.pasteboard,types: drag.types) }
        catch { invalidate(); return false }
        guard drag.pasteboard.changeCount == drag.changeCount, NativeDragItem.valid(items) else { invalidate(); return false }
        drag.ended = true; drag.prepared = false; destination = nil
        guard let expected = drag.allowed.first(where: { $0.appKit == drag.operation }),
              let reply = model.commitNativeDrop(.drop(drag.id,point,items,expected: expected),owner: drag.owner,mouse: mouse) else {
            // The broker can refuse an unseen Core frame before sending Drop.
            // Release the metadata-only destination without retrying a write.
            Task { _ = await model.nativeDrag(.leave(drag.id),owner: drag.owner,mouse: mouse) }
            return false
        }
        return !reply.active && reply.operation == expected
    }
    func exited(_ info: NSDraggingInfo) {
        guard let drag = destination, drag.sequence == info.draggingSequenceNumber else { return }
        let snapshot = view.flatMap { view in
            point(info,view: view,owner: drag.owner,allowOutside: true).flatMap { mouse(info,view: view,point: $0) }
        }
        invalidate(mouse: snapshot)
    }
    func invalidate(mouse: NativeDragMouse? = nil) {
        guard let drag = destination else { return }
        destination = nil; drag.ended = true; drag.operation = []; drag.prepared = false
        guard let model = drag.model else { return }
        Task { _ = await model.nativeDrag(.leave(drag.id),owner: drag.owner,mouse: mouse) }
    }
    private func negotiate(_ mouse: NativeDragMouse,drag: Destination,model: BrowserModel) {
        guard !drag.ended, let view, destination === drag, view.model === model,
              view.window?.isVisible == true, view.window?.isKeyWindow == true,
              view.window?.attachedSheet == nil, view.bounds.size == drag.viewSize,
              model.nativeDragIsCurrent(drag.owner) else { return }
        let reply = model.negotiateNativeDrag(.enter(drag.id,mouse.point,drag.types,drag.allowed),owner: drag.owner,mouse: mouse)
        guard !drag.ended, destination === drag, model.nativeDragIsCurrent(drag.owner), drag.mouse == mouse else { return }
        drag.decisionMouse = reply == nil ? nil : mouse
        drag.decisionFrame = reply?.frame_generation
        drag.operation = reply?.active == true ? reply!.operation.appKit : []
    }
    private func usable(_ info: NSDraggingInfo,view: CorePageView) -> Bool {
        guard let window = view.window else { return false }
        if let controller = info.draggingSource as? BrowserPageDragging {
            guard let source = controller.source, !source.ended, !source.cancelled,
                  source.session?.draggingSequenceNumber == info.draggingSequenceNumber,
                  source.model?.nativeDragIsCurrent(source.owner) == true else { return false }
        }
        return info.draggingDestinationWindow === window && window.isVisible && window.isKeyWindow && window.attachedSheet == nil
    }
    private func current(_ info: NSDraggingInfo,drag: Destination,view: CorePageView,model: BrowserModel) -> Bool {
        !drag.ended && usable(info,view: view) && drag.sequence == info.draggingSequenceNumber
            && drag.model === model
            && drag.pasteboard.name == info.draggingPasteboard.name && drag.pasteboard.changeCount == drag.changeCount
            && Self.advertisedTypes(drag.pasteboard) == drag.types
            && view.bounds.size == drag.viewSize && model.nativeDragIsCurrent(drag.owner)
    }
    private func point(_ info: NSDraggingInfo,view: CorePageView,owner: NativeDragOwner,allowOutside: Bool = false) -> CGPoint? {
        let position = view.convert(info.draggingLocation,from: nil)
        guard (allowOutside || view.bounds.contains(position)), view.bounds.width > 0, view.bounds.height > 0 else { return nil }
        let point = CGPoint(x: (position.x - view.bounds.minX) * owner.cssSize.width / view.bounds.width,
                            y: (position.y - view.bounds.minY) * owner.cssSize.height / view.bounds.height)
        return point.x.isFinite && point.y.isFinite && abs(point.x) <= 16384 && abs(point.y) <= 16384 ? point : nil
    }
    static func advertisedTypes(_ pasteboard: NSPasteboard) -> [String] {
        let types = pasteboard.types ?? []
        if types.contains(.fileURL) { return ["Files"] }
        if let formats = metadataTypes(pasteboard) { return formats }
        var result: [String] = []
        if types.contains(.string) { result.append("text/plain") }
        if types.contains(.html) { result.append("text/html") }
        // A filesystem URL is metadata for Files, never page-readable URI text.
        if types.contains(.URL) { result.append("text/uri-list") }
        return result
    }
    private static func metadataTypes(_ pasteboard: NSPasteboard) -> [String]? {
        let types = pasteboard.types ?? []
        guard types.contains(formatMetadata), let data = pasteboard.data(forType: formatMetadata), data.count <= maximumMetadataBytes,
              let formats = try? JSONDecoder().decode([String].self,from: data), formats.count <= 64,
              formats.allSatisfy({ NativeDragItem.validFormat($0) && $0 != "Files" && $0 == $0.lowercased() }),
              Set(formats).count == formats.count, formats.allSatisfy({ types.contains(pasteboardType($0)) }) else { return nil }
        return formats
    }
    private static func droppedItems(_ pasteboard: NSPasteboard,types: [String]) throws -> [NativeDragItem] {
        var result: [NativeDragItem] = []
        for format in types where format != "Files" {
            let type = pasteboardType(format)
            guard let value = pasteboard.string(forType: type), value.utf8.count <= SelectedFile.maximumBytes,
                  format != "text/uri-list" || URL(string: value)?.isFileURL == false else {
                throw BrowserFailure.invalid("Unsupported dropped text.")
            }
            result.append(.text(format: format,data: value))
        }
        if types.contains("Files") {
            guard let urls = pasteboard.readObjects(forClasses: [NSURL.self],options: [.urlReadingFileURLsOnly: true]) as? [URL],
                  !urls.isEmpty, Set(urls).count == urls.count else { throw BrowserFailure.invalid("Unsupported dropped files.") }
            result += try SelectedFile.read(urls,multiple: true).map(NativeDragItem.file)
        }
        guard NativeDragItem.valid(result) else { throw BrowserFailure.invalid("Dropped content exceeded its limit.") }
        return result
    }
}

private final class NativeDragReadDeadline: @unchecked Sendable {
    private let connection: BrowserConnection
    private let lock = NSLock()
    private var active = true
    init(_ connection: BrowserConnection) { self.connection = connection }
    func expire() { lock.withLock { if active { connection.interrupt() } } }
    func cancel() { lock.withLock { active = false } }
}

/// A drop acknowledgement must not depend on the GUI's MainActor reader.
/// This short-lived connection owns no service and never retries a mutation.
enum BrowserNativeDragCommit {
    static func drop(_ action: NativeDragAction,runtime: URL,owner: NativeDragOwner,frameGeneration: UInt64,mouse: NativeDragMouse? = nil) throws -> NativeDragState {
        guard case .drop(_,_,let items,let expected) = action, expected != nil, NativeDragItem.valid(items) else {
            throw BrowserFailure.invalid("Invalid native drop commit.")
        }
        let state = try exchange(action,runtime: runtime,owner: owner,mouse: mouse,expectedFrame: frameGeneration)
        guard state.items == nil else { throw BrowserFailure.invalid("A drop reply disclosed source content.") }
        return state
    }
    static func start(_ action: NativeDragAction,runtime: URL,owner: NativeDragOwner,frameGeneration: UInt64,mouse: NativeDragMouse? = nil) throws -> NativeDragState {
        guard case .start = action else { throw BrowserFailure.invalid("Invalid native drag start.") }
        let state = try exchange(action,runtime: runtime,owner: owner,mouse: mouse,expectedFrame: frameGeneration)
        guard !state.active || state.items != nil else { throw BrowserFailure.invalid("The core drag source payload is missing.") }
        return state
    }
    static func sourceMove(_ action: NativeDragAction,runtime: URL,owner: NativeDragOwner,mouse: NativeDragMouse) throws -> NativeDragState {
        guard case .sourceMove = action else { throw BrowserFailure.invalid("Invalid native drag source movement.") }
        let state = try exchange(action,runtime: runtime,owner: owner,mouse: mouse)
        guard state.items == nil else { throw BrowserFailure.invalid("Drag movement disclosed source content.") }
        return state
    }
    static func negotiate(_ action: NativeDragAction,runtime: URL,owner: NativeDragOwner,frameGeneration: UInt64,mouse: NativeDragMouse) throws -> NativeDragState {
        guard case .enter = action else { throw BrowserFailure.invalid("Invalid native drag negotiation.") }
        let state = try exchange(action,runtime: runtime,owner: owner,mouse: mouse,expectedFrame: frameGeneration)
        guard state.items == nil else { throw BrowserFailure.invalid("Drag negotiation disclosed source content.") }
        return state
    }
    private static func exchange(_ action: NativeDragAction,runtime: URL,owner: NativeDragOwner,mouse: NativeDragMouse?,expectedFrame: UInt64? = nil) throws -> NativeDragState {
        guard mouse?.valid != false else { throw BrowserFailure.invalid("Invalid native drag mouse snapshot.") }
        let connection = try BrowserConnection(socketPath: runtime.appendingPathComponent("browser.sock").path)
        defer { connection.interrupt(); connection.close() }
        let deadline = NativeDragReadDeadline(connection)
        let timeout = DispatchWorkItem { deadline.expire() }
        DispatchQueue.global().asyncAfter(deadline: .now() + 5,execute: timeout)
        defer { deadline.cancel(); timeout.cancel() }
        var request = UInt64.random(in: (1 << 48)...(1 << 63))
        func roundTrip(_ command: BrowserCommand,scoped: Bool) throws -> BrowserMessage {
            request += 1
            let wrapped = scoped ? BrowserCommand.browserContext(.command(owner.browserContext,.window(.command(owner.window,command)))) : command
            try connection.output.write(contentsOf: BrowserWire.encode(wrapped,tab: scoped ? owner.tab : nil,request: request))
            while true {
                let reply = try BrowserWire.read { count in
                    var bytes = [UInt8](repeating: 0,count: count)
                    let received = Darwin.read(connection.input.fileDescriptor,&bytes,count)
                    guard received >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
                    return Data(bytes.prefix(received))
                }
                guard reply.requestID == request else { continue }
                guard !scoped || reply.tabID == owner.tab else { throw BrowserFailure.invalid("Native drop reply belongs to another tab.") }
                if case .error(let message) = reply.message { throw BrowserFailure.invalid(message) }
                if case .nativeDrag = command {
                    switch reply.message {
                    case .frame, .viewportState, .displayPreferences: continue
                    default: break
                    }
                }
                return reply.message
            }
        }
        guard case .hello(BrowserWire.version) = try roundTrip(.values("Hello",["protocol_version": .unsigned(UInt64(BrowserWire.version))]),scoped: false),
              case .viewportState(let viewport) = try roundTrip(.unit("GetViewportState"),scoped: true),
              viewport.tabID == owner.tab, viewport.frameSource == owner.context.frame_source,
              viewport.cssWidth == owner.cssSize.width, viewport.cssHeight == owner.cssSize.height,
              case .textInputState(let input) = try roundTrip(.unit("GetTextInputState"),scoped: true),
              input.tab_id == owner.tab, input.frame_source == owner.context.frame_source,
              input.document_generation == owner.context.document_generation,
              input.frame_generation == viewport.frameGeneration,
              expectedFrame == nil || input.frame_generation == expectedFrame else {
            throw BrowserFailure.invalid("The native drop document has changed.")
        }
        try input.validate()
        guard case .nativeDragState(let state) = try roundTrip(.nativeDrag(owner.context,expectedFrame ?? input.frame_generation,action,mouse: mouse),scoped: true),
              state.drag_id == action.id, state.frame_source == owner.context.frame_source,
              state.document_generation == owner.context.document_generation, state.mouse == mouse else {
            throw BrowserFailure.invalid("Invalid native drop acknowledgement.")
        }
        return state
    }
}
