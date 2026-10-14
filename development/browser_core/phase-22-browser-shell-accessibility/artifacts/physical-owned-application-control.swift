// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import Foundation
import Darwin

let args = CommandLine.arguments
guard args.count == 4, let pid = Int32(args[1]), pid > 1,
      let app = NSRunningApplication(processIdentifier: pid),
      app.bundleIdentifier == "cc.blueice.BlueIce",
      app.bundleURL?.resolvingSymlinksInPath().path == URL(fileURLWithPath: args[2]).resolvingSymlinksInPath().path,
      app.executableURL?.resolvingSymlinksInPath() == URL(fileURLWithPath: args[2]).appendingPathComponent("Contents/MacOS/BlueIce").resolvingSymlinksInPath(),
      ["activate", "terminate"].contains(args[3]) else {
    FileHandle.standardError.write(Data("Exact owned BlueIce app PID/path/action required\n".utf8))
    exit(1)
}
let result = args[3] == "activate" ? app.activate(options: []) : app.terminate()
let report: [String: Any] = ["operation": args[3], "application": pid, "accepted": result]
let data = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
FileHandle.standardOutput.write(data + Data([10]))
exit(result ? 0 : 1)
