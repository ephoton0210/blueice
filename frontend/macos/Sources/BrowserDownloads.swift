// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

struct DownloadConfiguration: Sendable {
    let directory: URL
    let dataDirectory: URL
    static func configured() -> Self {
        let arguments = ProcessInfo.processInfo.arguments
        func option(_ name: String) -> URL? {
            guard let index = arguments.firstIndex(of: name), arguments.indices.contains(index + 1) else { return nil }
            return URL(fileURLWithPath: arguments[index + 1])
        }
        return Self(directory: option("--downloads-directory") ?? FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)[0].appendingPathComponent("BlueIce"),
                    dataDirectory: option("--downloads-data-directory") ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("BlueIce/Downloads"))
    }
    static func checkedFile(_ path: String, root: URL) throws -> URL {
        let file = URL(fileURLWithPath: path).standardizedFileURL
        let root = root.resolvingSymlinksInPath().standardizedFileURL
        let values = try file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true,
              file.resolvingSymlinksInPath().path.hasPrefix(root.path + "/"), !file.lastPathComponent.hasSuffix(".blueice-part") else {
            throw BrowserFailure.invalid("The completed file is missing or outside the downloads folder.")
        }
        return file
    }
}

enum DownloadState: String, Sendable {
    case queued, awaitingClearance = "awaiting_clearance", active, paused, completed, failed, cancelled, blocked, unknown
    var terminal: Bool { [.completed, .failed, .cancelled, .blocked].contains(self) }
    var title: String {
        switch self {
        case .queued: return "Queued"
        case .awaitingClearance: return "Waiting for review"
        case .active: return "Downloading"
        case .paused: return "Paused"
        case .completed: return "Completed"
        case .failed: return "Failed"
        case .cancelled: return "Cancelled"
        case .blocked: return "Blocked"
        case .unknown: return "Unsupported state"
        }
    }
}
struct DownloadInfo: Decodable, Sendable, Identifiable {
    struct Block: Decodable, Sendable { let reason: String; let category: String }
    struct Event: Decodable, Sendable { let at_ms: UInt64; let message: String }
    struct Segment: Decodable, Sendable { let start: UInt64; let end: UInt64; let completed: UInt64 }
    let id: UInt64
    let url: String
    let destPath: String
    let state: DownloadState
    let totalBytes: UInt64?
    let completedBytes: UInt64
    let speedBps: UInt64
    let etaSecs: UInt64?
    let connections: UInt32
    let resumeSafe: Bool
    let generation: UInt64
    let blocked: Block?
    let lastError: String?
    let events: [Event]
    let segments: [Segment]
    enum Keys: String, CodingKey {
        case id, url, state, connections, generation, blocked, events, segments
        case destPath = "dest_path", totalBytes = "total_bytes", completedBytes = "completed_bytes", speedBps = "speed_bps", etaSecs = "eta_secs", resumeSafe = "resume_safe", lastError = "last_error"
    }
    init(from decoder: Decoder) throws {
        let value = try decoder.container(keyedBy: Keys.self)
        id = try value.decode(UInt64.self, forKey: .id); url = try value.decode(String.self, forKey: .url)
        destPath = try value.decode(String.self, forKey: .destPath)
        state = DownloadState(rawValue: try value.decode(String.self, forKey: .state)) ?? .unknown
        totalBytes = try value.decodeIfPresent(UInt64.self, forKey: .totalBytes)
        completedBytes = try value.decode(UInt64.self, forKey: .completedBytes)
        speedBps = try value.decodeIfPresent(UInt64.self, forKey: .speedBps) ?? 0
        etaSecs = try value.decodeIfPresent(UInt64.self, forKey: .etaSecs)
        connections = try value.decodeIfPresent(UInt32.self, forKey: .connections) ?? 0
        resumeSafe = try value.decodeIfPresent(Bool.self, forKey: .resumeSafe) ?? false
        generation = try value.decode(UInt64.self, forKey: .generation)
        blocked = try value.decodeIfPresent(Block.self, forKey: .blocked)
        lastError = try value.decodeIfPresent(String.self, forKey: .lastError)
        events = try value.decodeIfPresent([Event].self, forKey: .events) ?? []
        segments = try value.decodeIfPresent([Segment].self, forKey: .segments) ?? []
        guard id > 0, generation > 0, url.utf8.count <= 8192, destPath.utf8.count <= 4096,
              totalBytes.map({ completedBytes <= $0 }) != false, events.count <= 32, segments.count <= 1024,
              segments.allSatisfy({ $0.end >= $0.start && $0.completed <= $0.end - $0.start }),
              events.allSatisfy({ $0.message.utf8.count <= 8192 }), lastError?.utf8.count ?? 0 <= 8192,
              blocked?.reason.utf8.count ?? 0 <= 8192 else { throw BrowserFailure.invalid("Invalid download metadata") }
    }
    var name: String { destPath.isEmpty ? (URL(string: url)?.lastPathComponent ?? "Download") : URL(fileURLWithPath: destPath).lastPathComponent }
    var fraction: Double? { if let totalBytes, totalBytes > 0 { return min(1, Double(completedBytes) / Double(totalBytes)) }; return state == .completed ? 1 : nil }
    static func bytes(_ count: UInt64) -> String {
        if count < 1024 { return "\(count) B" }
        let units = ["B", "KiB", "MiB", "GiB", "TiB"]
        var value = Double(count), index = 0
        while value >= 1024 && index < 4 { value /= 1024; index += 1 }
        if (value * 10).rounded() >= 10240 && index < 4 { value /= 1024; index += 1 }
        return String(format: "%.1f %@", value, units[index])
    }
    var progressText: String { totalBytes.map { "\(Self.bytes(completedBytes)) of \(Self.bytes($0))" } ?? "\(Self.bytes(completedBytes)) — total unknown" }
}

