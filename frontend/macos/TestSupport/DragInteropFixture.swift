// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

@MainActor
final class NativePreviewSource: NSView, NSDraggingSource {
    private var started = false
    override var isFlipped: Bool { true }
    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true); setAccessibilityRole(.button)
        setAccessibilityIdentifier("native-preview-source")
        setAccessibilityLabel("Native reference preview")
    }
    required init?(coder: NSCoder) { nil }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.controlBackgroundColor.setFill(); bounds.fill()
        ("Native reference preview" as NSString).draw(at: NSPoint(x: 4,y: 4),withAttributes: [.font:NSFont.systemFont(ofSize: 12)])
    }
    override func mouseDown(with event: NSEvent) { started = false }
    override func mouseDragged(with event: NSEvent) {
        guard !started else { return }
        started = true
        let point = convert(event.locationInWindow,from: nil)
        let writer = NSPasteboardItem(); writer.setString("Native preview payload",forType: .string)
        let item = NSDraggingItem(pasteboardWriter: writer)
        let image = NSImage(size: NSSize(width: 100,height: 48),flipped: true) { rect in
            NSColor.green.setFill(); rect.fill(); return true
        }
        item.setDraggingFrame(NSRect(x: point.x-12,y: point.y-8,width: 100,height: 48),contents: image)
        beginDraggingSession(with: [item],event: event,source: self)
    }
    func draggingSession(_ session: NSDraggingSession,sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation { .copy }
    func draggingSession(_ session: NSDraggingSession,endedAt point: NSPoint,operation: NSDragOperation) { started = false }
}

@MainActor
final class ExternalTextDestination: NSTextView {
    var reportPreview: ((NSSize,NSPoint) -> Void)?
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        capturePreview(sender)
        return super.draggingEntered(sender)
    }
    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool {
        capturePreview(sender)
        return super.prepareForDragOperation(sender)
    }
    private func capturePreview(_ sender: NSDraggingInfo) {
        sender.enumerateDraggingItems(options: [],for: self,classes: [NSString.self,NSPasteboardItem.self],searchOptions: [:]) { item,_,_ in
            let pointer = self.convert(sender.draggingLocation,from: nil)
            let frame = item.draggingFrame
            let hotspot = NSPoint(x: pointer.x-frame.minX,y: self.isFlipped ? pointer.y-frame.minY : frame.maxY-pointer.y)
            self.reportPreview?(frame.size,hotspot)
        }
    }
}

@MainActor
final class FilePromiseDestination: NSView {
    var destination: URL?
    private let status = NSTextField(labelWithString: "Waiting for file promise")
    private let queue = OperationQueue()
    private var receivers: [NSFilePromiseReceiver] = []

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        registerForDraggedTypes(NSFilePromiseReceiver.readableDraggedTypes.map { NSPasteboard.PasteboardType($0) })
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityIdentifier("external-file-destination")
        setAccessibilityLabel("Promised file destination")
        wantsLayer = true; layer?.backgroundColor = NSColor.controlBackgroundColor.cgColor
        status.frame = bounds.insetBy(dx: 8,dy: 8)
        status.autoresizingMask = [.width,.height]
        status.setAccessibilityIdentifier("promise-result")
        addSubview(status)
        queue.maxConcurrentOperationCount = 1
    }
    required init?(coder: NSCoder) { nil }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        destination != nil && sender.draggingSourceOperationMask.contains(.copy) ? .copy : []
    }
    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool { destination != nil }
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        guard let destination,
              let promises = sender.draggingPasteboard.readObjects(forClasses: [NSFilePromiseReceiver.self],options: nil) as? [NSFilePromiseReceiver],
              !promises.isEmpty else { return false }
        receivers = promises
        for receiver in promises {
            receiver.receivePromisedFiles(atDestination: destination,options: [:],operationQueue: queue) { [weak self] url,error in
                let message: String
                if let error { message = "Promise failed: " + error.localizedDescription }
                else {
                    do {
                        let bytes = try Data(contentsOf: url)
                        message = "Received \(url.lastPathComponent) (\(bytes.count) bytes)"
                    } catch { message = "Promise read failed: " + error.localizedDescription }
                }
                DispatchQueue.main.async { self?.status.stringValue = message }
            }
        }
        return true
    }
}

