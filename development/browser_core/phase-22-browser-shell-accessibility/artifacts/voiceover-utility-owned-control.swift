// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Foundation
import Darwin
import CoreGraphics

let bundle = URL(fileURLWithPath: "/System/Applications/Utilities/VoiceOver Utility.app").resolvingSymlinksInPath()
let identifier = "com.apple.VoiceOverUtility"
let arguments = CommandLine.arguments
if arguments.count == 2, arguments[1] == "status" {
    let rows = NSRunningApplication.runningApplications(withBundleIdentifier: identifier).map { app in
        let windows = (CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []).filter {
            ($0[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == app.processIdentifier
                && ($0[kCGWindowLayer as String] as? NSNumber)?.intValue == 0
        }.map { entry in
            ["window_id": entry[kCGWindowNumber as String] ?? "unavailable",
             "bounds": entry[kCGWindowBounds as String] ?? "unavailable"]
        }
        return ["pid": app.processIdentifier, "bundle_id": identifier,
         "bundle": app.bundleURL?.resolvingSymlinksInPath().path ?? "unavailable",
         "expected_system_bundle": app.bundleURL?.resolvingSymlinksInPath() == bundle,
         "active": app.isActive, "windows": windows] as [String: Any]
    }
    print(String(decoding: try JSONSerialization.data(withJSONObject: ["applications": rows], options: [.sortedKeys]), as: UTF8.self))
    exit(0)
}
guard arguments.count == 3, let pid = Int32(arguments[1]), pid > 1,
      ["activate", "terminate"].contains(arguments[2]),
      let app = NSRunningApplication(processIdentifier: pid),
      app.bundleIdentifier == identifier,
      app.bundleURL?.resolvingSymlinksInPath() == bundle,
      app.executableURL?.resolvingSymlinksInPath()
        == bundle.appendingPathComponent("Contents/MacOS/VoiceOver Utility").resolvingSymlinksInPath() else {
    FileHandle.standardError.write(Data("Exact newly owned system Utility PID/path/action required\n".utf8))
    exit(1)
}
let accepted = arguments[2] == "activate" ? app.activate(options: []) : app.terminate()
print(String(decoding: try JSONSerialization.data(withJSONObject:
    ["pid": pid, "operation": arguments[2], "accepted": accepted], options: [.sortedKeys]), as: UTF8.self))
exit(accepted ? 0 : 1)