struct DownloadEnvelope: Decodable, Sendable {
    enum Message: Sendable { case hello(UInt32), list([DownloadInfo]), transfer(DownloadInfo), updated(DownloadInfo), removed(UInt64), ok, error(String), unknown }
    let requestID: UInt64?
    let message: Message
    private enum Keys: String, CodingKey { case requestID = "request_id", message }
    private struct MessageKey: CodingKey {
        let stringValue: String
        let intValue: Int? = nil
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { return nil }
    }
    private struct Hello: Decodable { let protocol_version: UInt32 }
    private struct Identity: Decodable { let id: UInt64 }
    private struct Refusal: Decodable { let code: String; let message: String }
    init(from decoder: Decoder) throws {
        let envelope = try decoder.container(keyedBy: Keys.self)
        requestID = try envelope.decodeIfPresent(UInt64.self, forKey: .requestID)
        let payload = try envelope.superDecoder(forKey: .message)
        if let text = try? payload.singleValueContainer().decode(String.self) { message = text == "Ok" ? .ok : .unknown; return }
        let value = try payload.container(keyedBy: MessageKey.self)
        guard value.allKeys.count == 1, let key = value.allKeys.first else { throw BrowserFailure.invalid("Invalid downloads reply") }
        switch key.stringValue {
        case "Hello": message = .hello(try value.decode(Hello.self, forKey: key).protocol_version)
        case "Transfers":
            let list = try value.decode([DownloadInfo].self, forKey: key)
            guard list.count <= 1024, Set(list.map(\.id)).count == list.count else { throw BrowserFailure.invalid("Invalid downloads list") }
            message = .list(list)
        case "Started", "Transfer": message = .transfer(try value.decode(DownloadInfo.self, forKey: key))
        case "Updated": message = .updated(try value.decode(DownloadInfo.self, forKey: key))
        case "Removed": message = .removed(try value.decode(Identity.self, forKey: key).id)
        case "Error": let error = try value.decode(Refusal.self, forKey: key); message = .error("\(error.code): \(error.message)")
        default: message = .unknown
        }
    }
    static func read(_ read: (Int) throws -> Data) throws -> Self {
        func exact(_ count: Int) throws -> Data {
            var data = Data()
            while data.count < count {
                let part = try read(count - data.count)
                guard !part.isEmpty, part.count <= count - data.count else { throw BrowserFailure.invalid("Downloads connection ended during a message") }
                data.append(part)
            }
            return data
        }
        let count = try exact(4).enumerated().reduce(UInt32(0)) { $0 | UInt32($1.element) << ($1.offset * 8) }
        guard count > 0, count <= 8 * 1024 * 1024 else { throw BrowserFailure.invalid("Invalid downloads message length") }
        return try JSONDecoder().decode(Self.self, from: exact(Int(count)))
    }
}

