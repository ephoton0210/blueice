// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

/// Owns only Core-authorized bytes. Publication and revocation share a lock
/// because AppKit may request promised pasteboard data from another thread.
final class NativeDragPayload: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: Data]
    private var active = true

    init(values: [String: Data]) { self.values = values }

    @discardableResult
    func provide(_ type: String, publish: (Data) -> Void) -> Bool {
        lock.withLock {
            guard active, let data = values[type] else { return false }
            publish(data)
            return true
        }
    }

    func revoke() {
        lock.withLock {
            active = false
            values.removeAll(keepingCapacity: false)
        }
    }

    /// A pending owner decision blocks publication. A refusal atomically
    /// disables further reads before releasing the pasteboard service.
    func validateSource(_ decision: () -> Bool) -> Bool {
        lock.withLock {
            guard active else { return false }
            if decision() { return true }
            active = false
            values.removeAll(keepingCapacity: false)
            return false
        }
    }
}
