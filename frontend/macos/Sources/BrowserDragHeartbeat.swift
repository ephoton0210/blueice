// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Foundation

/// The main run loop owns the timer. Refusal stops every registered mode;
/// a delayed timer fires once instead of replaying missed drag iterations.
@MainActor
final class NativeDragHeartbeat {
    private var timer: Timer?

    init(interval: TimeInterval = 0.35,modes: [RunLoop.Mode],update: @escaping @MainActor () -> Bool) {
        precondition(interval.isFinite && interval > 0)
        let timer = Timer(timeInterval: interval,repeats: true) { timer in
            MainActor.assumeIsolated {
                guard timer.isValid else { return }
                if !update() { timer.invalidate() }
            }
        }
        self.timer = timer
        for mode in modes { RunLoop.main.add(timer,forMode: mode) }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }
}
