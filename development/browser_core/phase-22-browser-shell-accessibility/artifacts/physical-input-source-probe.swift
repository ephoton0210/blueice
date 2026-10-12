// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import Carbon
import Foundation

func property(_ source: TISInputSource, _ key: CFString) -> AnyObject? {
    guard let pointer = TISGetInputSourceProperty(source, key) else { return nil }
    return Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue()
}
func sourceRow(_ source: TISInputSource) -> [String: Any] {
    return [
        "id": property(source, kTISPropertyInputSourceID) as? String ?? "unknown",
        "name": property(source, kTISPropertyLocalizedName) as? String ?? "unknown",
        "enabled": (property(source, kTISPropertyInputSourceIsEnabled) as? NSNumber)?.boolValue ?? false,
        "selected": (property(source, kTISPropertyInputSourceIsSelected) as? NSNumber)?.boolValue ?? false,
        "selectable": (property(source, kTISPropertyInputSourceIsSelectCapable) as? NSNumber)?.boolValue ?? false,
    ]
}
let current = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
let filter = [kTISPropertyInputSourceID as String: "com.apple.inputmethod.TCIM.Zhuyin"] as CFDictionary
let candidates = TISCreateInputSourceList(filter, true).takeRetainedValue() as! [TISInputSource]
let enabledCandidates = TISCreateInputSourceList(filter, false).takeRetainedValue() as! [TISInputSource]
let output: [String: Any] = [
    "scope": "Read-only system TIS input source inspection; no source selection or key injection",
    "current": sourceRow(current),
    "zhuyin_candidates": candidates.map(sourceRow),
    "enabled_zhuyin_count": enabledCandidates.count,
    "physical_ime_accepted": false,
]
print(String(decoding: try JSONSerialization.data(withJSONObject: output,
    options: [.prettyPrinted, .sortedKeys]), as: UTF8.self))
