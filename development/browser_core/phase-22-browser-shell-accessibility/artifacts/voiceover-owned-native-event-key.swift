// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}
let arguments = CommandLine.arguments
guard arguments.count == 5, let ownerPID = Int32(arguments[1]), ownerPID > 1,
      let readerPID = Int32(arguments[3]), readerPID > 1 else {
    fail("Expected owned PID, exact candidate bundle, actual reader PID and bounded operation")
}
let bundle = URL(fileURLWithPath: arguments[2]).resolvingSymlinksInPath()
let candidate = URL(fileURLWithPath:
    "/private/tmp/blueice-native-assistant-scroll-fix-20261013/frontend/macos/.build/Build/Products/Debug/BlueIce.app").resolvingSymlinksInPath()
guard bundle == candidate else { fail("Only the current owned candidate bundle is supported") }
let operation = arguments[4]
let mainKeys: [String: CGKeyCode] = [
    "vo-right": 124, "vo-left": 123, "vo-into": 125, "vo-out": 126,
    "vo-action": 49, "vo-sync-focused": 118, "vo-read-cursor": 0,
    "vo-copy-phrase": 8,
]
guard let mainKey = mainKeys[operation] else { fail("Unsupported native VoiceOver operation") }
func guardedOwner() -> Bool {
    guard let owner = NSWorkspace.shared.frontmostApplication else { return false }
    let readers = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.VoiceOver")
    let expectedReader = URL(fileURLWithPath: "/System/Library/CoreServices/VoiceOver.app").resolvingSymlinksInPath()
    return owner.processIdentifier == ownerPID
        && owner.bundleIdentifier == "cc.blueice.BlueIce"
        && owner.bundleURL?.resolvingSymlinksInPath() == bundle
        && owner.executableURL?.resolvingSymlinksInPath()
            == bundle.appendingPathComponent("Contents/MacOS/BlueIce").resolvingSymlinksInPath()
        && NSWorkspace.shared.isVoiceOverEnabled
        && readers.count == 1 && readers[0].processIdentifier == readerPID
        && readers[0].bundleURL?.resolvingSymlinksInPath() == expectedReader
        && NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.VoiceOverQuickstart").isEmpty
        && NSWorkspace.shared.runningApplications.allSatisfy {
            !($0.bundleURL?.lastPathComponent.hasSuffix("-Runner.app") ?? false)
        }
}
guard CGPreflightPostEventAccess(), AXIsProcessTrusted(), guardedOwner() else {
    fail("Existing native event authorization and exact foreground/reader ownership required")
}
let transient: CGEventFlags = [.maskControl, .maskAlternate, .maskShift, .maskCommand, .maskSecondaryFn]
let initial = CGEventSource.flagsState(.combinedSessionState)
guard initial.intersection(transient).isEmpty else { fail("Existing held modifier keys must be released first") }
guard let source = CGEventSource(stateID: .combinedSessionState) else { fail("Cannot create session event source") }
var modifiers: [(CGKeyCode, CGEventFlags)] = [(59, .maskControl), (58, .maskAlternate)]
if ["vo-into", "vo-out", "vo-sync-focused", "vo-copy-phrase"].contains(operation) {
    modifiers.append((56, .maskShift))
}
if operation == "vo-sync-focused" { modifiers.append((63, .maskSecondaryFn)) }
var flags = initial
var held: [CGEvent] = []
var mainDown = false
func event(_ key: CGKeyCode, down: Bool, modifier: Bool, flags: CGEventFlags) -> CGEvent? {
    guard let value = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: down) else { return nil }
    if modifier { value.type = .flagsChanged }
    value.flags = flags
    value.setIntegerValueField(.eventSourceUserData, value: 0x424C5545494345)
    return value
}
var prepared: [(CGEvent, CGEvent)] = []
for (key, mask) in modifiers {
    let before = flags
    flags.insert(mask)
    guard let down = event(key, down: true, modifier: true, flags: flags),
          let up = event(key, down: false, modifier: true, flags: before) else {
        fail("Cannot allocate both modifier down/release events before posting")
    }
    prepared.append((down, up))
}
guard let mainPress = event(mainKey, down: true, modifier: false, flags: flags),
      let mainRelease = event(mainKey, down: false, modifier: false, flags: flags) else {
    fail("Cannot allocate both main down/release events before posting")
}
func releaseOwnedKeys() {
    if mainDown {
        mainRelease.post(tap: .cghidEventTap)
        mainDown = false
    }
    while let release = held.popLast() {
        release.post(tap: .cghidEventTap)
        usleep(20000)
    }
}
var delivered = false
do {
    defer { releaseOwnedKeys() }
    for (down, release) in prepared {
        guard guardedOwner() else { break }
        held.append(release)
        down.post(tap: .cghidEventTap)
        usleep(40000)
    }
    if held.count == modifiers.count, guardedOwner() {
        mainDown = true
        mainPress.post(tap: .cghidEventTap)
        usleep(80000)
        mainRelease.post(tap: .cghidEventTap)
        mainDown = false
        delivered = true
    }
}
usleep(80000)
let modifiersRestored = CGEventSource.flagsState(.combinedSessionState).intersection(transient)
    == initial.intersection(transient)
let result: [String: Any] = [
    "operation": operation, "delivered": delivered, "owner_pid": ownerPID,
    "reader_pid": readerPID, "event_source": "combinedSessionState",
    "event_tap": "cghidEventTap", "owned_modifier_keys_released": held.isEmpty,
    "observed_transient_modifier_state_restored": modifiersRestored,
    "foreground_reader_guard_after": guardedOwner(), "physical_voiceover_accepted": false,
]
print(String(decoding: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), as: UTF8.self))
exit(delivered && modifiersRestored ? 0 : 1)
