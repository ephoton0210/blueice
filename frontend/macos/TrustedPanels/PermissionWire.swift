// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

enum PanelFailure: Error, LocalizedError, Sendable {
    case invalid(String)
    var errorDescription: String? { if case .invalid(let reason) = self { return reason }; return nil }
}
struct OptionalPermission: Decodable, Equatable, Identifiable, Sendable {
    let capability: String
    let granted: Bool
    let origins: [String]
    var id: String { capability }
}
struct EphemeralPermission: Decodable, Equatable, Identifiable, Sendable {
    let capability: String
    let origins: [String]
    var id: String { capability }
}
struct InstalledPermissions: Decodable, Equatable, Sendable {
    let extension_id: String
    let name: String
    let version: String
    let optional: [OptionalPermission]
    let runtime_ephemeral: [EphemeralPermission]
    var valid: Bool {
        !extension_id.isEmpty && extension_id.utf8.count <= 4096
            && !name.isEmpty && name.utf8.count <= 4096 && version.utf8.count <= 4096
            && optional.count <= 256 && runtime_ephemeral.count <= 256
            && Set(optional.map(\.capability)).count == optional.count
            && Set(runtime_ephemeral.map(\.capability)).count == runtime_ephemeral.count
            && optional.allSatisfy { Self.validCapability($0.capability, $0.origins) }
            && runtime_ephemeral.allSatisfy { $0.capability == "dom:read" && Self.validCapability($0.capability, $0.origins) }
    }
    private static func validCapability(_ capability: String, _ origins: [String]) -> Bool {
        !capability.isEmpty && capability.utf8.count <= 256 && origins.count <= 256
            && origins.allSatisfy { !$0.isEmpty && $0.utf8.count <= 4096 }
    }
}
struct PermissionSnapshot: Decodable, Equatable, Sendable {
    let core_generation: UInt64
    let installed: InstalledPermissions?
}
struct OneShotReview: Decodable, Equatable, Sendable {
    let core_generation: UInt64
    let installed: InstalledPermissions
    let capability: String
    let tab_id: UInt64
    let document_epoch: UInt64
    let url: String
}
struct OneShotArmed: Decodable, Sendable {
    let core_generation: UInt64
    let installed: InstalledPermissions
    let capability: String
    let tab_id: UInt64
    let document_epoch: UInt64
}
enum PermissionReply: Decodable, Sendable {
    case state(PermissionSnapshot), review(OneShotReview), armed(OneShotArmed), rejected(String)
    private enum Keys: String, CodingKey { case state, ephemeral_review, ephemeral_armed, rejected }
    private struct Rejection: Decodable { let reason: String }
    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: Keys.self)
        guard box.allKeys.count == 1 else { throw PanelFailure.invalid("Invalid permission reply.") }
        switch box.allKeys[0] {
        case .state:
            let value = try box.decode(PermissionSnapshot.self, forKey: .state)
            guard value.installed?.valid != false else { throw PanelFailure.invalid("Invalid installed permission state.") }
            self = .state(value)
        case .ephemeral_review:
            let value = try box.decode(OneShotReview.self, forKey: .ephemeral_review)
            guard value.installed.valid, value.tab_id > 0, value.capability == "dom:read",
                  value.url.utf8.count <= 16384, let url = URL(string: value.url),
                  ["http", "https"].contains(url.scheme), url.host != nil else {
                throw PanelFailure.invalid("Invalid one-shot document review.")
            }
            self = .review(value)
        case .ephemeral_armed:
            let value = try box.decode(OneShotArmed.self, forKey: .ephemeral_armed)
            guard value.installed.valid, value.tab_id > 0, value.capability == "dom:read" else {
                throw PanelFailure.invalid("Invalid one-shot acknowledgement.")
            }
            self = .armed(value)
        case .rejected: self = .rejected(try box.decode(Rejection.self, forKey: .rejected).reason)
        }
    }
}
enum PermissionRequest: Encodable, Sendable {
    case inspect
    case change(UInt64, String, String, Bool)
    case review(UInt64, String, UInt64)
    case arm(OneShotReview)
    private enum Keys: String, CodingKey { case change, inspect_ephemeral, arm_ephemeral }
    private struct Change: Encodable {
        let expected_core_generation: UInt64; let expected_extension_id: String
        let capability: String; let action: String
    }
    private struct Review: Encodable {
        let expected_core_generation: UInt64; let expected_extension_id: String
        let capability = "dom:read"; let tab_id: UInt64
    }
    private struct Arm: Encodable {
        let expected_core_generation: UInt64; let expected_extension_id: String
        let capability: String; let tab_id: UInt64; let document_epoch: UInt64
    }
    func encode(to encoder: Encoder) throws {
        if case .inspect = self { var box = encoder.singleValueContainer(); try box.encode("inspect"); return }
        var box = encoder.container(keyedBy: Keys.self)
        switch self {
        case .inspect: break
        case .change(let generation, let extensionID, let capability, let grant):
            try box.encode(Change(expected_core_generation: generation, expected_extension_id: extensionID,
                                  capability: capability, action: grant ? "grant" : "revoke"), forKey: .change)
        case .review(let generation, let extensionID, let tab):
            try box.encode(Review(expected_core_generation: generation, expected_extension_id: extensionID, tab_id: tab), forKey: .inspect_ephemeral)
        case .arm(let review):
            try box.encode(Arm(expected_core_generation: review.core_generation, expected_extension_id: review.installed.extension_id,
                               capability: review.capability, tab_id: review.tab_id, document_epoch: review.document_epoch), forKey: .arm_ephemeral)
        }
    }
}
struct PermissionTab: Decodable, Equatable, Identifiable, Sendable {
    let id: UInt64
    let url: String?
}
enum PermissionFraming {
    static let maximumBytes = 128 * 1024
    static func frame<T: Encodable>(_ value: T) throws -> Data {
        let body = try JSONEncoder().encode(value)
        guard !body.isEmpty, body.count <= maximumBytes else { throw PanelFailure.invalid("Permission request exceeds its limit.") }
        var count = UInt32(body.count).littleEndian
        var result = withUnsafeBytes(of: &count) { Data($0) }; result.append(body); return result
    }
    static func length(_ prefix: Data, maximum: Int = maximumBytes) throws -> Int {
        guard prefix.count == 4 else { throw PanelFailure.invalid("Incomplete permission frame.") }
        let value = prefix.enumerated().reduce(UInt32(0)) { $0 | UInt32($1.element) << ($1.offset * 8) }
        guard value > 0, value <= maximum else { throw PanelFailure.invalid("Permission reply exceeds its limit.") }
        return Int(value)
    }
}
