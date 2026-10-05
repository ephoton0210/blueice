// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Darwin
import Foundation
import SwiftUI

struct PrintProfile: Codable, Equatable, Sendable {
    let width_points: Double
    let height_points: Double
    var backgrounds: Bool = true
    var valid: Bool {
        [width_points,height_points].allSatisfy { $0.isFinite && $0 >= 0.75 && ceil($0 * 8 / 3) <= 4096 }
    }
    @MainActor static func from(_ info: NSPrintInfo) throws -> PrintProfile {
        let scale = info.scalingFactor
        guard scale.isFinite, scale > 0 else { throw BrowserFailure.invalid("Invalid print scale.") }
        let profile = PrintProfile(width_points: (info.paperSize.width - info.leftMargin - info.rightMargin) / scale,
                                   height_points: (info.paperSize.height - info.topMargin - info.bottomMargin) / scale)
        guard profile.valid else { throw BrowserFailure.invalid("The printable area exceeds the supported 192 DPI print surface.") }
        return profile
    }
}

enum PrintAction: Encodable, Sendable {
    case begin(UInt64,UInt64), render(String,PrintProfile), validate(String), end(String)
    private enum Keys: String, CodingKey { case Begin, Render, Validate, End }
    private struct Begin: Encodable { let frame_source: UInt64; let document_generation: UInt64 }
    private struct Render: Encodable { let ticket: String; let profile: PrintProfile }
    private struct End: Encodable { let ticket: String }
    func encode(to encoder: Encoder) throws {
        var value = encoder.container(keyedBy: Keys.self)
        switch self {
        case .begin(let source,let generation): try value.encode(Begin(frame_source: source,document_generation: generation),forKey: .Begin)
        case .render(let ticket,let profile): try value.encode(Render(ticket: ticket,profile: profile),forKey: .Render)
        case .validate(let ticket): try value.encode(End(ticket: ticket),forKey: .Validate)
        case .end(let ticket): try value.encode(End(ticket: ticket),forKey: .End)
        }
    }
}
struct PrintedPage: Decodable, Sendable {
    let shm_path: String
    let width: Int
    let height: Int
    let height_points: Double
}
enum PrintReply: Decodable, Sendable {
    case begun(String,UInt64), rendered(String,UInt64,PrintProfile,[PrintedPage]), validated(String), ended(String)
    private enum Keys: String, CodingKey { case Begun, Rendered, Validated, Ended }
    private struct Begun: Decodable { let ticket: String; let document_generation: UInt64 }
    private struct Rendered: Decodable { let ticket: String; let revision: UInt64; let profile: PrintProfile; let pages: [PrintedPage] }
    private struct Ended: Decodable { let ticket: String }
    static func validTicket(_ text: String) -> Bool {
        text.utf8.count == 32 && text.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
    init(from decoder: Decoder) throws {
        let value = try decoder.container(keyedBy: Keys.self)
        guard value.allKeys.count == 1, let key = value.allKeys.first else { throw BrowserFailure.invalid("Invalid print reply.") }
        switch key {
        case .Begun:
            let reply = try value.decode(Begun.self,forKey: key)
            guard Self.validTicket(reply.ticket) else { throw BrowserFailure.invalid("Invalid print ticket.") }
            self = .begun(reply.ticket,reply.document_generation)
        case .Rendered:
            let reply = try value.decode(Rendered.self,forKey: key)
            guard Self.validTicket(reply.ticket), reply.revision > 0, reply.profile.valid, (1...32).contains(reply.pages.count),
                  reply.pages.allSatisfy({ p in
                      (1...4096).contains(p.width) && (1...4096).contains(p.height) && p.height_points.isFinite
                          && p.height_points > 0 && p.height_points <= reply.profile.height_points + 1e-6
                          && Double(p.width) == ceil(reply.profile.width_points * 8 / 3)
                          && Double(p.height) == ceil(p.height_points * 8 / 3)
                  }), reply.pages.reduce(UInt64(0), { $0 + UInt64($1.width * $1.height) }) <= 64 * 1024 * 1024,
                  Set(reply.pages.map(\.shm_path)).count == reply.pages.count else { throw BrowserFailure.invalid("Invalid print pages.") }
            self = .rendered(reply.ticket,reply.revision,reply.profile,reply.pages)
        case .Validated:
            let reply = try value.decode(Ended.self,forKey: key)
            guard Self.validTicket(reply.ticket) else { throw BrowserFailure.invalid("Invalid validated print ticket.") }
            self = .validated(reply.ticket)
        case .Ended:
            let reply = try value.decode(Ended.self,forKey: key)
            guard Self.validTicket(reply.ticket) else { throw BrowserFailure.invalid("Invalid released print ticket.") }
            self = .ended(reply.ticket)
        }
    }
}

// Fence a deadline callback against closing/reusing the socket descriptor.
private final class PrintReadDeadline: @unchecked Sendable {
    private let connection: BrowserConnection
    private let lock = NSLock()
    private var active = true
    init(_ connection: BrowserConnection) { self.connection = connection }
    func expire() { lock.withLock { if active { connection.interrupt() } } }
    func cancel() { lock.withLock { active = false } }
}

// A separate broker connection permits synchronous AppKit preview callbacks
// without waiting for the GUI's MainActor browser reader. It owns no service.
final class BrowserPrintSession: @unchecked Sendable {
    private let socketPath: String
    private let root: URL
    private let tab: UInt64
    private let context: UInt64
    private let window: UInt64
    // The broker broadcasts replies from every client. Reserve a random high
    // request namespace so the GUI/MCP low counters cannot complete this job.
    private var request = UInt64.random(in: (1 << 48)...(1 << 63))
    private var closed = false
    private(set) var ticket: String?
    private(set) var documentGeneration: UInt64?
    private(set) var revision: UInt64 = 0
    init(runtime: URL,tab: UInt64,context: UInt64,window: UInt64,source: UInt64,document: UInt64) throws {
        root = runtime.appendingPathComponent("frames"); self.tab = tab; self.context = context; self.window = window
        socketPath = runtime.appendingPathComponent("browser.sock").path
        do {
            guard case .printState(.begun(let ticket,let document)) = try exchange(.printAction(.begin(source,document))) else {
                throw BrowserFailure.invalid("Core did not capture the print document.")
            }
            self.ticket = ticket; documentGeneration = document
        } catch { closeConnection(); throw error }
    }
    deinit { closeConnection() }
    private func closeConnection() {
        guard !closed else { return }
        closed = true
    }
    private func exchange(_ command: BrowserCommand) throws -> BrowserMessage {
        guard !closed else { throw BrowserFailure.invalid("Print connection is closed.") }
        // The broker broadcasts to all connections. Close each exchange before
        // a native panel waits, so unrelated AX traffic cannot fill an idle
        // socket and cause the broker to prune the captured print job's reader.
        let connection = try BrowserConnection(socketPath: socketPath)
        defer { connection.interrupt(); connection.close() }
        // Bound the entire exchange, including a partial/flooded reply. Once
        // interrupted the connection is discarded; mutations are never retried.
        let deadline = PrintReadDeadline(connection)
        let timeout = DispatchWorkItem { deadline.expire() }
        DispatchQueue.global().asyncAfter(deadline: .now() + 10,execute: timeout)
        defer { deadline.cancel(); timeout.cancel() }
        func roundTrip(_ command: BrowserCommand, scoped: Bool) throws -> BrowserMessage {
            request += 1
            let wrapped = scoped ? BrowserCommand.browserContext(.command(context,.window(.command(window,command)))) : command
            try connection.output.write(contentsOf: BrowserWire.encode(wrapped,tab: scoped ? tab : nil,request: request))
            while true {
                let reply = try BrowserWire.read { count in
                    var bytes = [UInt8](repeating: 0,count: count)
                    let read = Darwin.read(connection.input.fileDescriptor,&bytes,count)
                    guard read >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
                    return Data(bytes.prefix(read))
                }
                guard reply.requestID == request else { continue }
                guard !scoped || reply.tabID == tab else { throw BrowserFailure.invalid("Print reply belongs to another tab.") }
                if case .error(let message) = reply.message { throw BrowserFailure.invalid(message) }
                return reply.message
            }
        }
        guard case .hello(BrowserWire.version) = try roundTrip(.values("Hello",["protocol_version": .unsigned(UInt64(BrowserWire.version))]),scoped: false) else {
            throw BrowserFailure.invalid("Print connection did not complete the browser handshake.")
        }
        return try roundTrip(command,scoped: true)
    }
    func render(_ profile: PrintProfile) throws -> PrintImages {
        guard let ticket, profile.valid else { throw BrowserFailure.invalid("Print document is closed or the page size is invalid.") }
        guard case .printState(.rendered(let returned,let next,let applied,let pages)) = try exchange(.printAction(.render(ticket,profile))),
              returned == ticket, next > revision, applied == profile else { throw BrowserFailure.invalid("Stale print page reply.") }
        let directory = root.appendingPathComponent("print-" + ticket)
        let images = try pages.enumerated().map { index,page -> PrintImages.Page in
            let expected = directory.appendingPathComponent("frame-\(index + 1)-\(next).rgba")
            guard page.shm_path == expected.path else { throw BrowserFailure.invalid("Print file does not belong to this job.") }
            let pixels = try FramePixels.read(FrameNotice(path: page.shm_path,width: page.width,height: page.height,generation: next),directory: directory)
            return PrintImages.Page(image: try pixels.image(),height: page.height_points)
        }
        revision = next
        return PrintImages(profile: applied,pages: images)
    }
    func validate() throws {
        guard let ticket, case .printState(.validated(let returned)) = try exchange(.printAction(.validate(ticket))), returned == ticket else {
            throw BrowserFailure.invalid("Print document is no longer current.")
        }
    }
    func end() {
        guard let ticket else { return }
        _ = try? exchange(.printAction(.end(ticket)))
        self.ticket = nil
        closeConnection()
    }
}
struct PrintImages: @unchecked Sendable {
    struct Page { let image: CGImage; let height: Double }
    let profile: PrintProfile
    let pages: [Page]
}

// Yield the MainActor while AppKit owns the document-modal panel and release
// the captured job only after its native completion callback.
@MainActor
final class BrowserPrintCompletion: NSObject {
    private var continuation: CheckedContinuation<Bool, Never>?
    func run(_ operation: NSPrintOperation, for window: NSWindow) async -> Bool {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            operation.runModal(for: window, delegate: self,
                didRun: #selector(finished(_:success:contextInfo:)), contextInfo: nil)
        }
    }
    @objc private func finished(_ operation: NSPrintOperation, success: Bool, contextInfo: UnsafeMutableRawPointer?) {
        let pending = continuation
        continuation = nil
        pending?.resume(returning: success)
    }
}

@MainActor
final class BrowserPrintView: NSView {
    private let session: BrowserPrintSession
    private(set) var output: PrintImages
    private(set) var failure: Error?
    weak var operation: NSPrintOperation?
    override var isFlipped: Bool { true }
    init(session: BrowserPrintSession,output: PrintImages) {
        self.session = session; self.output = output
        super.init(frame: NSRect(x: 0,y: 0,width: output.profile.width_points,height: output.profile.height_points * Double(output.pages.count)))
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    override func knowsPageRange(_ range: NSRangePointer) -> Bool {
        do {
            try session.validate()
            if let info = operation?.printInfo ?? NSPrintOperation.current?.printInfo {
                let profile = try PrintProfile.from(info)
                if profile != output.profile { output = try session.render(profile) }
            }
            // Native numeric controls expose intermediate values while typing.
            // A later valid profile must recover its preview and delivery.
            failure = nil
            frame.size = NSSize(width: output.profile.width_points,height: output.profile.height_points * Double(output.pages.count))
            range.pointee = NSRange(location: 1,length: output.pages.count)
        } catch {
            failure = error
            (operation?.printInfo ?? NSPrintOperation.current?.printInfo)?.jobDisposition = .cancel
            range.pointee = NSRange(location: 1,length: 0)
        }
        return true
    }
    override func beginDocument() {
        do {
            try session.validate()
            if let failure { throw failure }
            if let info = operation?.printInfo ?? NSPrintOperation.current?.printInfo { _ = try PrintProfile.from(info) }
        }
        catch { failure = error; (operation?.printInfo ?? NSPrintOperation.current?.printInfo)?.jobDisposition = .cancel }
        super.beginDocument()
    }
    override func rectForPage(_ page: Int) -> NSRect {
        guard failure == nil, (1...output.pages.count).contains(page) else { return .zero }
        return NSRect(x: 0,y: Double(page - 1) * output.profile.height_points,width: output.profile.width_points,height: output.profile.height_points)
    }
    override func draw(_ dirtyRect: NSRect) {
        guard failure == nil, let context = NSGraphicsContext.current?.cgContext else { return }
        for (index,page) in output.pages.enumerated() {
            let rect = NSRect(x: 0,y: Double(index) * output.profile.height_points,width: output.profile.width_points,height: page.height)
            guard dirtyRect.intersects(rect) else { continue }
            context.saveGState()
            context.translateBy(x: rect.minX,y: rect.maxY); context.scaleBy(x: 1,y: -1)
            context.interpolationQuality = .high
            context.draw(page.image,in: NSRect(x: 0,y: 0,width: rect.width,height: rect.height))
            context.restoreGState()
        }
    }
    static func jobTitle(_ page: PageRepresentation?) -> String {
        let url = page?.url.flatMap(URL.init(string:))
        let heading = page?.nodes.first { if case .heading = $0.role { return true }; return false }?.name
        var title = (heading ?? url?.lastPathComponent ?? "BlueIce Page").map { character in
            character.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) || "/:\\".contains(character) ? " " : String(character)
        }.joined().trimmingCharacters(in: .whitespacesAndNewlines)
        while title.utf8.count > 200 { title.removeLast() }
        return title.isEmpty ? "BlueIce Page" : title
    }
    static func info() -> NSPrintInfo {
        let info = NSPrintInfo.shared.copy() as! NSPrintInfo
        info.topMargin = 36; info.bottomMargin = 36; info.leftMargin = 36; info.rightMargin = 36
        info.isHorizontallyCentered = false; info.isVerticallyCentered = false
        info.horizontalPagination = .clip; info.verticalPagination = .clip
        return info
    }
}
struct BrowserPrintingCommands: Commands {
    @ObservedObject var model: BrowserModel
    var body: some Commands {
        CommandGroup(replacing: .printItem) {
            Button("Print…") { Task { await model.printCurrentPage() } }
                .keyboardShortcut("p").disabled(!model.canPrint)
        }
    }
}