@MainActor
final class BrowserDownloadsModel: ObservableObject {
    let configuration: DownloadConfiguration
    private let browser: BrowserSession
    private var session: BrowserDownloadsSession?
    private var epoch = UUID()
    @Published private(set) var transfers: [DownloadInfo] = []
    @Published private(set) var available = false
    @Published private(set) var connecting = false
    @Published private(set) var busy: Set<UInt64> = []
    @Published private(set) var starting = false
    @Published private(set) var notice: String?
    private var requests: Set<UInt64> = []
    private var replies: [UInt64: DownloadEnvelope.Message] = [:]
    private var removed: Set<UInt64> = []
    private var stopping = false
    var processID: Int32? { session?.processID }
    init(browser: BrowserSession, configuration: DownloadConfiguration) { self.browser = browser; self.configuration = configuration }
    func connect() async {
        guard !available, !connecting, !stopping else { return }
        connecting = true; defer { connecting = false }
        if let session { await session.stop() }
        guard !stopping else { return }
        requests = []; replies = [:]; removed = []; epoch = UUID()
        let current = epoch
        guard let executable = browser.downloadsExecutable else { notice = "Downloads require the supervised browser services."; return }
        let next = BrowserDownloadsSession(executable: executable, runtime: browser.runtimeDirectory, configuration: configuration)
        session = next
        next.received = { [weak self] result in
            DispatchQueue.main.async {
                guard let self, self.epoch == current else { return }
                do { self.receive(try result.get()) }
                catch { self.available = false; self.notice = error.localizedDescription }
            }
        }
        do {
            try await next.start()
            guard !stopping else { await next.stop(); return }
            transfers = []
            available = true; notice = nil
            _ = await command(.unit("Subscribe"))
            if case .list = await command(.values("List", ["state": .null])) {} else { available = false }
        } catch { available = false; notice = error.localizedDescription; await next.stop() }
    }
    func receive(_ envelope: DownloadEnvelope) {
        let owned = envelope.requestID.map { requests.remove($0) != nil } ?? false
        if let id = envelope.requestID, owned { replies[id] = envelope.message }
        switch envelope.message {
        case .list(let list):
            guard owned else { return }
            let existing = Dictionary(uniqueKeysWithValues: transfers.map { ($0.id,$0) })
            transfers = list.filter { !removed.contains($0.id) }.map { incoming in
                if let old = existing[incoming.id], old.generation > incoming.generation { return old }
                return incoming
            }.sorted { $0.id > $1.id }
        case .transfer(let info): if owned { update(info) }
        case .updated(let info): if envelope.requestID == nil { update(info) }
        case .removed(let id): if envelope.requestID == nil { removed.insert(id); transfers.removeAll { $0.id == id } }
        default: break
        }
    }
    private func update(_ info: DownloadInfo) {
        guard !removed.contains(info.id), transfers.first(where: { $0.id == info.id }).map({ info.generation > $0.generation }) != false else { return }
        transfers.removeAll { $0.id == info.id }; transfers.append(info); transfers.sort { $0.id > $1.id }
    }
    private func command(_ action: BrowserCommand) async -> DownloadEnvelope.Message? {
        guard available, let session else { return nil }
        let current = epoch
        do {
            let request = try await session.send(action) { [weak self] id in
                DispatchQueue.main.async { if let self, self.epoch == current { self.requests.insert(id) } }
            }
            let deadline = Date().addingTimeInterval(10)
            while epoch == current, Date() < deadline, !Task.isCancelled {
                if let reply = replies.removeValue(forKey: request) {
                    if case .error(let error) = reply { notice = error; return nil }
                    return reply
                }
                if !available { break }
                try? await Task.sleep(for: .milliseconds(10))
            }
            guard epoch == current else { return nil }
            requests.remove(request); notice = "The download action did not complete. Refresh before trying again."
        } catch { if epoch == current { available = false; notice = error.localizedDescription } }
        return nil
    }
    func start(_ url: String, name: String?) async -> UInt64? {
        guard !starting, !stopping else { return nil }; starting = true; defer { starting = false }
        await connect()
        guard available else { return nil }; notice = nil
        let action = BrowserCommand.values("Start", ["url": .string(url), "dest": name.map(JSONValue.string) ?? .null, "overwrite": .boolean(false)])
        if case .transfer(let info) = await command(action) { return info.id }
        return nil
    }
    func refresh() async {
        guard available, !stopping else { return }
        if case .list = await command(.values("List", ["state": .null])) { notice = nil }
    }
    func perform(_ action: String, id: UInt64) async -> Bool {
        guard ["Pause", "Resume", "Cancel", "Remove"].contains(action), available, !busy.contains(id), transfers.contains(where: { $0.id == id }) else { return false }
        busy.insert(id); defer { busy.remove(id) }; notice = nil
        let result = await command(.values(action, ["id": .unsigned(id)]))
        if case .transfer = result { return true }
        if case .ok = result, action == "Remove" { removed.insert(id); transfers.removeAll { $0.id == id }; return true }
        return false
    }
    func open(_ info: DownloadInfo, reveal: Bool) {
        guard info.state == .completed, transfers.contains(where: { $0.id == info.id && $0.generation == info.generation }) else { return }
        do {
            let url = try DownloadConfiguration.checkedFile(info.destPath,root: configuration.directory)
            if reveal { NSWorkspace.shared.activateFileViewerSelecting([url]) }
            else if !NSWorkspace.shared.open(url) { throw BrowserFailure.invalid("macOS could not open this file.") }
        } catch { notice = error.localizedDescription }
    }
    func stop() async {
        guard !stopping else { return }; stopping = true
        if available { _ = await command(.unit("Shutdown")) }
        available = false; await session?.stop(); session = nil
    }
}
