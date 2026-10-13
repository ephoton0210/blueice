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
guard [2, 3].contains(arguments.count), let pid = Int32(arguments[1]), pid > 1 else {
    fail("Expected exact newly owned Utility PID")
}
let expected = URL(fileURLWithPath: "/System/Applications/Utilities/VoiceOver Utility.app").resolvingSymlinksInPath()
func owner() -> Bool {
    guard let app = NSWorkspace.shared.frontmostApplication else { return false }
    return app.processIdentifier == pid && app.bundleIdentifier == "com.apple.VoiceOverUtility"
        && app.bundleURL?.resolvingSymlinksInPath() == expected
        && app.executableURL?.resolvingSymlinksInPath()
            == expected.appendingPathComponent("Contents/MacOS/VoiceOver Utility").resolvingSymlinksInPath()
        && NSWorkspace.shared.runningApplications.allSatisfy {
            !($0.bundleURL?.lastPathComponent.hasSuffix("-Runner.app") ?? false)
        }
}
guard AXIsProcessTrusted(), owner() else { fail("Existing Accessibility authority and exact foreground Utility required") }
func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var result: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &result) == .success else { return nil }
    return result
}
let application = AXUIElementCreateApplication(pid)
guard let windows = attribute(application, kAXWindowsAttribute) as? [AXUIElement], windows.count == 1 else {
    fail("Expected exactly one owned Utility window")
}
var queue: [(AXUIElement, Int?)] = [(windows[0], nil)]
var seen: [AXUIElement] = []
var records: [[String: Any]] = []
var index = 0
let names = [kAXRoleDescriptionAttribute, kAXIdentifierAttribute, kAXTitleAttribute,
             kAXDescriptionAttribute, kAXHelpAttribute, kAXValueAttribute]
