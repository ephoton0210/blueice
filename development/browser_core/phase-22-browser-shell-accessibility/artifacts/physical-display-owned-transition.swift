// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
import AppKit
import CoreGraphics
import Darwin
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}
@discardableResult
func report(_ value: [String: Any]) -> Bool {
    guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) else { fail("Cannot encode transition state") }
    let bytes = data + Data([10])
    return bytes.withUnsafeBytes { buffer in
        guard let address = buffer.baseAddress else { return false }
        var offset = 0
        while offset < buffer.count {
            let written = Darwin.write(STDOUT_FILENO, address.advanced(by: offset), buffer.count - offset)
            if written < 0 && errno == EINTR { continue }
            guard written > 0 else { return false }
            offset += written
        }
        return true
    }
}
// A closed output pipe must not terminate the process before mode restoration.
signal(SIGPIPE, SIG_IGN)
var terminationSignal: Int32 = 0
let terminationMonitors: [DispatchSourceSignal] = [SIGINT, SIGTERM, SIGHUP].map { number in
    signal(number, SIG_IGN)
    let monitor = DispatchSource.makeSignalSource(signal: number, queue: .main)
    monitor.setEventHandler { terminationSignal = number }
    monitor.resume()
    return monitor
}
func waitForRelease(seconds: TimeInterval) -> [String: Any] {
    let deadline = ProcessInfo.processInfo.systemUptime + seconds
    var descriptor = pollfd(fd: STDIN_FILENO, events: Int16(POLLIN | POLLHUP), revents: 0)
    while true {
        // Dispatch signal callbacks on the main thread, outside signal context.
        RunLoop.current.run(until: Date().addingTimeInterval(0.001))
        if terminationSignal != 0 {
            return ["release_reason": "signal", "termination_signal": terminationSignal]
        }
        let remaining = deadline - ProcessInfo.processInfo.systemUptime
        if remaining <= 0 { return ["release_reason": "deadline", "deadline_expired": true] }
        let result = poll(&descriptor, 1, min(100, max(1, Int32(remaining * 1000))))
        if result > 0 { return ["release_reason": "stdin", "poll_result": result, "poll_events": descriptor.revents] }
        if result < 0 && errno != EINTR { return ["release_reason": "poll-error", "poll_errno": errno] }
    }
}
func modeInfo(_ mode: CGDisplayMode) -> [String: Any] {
    ["id": mode.ioDisplayModeID, "width": mode.width, "height": mode.height,
     "pixel_width": mode.pixelWidth, "pixel_height": mode.pixelHeight, "refresh_rate": mode.refreshRate]
}
let args = CommandLine.arguments
if args.count == 3, args[1] == "lease-probe", let seconds = Double(args[2]), seconds > 0, seconds <= 15 {
    report(["operation": "lease-probe-ready", "physical_display_changed": false, "lease_seconds": seconds])
    var outcome = waitForRelease(seconds: seconds)
    outcome["operation"] = "lease-probe-finished"
    outcome["physical_display_changed"] = false
    report(outcome)
    exit(0)
}
if args.count == 2, args[1] == "describe" {
    let id = CGMainDisplayID()
    guard let current = CGDisplayCopyDisplayMode(id),
          let modes = CGDisplayCopyAllDisplayModes(id, nil) as? [CGDisplayMode] else { fail("Cannot read display modes") }
    report(["operation": "describe", "display": id, "vendor": CGDisplayVendorNumber(id),
            "model": CGDisplayModelNumber(id), "current": modeInfo(current), "modes": modes.map(modeInfo)])
    exit(0)
}
guard args.count == 5, let display = UInt32(args[1]), let wanted = UInt32(args[2]),
      let ownerPID = Int32(args[3]) else { fail("Usage: describe OR display mode owned-app-PID resolved-app-bundle-path") }
guard CGDisplayIsOnline(display) != 0, CGDisplayVendorNumber(display) == 1715,
      CGDisplayModelNumber(display) == 10209 else { fail("Expected the previously identified PA279CRV display") }
let runners = NSRunningApplication.runningApplications(withBundleIdentifier: "cc.blueice.BrowserUITests.xctrunner")
guard runners.isEmpty else { fail("An existing native UI Runner must finish before display transitions") }
guard let owner = NSWorkspace.shared.frontmostApplication,
      owner.processIdentifier == ownerPID, owner.bundleIdentifier == "cc.blueice.BlueIce",
      owner.bundleURL?.resolvingSymlinksInPath().path == URL(fileURLWithPath: args[4]).resolvingSymlinksInPath().path else { fail("Exact owned BlueIce fixture must be foreground") }
guard let original = CGDisplayCopyDisplayMode(display),
      let modes = CGDisplayCopyAllDisplayModes(display, nil) as? [CGDisplayMode],
      let target = modes.first(where: { $0.ioDisplayModeID == wanted }),
      target.width >= 1280, target.height >= 720 else { fail("Unknown or unusable target mode") }
guard target.ioDisplayModeID != original.ioDisplayModeID else { fail("A real mode transition is required") }
let changed = CGDisplaySetDisplayMode(display, target, nil)
guard changed == .success else { fail("Display change failed: \(changed.rawValue)") }
report(["operation": "changed", "display": display, "owner_pid": ownerPID,
        "original": modeInfo(original), "requested": modeInfo(target), "lease_seconds": 15])
// The controller keeps stdin open while it observes the real app/Core state.
// EOF, explicit completion, termination signals or the deadline restore it.
let release = waitForRelease(seconds: 15)
let restored = CGDisplaySetDisplayMode(display, original, nil)
let actual = CGDisplayCopyDisplayMode(display)
report(["operation": "restored", "display": display, "result": restored.rawValue,
        "actual": actual.map(modeInfo) ?? [:], "initial_mode": original.ioDisplayModeID,
        "release": release])
guard restored == .success, actual?.ioDisplayModeID == original.ioDisplayModeID else { fail("Original display mode was not restored") }