/// An independent AppKit process for actual OS drag interoperability tests.
/// It does not import BlueIce, emulate Core replies or open user documents.
@MainActor
final class DragInteropFixtureDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(contentRect: NSRect(x: 820,y: 140,width: 600,height: 320),
                              styleMask: [.titled,.closable,.resizable],backing: .buffered,defer: false)
        window.title = "BlueIce external drag fixture"
        window.identifier = NSUserInterfaceItemIdentifier("external-drag-window")
        window.isReleasedWhenClosed = false
        let content = NSView(frame: NSRect(x: 0,y: 0,width: 600,height: 320))
        window.contentView = content
        let source = NSTextView(frame: NSRect(x: 20,y: 220,width: 560,height: 80))
        source.isRichText = false; source.isEditable = false; source.isSelectable = true
        source.font = .systemFont(ofSize: 18)
        source.string = "External 中文 😀"
        source.setAccessibilityIdentifier("external-source")
        source.setAccessibilityLabel("External drag source")
        let destination = ExternalTextDestination(frame: NSRect(x: 20,y: 20,width: 270,height: 160))
        destination.isRichText = false; destination.allowsUndo = true
        destination.font = .systemFont(ofSize: 18)
        destination.setAccessibilityIdentifier("external-destination")
        destination.setAccessibilityLabel("External drag destination")
        destination.autoresizingMask = [.width,.height]
        source.autoresizingMask = [.width,.minYMargin]
        content.addSubview(source); content.addSubview(destination)
        let preview = NSTextField(labelWithString: "Preview not received")
        preview.frame = NSRect(x: 20,y: 190,width: 270,height: 24)
        preview.setAccessibilityIdentifier("preview-result")
        content.addSubview(preview)
        content.addSubview(NativePreviewSource(frame: NSRect(x: 310,y: 190,width: 270,height: 24)))
        destination.reportPreview = { size,hotspot in
            preview.stringValue = "Preview \(Int(size.width))x\(Int(size.height)) hotspot \(Int(hotspot.x.rounded())),\(Int(hotspot.y.rounded()))"
        }
        let promises = FilePromiseDestination(frame: NSRect(x: 310,y: 20,width: 270,height: 160))
        let arguments = ProcessInfo.processInfo.arguments
        if let index = arguments.firstIndex(of: "--promise-directory"), index + 1 < arguments.count {
            promises.destination = URL(fileURLWithPath: arguments[index+1],isDirectory: true)
        }
        content.addSubview(promises)
        self.window = window
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

@main
struct DragInteropFixture {
    @MainActor
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let menu = NSMenu()
        let applicationItem = NSMenuItem()
        let applicationMenu = NSMenu()
        applicationMenu.addItem(withTitle: "Quit",action: #selector(NSApplication.terminate(_:)),keyEquivalent: "q")
        applicationItem.submenu = applicationMenu; menu.addItem(applicationItem)
        let editItem = NSMenuItem(title: "Edit",action: nil,keyEquivalent: "")
        let editMenu = NSMenu(title: "Edit")
        for (title,action,key) in [("Copy",#selector(NSText.copy(_:)),"c"),
                                  ("Paste",#selector(NSText.paste(_:)),"v"),
                                  ("Select All",#selector(NSText.selectAll(_:)),"a")] {
            editMenu.addItem(withTitle: title,action: action,keyEquivalent: key)
        }
        editItem.submenu = editMenu; menu.addItem(editItem)
        app.mainMenu = menu
        let delegate = DragInteropFixtureDelegate()
        app.delegate = delegate
        app.run()
    }
}