while index < queue.count {
    guard owner(), queue.count <= 1000 else { fail("Utility ownership changed or tree bound exceeded") }
    let (element, parent) = queue[index]
    index += 1
    if seen.contains(where: { CFEqual($0, element) }) { continue }
    seen.append(element)
    let role = attribute(element, kAXRoleAttribute) as? String ?? "unavailable"
    // Do not inspect or descend into greeting/search/other text entry fields.
    if ["AXTextField", "AXTextArea"].contains(role) {
        var metadata: [String: Any] = ["index": records.count, "role": role, "parent": parent.map { $0 as Any } ?? NSNull()]
        for name in [kAXIdentifierAttribute, kAXDescriptionAttribute, kAXRoleDescriptionAttribute] {
            if let value = attribute(element, name) as? String { metadata[name] = String(value.prefix(512)) }
        }
        metadata["text_value_read"] = false
        records.append(metadata)
        continue
    }
    let ownIndex = records.count
    var row: [String: Any] = ["index": ownIndex, "role": role, "parent": parent.map { $0 as Any } ?? NSNull()]
    var attributeNames: CFArray?
    if AXUIElementCopyAttributeNames(element, &attributeNames) == .success {
        row["attribute_names"] = (attributeNames as? [String] ?? []).sorted()
    }
    var values: [String: Any] = [:]
    for name in names {
        guard let value = attribute(element, name) else { continue }
        if let text = value as? String { values[name] = String(text.prefix(4096)) }
        else if let number = value as? NSNumber { values[name] = number }
    }
    if let label = attribute(element, kAXTitleUIElementAttribute), CFGetTypeID(label) == AXUIElementGetTypeID() {
        let labelElement = label as! AXUIElement
        let labelRole = attribute(labelElement, kAXRoleAttribute) as? String
        if labelRole == "AXStaticText" {
            for name in [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute] {
                if let value = attribute(labelElement, name) as? String {
                    values["AXTitleUIElement." + name] = String(value.prefix(4096))
                }
            }
        }
    }
    row["values"] = values
    records.append(row)
    if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] {
        let inspected: [AXUIElement]
        if children.count > 256 {
            if let visible = attribute(element, kAXVisibleChildrenAttribute) as? [AXUIElement], visible.count <= 256 {
                inspected = visible
                records[ownIndex]["children_traversal"] = "visible-only"
            } else {
                inspected = Array(children.prefix(32))
                records[ownIndex]["children_traversal"] = "first-32-overview"
            }
            records[ownIndex]["total_child_count"] = children.count
        } else { inspected = children }
        queue.append(contentsOf: inspected.map { ($0, ownIndex) })
    }
}
guard owner() else { fail("Utility ownership changed after read-only traversal") }
if arguments.count == 3 {
    if arguments[2] == "done-command-browser" {
        let buttons = seen.filter { element in
            attribute(element, kAXRoleAttribute) as? String == "AXButton"
                && ["完成", "Done"].contains(attribute(element, kAXDescriptionAttribute) as? String ?? "")
        }
        guard buttons.count == 1, owner(),
              AXUIElementPerformAction(buttons[0], kAXPressAction as CFString) == .success else {
            fail("One exact owned command browser Done button is required")
        }
        print("{\"operation\":\"closed-command-browser\",\"command_assignments_changed\":false}")
        exit(0)
    }
    if arguments[2] == "select-copy-command" {
        let labels = seen.filter { element in
            attribute(element, kAXRoleAttribute) as? String == "AXStaticText"
                && attribute(element, kAXValueAttribute) as? String == "將最後一段詞句拷貝到剪貼板"
        }
        guard labels.count == 1 else { fail("Expected one exact last-phrase copy command") }
        var ancestor = attribute(labels[0], kAXParentAttribute)
        for _ in 0..<4 {
            guard let value = ancestor, CFGetTypeID(value) == AXUIElementGetTypeID() else { break }
            let parent = value as! AXUIElement
            if attribute(parent, kAXRoleAttribute) as? String == "AXRow" {
                guard owner(), AXUIElementSetAttributeValue(parent, kAXSelectedAttribute as CFString, kCFBooleanTrue) == .success else {
                    fail("Cannot select the exact owned command row")
                }
                print("{\"operation\":\"selected-copy-command\",\"command_assignments_changed\":false}")
                exit(0)
            }
            ancestor = attribute(parent, kAXParentAttribute)
        }
        fail("Copy command row not found within the bounded parent chain")
    }
    if arguments[2] == "filter-copy-phrase" {
        let matches = seen.filter { element in
            guard attribute(element, kAXRoleAttribute) as? String == "AXTextField" else { return false }
            var ancestor = attribute(element, kAXParentAttribute)
            for _ in 0..<8 {
                guard let value = ancestor, CFGetTypeID(value) == AXUIElementGetTypeID() else { return false }
                let parent = value as! AXUIElement
                if attribute(parent, kAXRoleAttribute) as? String == "AXSheet" { return true }
                ancestor = attribute(parent, kAXParentAttribute)
            }
            return false
        }
        guard matches.count == 1, owner(), ["", "複製", "朗讀"].contains(attribute(matches[0], kAXValueAttribute) as? String ?? "unavailable"),
              AXUIElementSetAttributeValue(matches[0], kAXValueAttribute as CFString, "拷貝" as CFString) == .success else {
            fail("One empty search field in the newly owned command sheet is required")
        }
        print("{\"operation\":\"filtered-copy-commands\",\"command_assignments_changed\":false}")
        exit(0)
    }
    guard arguments[2] == "open-command-browser" else { fail("Unsupported Utility view action") }
    let matches = seen.filter { element in
        guard attribute(element, kAXRoleAttribute) as? String == "AXButton",
              let label = attribute(element, kAXTitleUIElementAttribute),
              CFGetTypeID(label) == AXUIElementGetTypeID() else { return false }
        let labelElement = label as! AXUIElement
        return ["指令集：", "Command set:"].contains(attribute(labelElement, kAXValueAttribute) as? String ?? "")
            && ["編輯⋯", "Edit…", "Edit...", "Edit"].contains(attribute(element, kAXDescriptionAttribute) as? String ?? "")
    }
    guard matches.count == 1, owner(),
          AXUIElementPerformAction(matches[0], kAXPressAction as CFString) == .success else {
        fail("One exact owned Command set Edit button is required")
    }
    print("{\"operation\":\"opened-command-browser\",\"preferences_changed\":false}")
    exit(0)
}
let result: [String: Any] = ["scope": "Read-only native AX attributes of exact newly owned Utility; no actions, text entries or permission requests",
    "pid": pid, "controls": records, "physical_voiceover_accepted": false]
print(String(decoding: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), as: UTF8.self))
