// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

struct NativeLoopbackSettings: Codable, Equatable, Sendable {
    var provider: String
    var base_url: String
    var model: String
    var valid: Bool {
        guard ["ollama", "huggingface", "llamacpp"].contains(provider),
              !model.isEmpty, model.utf8.count <= 128,
              model.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || Array("._-/:@+".utf8).contains($0) }),
              let url = URLComponents(string: base_url), url.scheme == "http",
              ["127.0.0.1", "::1", "[::1]"].contains(url.host), let port = url.port, (1...65535).contains(port),
              url.user == nil, url.password == nil, url.path == "/v1/", url.query == nil, url.fragment == nil else { return false }
        return true
    }
}
struct NativeCandleSettings: Codable, Equatable, Sendable {
    var model_path: String
    var tokenizer_path: String
    var context: UInt64
    var valid: Bool {
        [model_path, tokenizer_path].allSatisfy { !$0.isEmpty && $0.utf8.count <= 4096 && !$0.contains("\0") }
            && (64...131072).contains(context)
    }
}
struct NativeAssistantSettings: Codable, Equatable, Sendable {
    var version: UInt64
    var backend: String
    var loopback: NativeLoopbackSettings?
    var candle: NativeCandleSettings?
    var idle_timeout_secs: UInt64
    var max_resident_mb: UInt64?
    var nice: Int
    static let defaults = Self(version: 1, backend: "none", loopback: nil, candle: nil, idle_timeout_secs: 600, max_resident_mb: nil, nice: 10)
    var valid: Bool {
        version == 1 && ["none", "loopback", "candle", "both"].contains(backend)
            && (30...86400).contains(idle_timeout_secs) && (0...19).contains(nice)
            && max_resident_mb.map { (256...1048576).contains($0) } != false
            && (loopback != nil) == ["loopback", "both"].contains(backend)
            && (candle != nil) == ["candle", "both"].contains(backend)
            && loopback?.valid != false && candle?.valid != false
    }
    // Include null sections in the same schema the Rust settings writer uses.
    enum CodingKeys: String, CodingKey { case version, backend, loopback, candle, idle_timeout_secs, max_resident_mb, nice }
    func encode(to encoder: Encoder) throws {
        var box = encoder.container(keyedBy: CodingKeys.self)
        try box.encode(version, forKey: .version); try box.encode(backend, forKey: .backend)
        try box.encode(loopback, forKey: .loopback); try box.encode(candle, forKey: .candle)
        try box.encode(idle_timeout_secs, forKey: .idle_timeout_secs)
        try box.encode(max_resident_mb, forKey: .max_resident_mb); try box.encode(nice, forKey: .nice)
    }
    var fields: [(String, String)] {
        [("Backend", backend), ("Idle timeout (seconds)", String(idle_timeout_secs)),
         ("Memory ceiling (MiB)", max_resident_mb.map(String.init) ?? "Unlimited"), ("Scheduling niceness", String(nice)),
         ("Model provider", loopback?.provider ?? "—"), ("Loopback base URL", loopback?.base_url ?? "—"),
         ("Model name", loopback?.model ?? "—"), ("GGUF model file", candle?.model_path ?? "—"),
         ("Tokenizer file", candle?.tokenizer_path ?? "—"), ("Candle context", candle.map { String($0.context) } ?? "—")]
    }
    func differences(from before: Self) -> [AssistantSettingDifference] {
        zip(before.fields, fields).compactMap { old, new in
            old.1 == new.1 ? nil : .init(label: new.0, before: old.1, after: new.1)
        }
    }
}
struct AssistantSettingDifference: Identifiable {
    let label: String
    let before: String
    let after: String
    var id: String { label }
}
struct NativeAssistantProposal: Decodable, Equatable, Sendable {
    let id: UInt64
    let digest: String
    let diff: [String]
    let proposed: NativeAssistantSettings
    let seconds_left: UInt64
    var valid: Bool {
        id > 0 && digest.utf8.count == 64 && digest.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
            && proposed.valid && diff.count <= 32 && diff.allSatisfy { $0.utf8.count <= 16384 }
    }
}
struct NativeAssistantState: Decodable, Equatable, Sendable {
    let current: NativeAssistantSettings
    let pending: NativeAssistantProposal?
    var valid: Bool { current.valid && pending?.valid != false }
}
enum AssistantConsentText {
    // Escape controls and bidi format characters rather than letting an
    // agent-controlled path/model impersonate the surrounding decision UI.
    static func display(_ value: String) -> String {
        value.unicodeScalars.map { scalar in
            if CharacterSet.controlCharacters.contains(scalar) || [0x061C, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2066, 0x2067, 0x2068, 0x2069].contains(scalar.value) {
                return String(format: "[U+%04X]", scalar.value)
            }
            return String(scalar)
        }.joined()
    }
}
struct AssistantSettingsDraft {
    var backend: String
    var idle: String
    var ceiling: String
    var nice: String
    var provider: String
    var baseURL: String
    var model: String
    var modelPath: String
    var tokenizerPath: String
    var context: String
    init(_ value: NativeAssistantSettings) {
        backend = value.backend; idle = String(value.idle_timeout_secs)
        ceiling = value.max_resident_mb.map(String.init) ?? ""; nice = String(value.nice)
        provider = value.loopback?.provider ?? "ollama"; baseURL = value.loopback?.base_url ?? "http://127.0.0.1:11434/v1/"
        model = value.loopback?.model ?? ""; modelPath = value.candle?.model_path ?? ""
        tokenizerPath = value.candle?.tokenizer_path ?? ""; context = value.candle.map { String($0.context) } ?? "4096"
    }
    func settings() throws -> NativeAssistantSettings {
        guard let idle = UInt64(idle), let nice = Int(nice), ceiling.isEmpty || UInt64(ceiling) != nil else {
            throw PanelFailure.invalid("Enter whole numbers for the resource limits. Leave the memory ceiling empty for unlimited.")
        }
        var value = NativeAssistantSettings.defaults
        value.backend = backend; value.idle_timeout_secs = idle; value.nice = nice; value.max_resident_mb = UInt64(ceiling)
        if ["loopback", "both"].contains(backend) { value.loopback = .init(provider: provider, base_url: baseURL, model: model) }
        if ["candle", "both"].contains(backend) {
            guard let context = UInt64(context) else { throw PanelFailure.invalid("Enter a whole number for model context.") }
            value.candle = .init(model_path: modelPath, tokenizer_path: tokenizerPath, context: context)
        }
        guard value.valid else {
            throw PanelFailure.invalid("Check the resource limits and model fields. Loopback models require a credential-free http://127.0.0.1:port/v1/ or http://[::1]:port/v1/ address and a safe model name.")
        }
        return value
    }
}
