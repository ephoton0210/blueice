// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import ApplicationServices
import Foundation
import Darwin

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}
let arguments = CommandLine.arguments
guard arguments.count == 4, let ownerPID = Int32(arguments[1]), ownerPID > 1,
      let readerPID = Int32(arguments[3]), readerPID > 1 else {
    fail("Expected exact owned BlueIce PID/bundle and actual system reader PID")
}
let bundle = URL(fileURLWithPath: arguments[2]).resolvingSymlinksInPath()
let expectedBundle = URL(fileURLWithPath:
    "/private/tmp/blueice-native-assistant-scroll-fix-20261013/frontend/macos/.build/Build/Products/Debug/BlueIce.app").resolvingSymlinksInPath()
guard bundle == expectedBundle else { fail("Unexpected owned fixture bundle") }
let expectedReader = URL(fileURLWithPath: "/System/Library/CoreServices/VoiceOver.app").resolvingSymlinksInPath()
func guarded() -> Bool {
    guard let app = NSWorkspace.shared.frontmostApplication else { return false }
    let readers = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.VoiceOver")
    return app.processIdentifier == ownerPID && app.bundleIdentifier == "cc.blueice.BlueIce"
        && app.bundleURL?.resolvingSymlinksInPath() == bundle
        && NSWorkspace.shared.isVoiceOverEnabled
        && readers.count == 1 && readers[0].processIdentifier == readerPID
        && readers[0].bundleURL?.resolvingSymlinksInPath() == expectedReader
        && NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.VoiceOverQuickstart").isEmpty
        && NSWorkspace.shared.runningApplications.allSatisfy {
            !($0.bundleURL?.lastPathComponent.hasSuffix("-Runner.app") ?? false)
        }
}
guard AXIsProcessTrusted(), guarded() else { fail("Existing AX authority and exact foreground/reader required") }
func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
    return value
}
let reader = AXUIElementCreateApplication(readerPID)
let deadline = ProcessInfo.processInfo.systemUptime + 8
var windowMetadata: [[String: Any]] = []
var lastError = "Caption window unavailable"
var acceptedText: String?
repeat {
    guard guarded() else { fail("Foreground/reader ownership changed") }
    var rawWindows: CFTypeRef?
    let error = AXUIElementCopyAttributeValue(reader, kAXWindowsAttribute as CFString, &rawWindows)
    let windows = rawWindows as? [AXUIElement] ?? []
    guard windows.count <= 16 else { fail("Reader window inventory exceeds bound") }
    windowMetadata = []
    for window in windows {
        let title = attribute(window, kAXTitleAttribute) as? String ?? ""
        let identifier = attribute(window, kAXIdentifierAttribute) as? String ?? ""
        let role = attribute(window, kAXRoleAttribute) as? String ?? "unavailable"
        let isCaption = ["VoiceOver Caption Panel", "旁白字幕面板", "字幕面板", "Caption Panel"].contains(title)
            || identifier.lowercased().contains("caption")
        // Never emit text from other reader dialogs or from unknown captions.
        windowMetadata.append(["role": role, "caption_title_match": isCaption,
                               "has_identifier": !identifier.isEmpty])
        if !isCaption { continue }
        var queue = [window]
        var seen: [AXUIElement] = []
        var index = 0
        var texts: [String] = []
        while index < queue.count {
            guard queue.count <= 128, guarded() else { fail("Caption tree bound or ownership changed") }
            let element = queue[index]
            index += 1
            if seen.contains(where: { CFEqual($0, element) }) { continue }
            seen.append(element)
            let elementRole = attribute(element, kAXRoleAttribute) as? String ?? ""
            if ["AXStaticText", "AXTextArea", "AXTextField"].contains(elementRole),
               let value = attribute(element, kAXValueAttribute) as? String, value.utf8.count <= 4096 {
                texts.append(value)
            }
            if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] {
                guard children.count <= 64 else { fail("Caption child count exceeds bound") }
                queue.append(contentsOf: children)
            }
        }
        let text = texts.joined(separator: " ")
        if ["Name", "hello", "Open destination"].contains(where: { text.contains($0) }) {
            acceptedText = text
            break
        }
        lastError = "Caption exists but contains no owned fixture phrase"
    }
    if error != .success { lastError = "Reader AXWindows error \(error.rawValue)" }
    if acceptedText != nil { break }
    RunLoop.current.run(until: Date().addingTimeInterval(0.1))
} while ProcessInfo.processInfo.systemUptime < deadline
guard guarded() else { fail("Ownership changed after caption observation") }
var result: [String: Any] = ["operation": "caption-observation-diagnostic", "reader_pid": readerPID,
    "owner_pid": ownerPID, "window_metadata": windowMetadata, "error": lastError,
    "physical_voiceover_accepted": false, "clipboard_written": false,
    "preferences_changed": false]
if let text = acceptedText {
    result["operation"] = "phrase"
    result["text"] = text
    result["source"] = "Actual system VoiceOver caption panel read-only AX value"
    result.removeValue(forKey: "error")
}
print(String(decoding: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), as: UTF8.self))
exit(acceptedText == nil ? 1 : 0)
