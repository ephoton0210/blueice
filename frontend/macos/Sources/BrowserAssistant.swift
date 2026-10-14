// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation
import Combine

struct AssistantDocument: Codable, Equatable, Sendable {
    let tab_id: UInt64
    let frame_source: UInt64
    let document_generation: UInt64
    var valid: Bool { tab_id > 0 && document_generation > 0 }
}
enum AssistantPageAction: Encodable, Sendable {
    case summarize, organize(String), showTranslation(Bool)
    private enum Keys: String, CodingKey { case organize = "Organize", show = "ShowTranslation" }
    func encode(to encoder: Encoder) throws {
        if case .summarize = self { var box = encoder.singleValueContainer(); try box.encode("Summarize"); return }
        var box = encoder.container(keyedBy: Keys.self)
        switch self {
        case .summarize: break
        case .organize(let text): try box.encode(["instruction": text], forKey: .organize)
        case .showTranslation(let shown): try box.encode(["shown": shown], forKey: .show)
        }
    }
}
enum AssistantResultKind: String, Decodable, Sendable { case summary = "Summary", organized = "Organized" }
struct AssistantPageResult: Decodable, Sendable {
    let context: AssistantDocument
    let kind: AssistantResultKind
    let text: String
    var valid: Bool { context.valid && !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && text.utf8.count <= (kind == .summary ? 16384 : 32768) }
}
struct TranslationState: Decodable, Equatable, Sendable {
    let language: String?
    let available: Bool
    let shown: Bool
    var valid: Bool { language.map(Self.validTag) != false && (!shown || available) }
    static func validTag(_ text: String) -> Bool {
        !text.isEmpty && text.utf8.count <= 35 && text.split(separator: "-", omittingEmptySubsequences: false).allSatisfy {
            !$0.isEmpty && $0.utf8.allSatisfy { (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) }
        }
    }
}
enum AssistantTranslationAction: Equatable { case inspect, language(String?), show(Bool) }
enum AssistantTask {
    case summary, organized(String), translation(AssistantTranslationAction)
    var resultKind: AssistantResultKind? {
        switch self { case .summary: return .summary; case .organized: return .organized; case .translation: return nil }
    }
}
struct AssistantPresentedResult {
    let kind: AssistantResultKind
    let text: String
    let url: String
    let translated: Bool
}
@MainActor
final class AssistantOperation {
    let id = UUID()
    let document: AssistantDocument
    let revision: UInt64
    let task: AssistantTask
    let url: String
    let translated: Bool
    var requestID: UInt64?
    weak var owner: BrowserAssistant?
    init(_ page: AssistantPage, task: AssistantTask) {
        document = page.document; revision = page.revision; self.task = task
        url = page.url; translated = page.translation?.shown == true
    }
    var command: BrowserCommand {
        switch task {
        case .summary: return .assistantPage(document, .summarize)
        case .organized(let text): return .assistantPage(document, .organize(text))
        case .translation(.inspect): return .unit("GetTranslationState")
        case .translation(.language(let language)): return .values("SetTranslationLanguage", ["target_language": language.map(JSONValue.string) ?? .null])
        case .translation(.show(let shown)): return .assistantPage(document, .showTranslation(shown))
        }
    }
}
struct AssistantPage {
    var document: AssistantDocument
    var url: String
    var revision: UInt64 = 0
    var instruction = "Group the main points."
    var result: AssistantPresentedResult?
    var translation: TranslationState?
    var pending: AssistantOperation?
    var translating: AssistantOperation?
    var notice: String?
}
@MainActor
final class BrowserAssistant: ObservableObject {
    @Published private(set) var pages: [UInt64: AssistantPage] = [:]
    @Published var languageDraft = "zh-TW"
    private var operations: [UUID: AssistantOperation] = [:]
    var languageConfirmed: ((String?) -> Void)?
    func page(_ tab: UInt64) -> AssistantPage? { pages[tab] }
    func observe(_ document: AssistantDocument, url: String) {
        guard document.valid else { return }
        if let previous = pages[document.tab_id], previous.document != document { invalidate(document.tab_id, clear: true) }
        if pages[document.tab_id] == nil { pages[document.tab_id] = .init(document: document, url: url) }
        else { pages[document.tab_id]?.document = document; pages[document.tab_id]?.url = url }
    }
    func setInstruction(_ text: String, tab: UInt64) { pages[tab]?.instruction = text }
    func begin(_ tab: UInt64, task: AssistantTask) -> AssistantOperation? {
        guard let page = pages[tab], page.pending == nil, operations.count < 16 else { return nil }
        if case .organized(let text) = task, text.isEmpty || text.utf8.count > 512 { pages[tab]?.notice = "Enter an instruction of 1–512 UTF-8 bytes."; return nil }
        let operation = make(page, task: task); pages[tab]?.pending = operation; pages[tab]?.notice = nil
        return operation
    }
    func translate(_ tab: UInt64, action: AssistantTranslationAction) -> AssistantOperation? {
        guard let page = pages[tab], page.translating == nil, operations.count < 16 else { return nil }
        if case .language(let tag) = action, tag.map(TranslationState.validTag) == false { pages[tab]?.notice = "Enter a language tag such as en or zh-TW."; return nil }
        let operation = make(page, task: .translation(action)); pages[tab]?.translating = operation; pages[tab]?.notice = nil
        return operation
    }
    private func make(_ page: AssistantPage, task: AssistantTask) -> AssistantOperation {
        let operation = AssistantOperation(page, task: task); operation.owner = self; operations[operation.id] = operation
        Task { [weak operation] in
            try? await Task.sleep(for: .seconds(65))
            if let operation { operation.owner?.expire(operation) }
        }
        return operation
    }
    func receive(_ envelope: IncomingEnvelope) -> Bool {
        guard let request = envelope.requestID, let operation = operations.values.first(where: { $0.requestID == request }) else { return false }
        let tab = operation.document.tab_id
        guard envelope.tabID == tab else { return true }
        switch envelope.message {
        case .assistantResult, .assistantUnavailable, .translation, .translationUnavailable, .error: break
        default: return false
        }
        operations.removeValue(forKey: operation.id)
        guard var page = pages[tab], page.revision == operation.revision, page.document == operation.document,
              page.pending === operation || page.translating === operation else { return true }
        let isTask = page.pending === operation
        if isTask { page.pending = nil } else { page.translating = nil }
        switch envelope.message {
        case .assistantResult(let result):
            if isTask, result.valid, result.context == operation.document, result.kind == operation.task.resultKind {
                page.result = .init(kind: result.kind, text: result.text, url: operation.url, translated: operation.translated)
                page.notice = nil
            } else { page.notice = "The assistant result did not match the requested document." }
        case .translation(let state):
            var expected = !isTask && state.valid
            if case .translation(.language(let tag)) = operation.task { expected = expected && tag == state.language }
            if case .translation(.show(let shown)) = operation.task { expected = expected && (!state.available || state.shown == shown) }
            if expected {
                if let previous = page.translation, previous.shown != state.shown { page.revision &+= 1; page.pending = nil; page.result = nil }
                page.translation = state; page.notice = nil
                if case .translation(.language) = operation.task { languageConfirmed?(state.language) }
            } else { page.notice = "The translation change was not confirmed. Refresh its state." }
        case .error(let text): page.notice = text
        default: page.notice = "The assistant reply is unavailable or exceeds its bounds."
        }
        pages[tab] = page; return true
    }
    func invalidate(_ tab: UInt64, clear: Bool) {
        pages[tab]?.revision &+= 1; pages[tab]?.pending = nil; pages[tab]?.translating = nil
        if clear { pages[tab]?.result = nil; pages[tab]?.translation = nil; pages[tab]?.notice = nil }
    }
    func stopWaiting(_ tab: UInt64) { pages[tab]?.pending = nil; pages[tab]?.notice = "Stopped waiting. The local task may finish in the background." }
    func expire(_ operation: AssistantOperation) {
        guard operations.removeValue(forKey: operation.id) != nil else { return }
        let tab = operation.document.tab_id
        if pages[tab]?.pending === operation { pages[tab]?.pending = nil; pages[tab]?.notice = "The local assistant did not reply in time." }
        if pages[tab]?.translating === operation { pages[tab]?.translating = nil; pages[tab]?.notice = "Translation state could not be inspected." }
    }
    func retainTabs(_ tabs: Set<UInt64>) { pages = pages.filter { tabs.contains($0.key) } }
    func transfer(_ tab: UInt64, to destination: BrowserAssistant) {
        if let page = pages.removeValue(forKey: tab) { destination.pages[tab] = page }
        for operation in Array(operations.values) where operation.document.tab_id == tab {
            operations.removeValue(forKey: operation.id); operation.owner = destination; destination.operations[operation.id] = operation
        }
    }
    func clear() { pages.removeAll(); operations.removeAll() }
}
