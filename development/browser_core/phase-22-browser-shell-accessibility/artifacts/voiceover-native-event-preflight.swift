// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import ApplicationServices
import CoreGraphics
import Foundation

let result: [String: Any] = [
    "scope": "Read-only native event-post and Accessibility preflight; no events or authorization requests",
    "post_event_access": CGPreflightPostEventAccess(),
    "accessibility_trusted": AXIsProcessTrusted(),
    "posted_event_count": 0,
    "physical_voiceover_accepted": false,
]
print(String(decoding: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), as: UTF8.self))
