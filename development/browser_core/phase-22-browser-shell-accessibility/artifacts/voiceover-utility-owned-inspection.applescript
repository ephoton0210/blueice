-- This Source Code Form is subject to the terms of the Mozilla Public
-- License, v. 2.0. If a copy of the MPL was not distributed with this
-- file, You can obtain one at https://mozilla.org/MPL/2.0/.
use framework "AppKit"
use scripting additions
on readAttribute(theElement, theName)
    tell application "System Events"
        try
            return value of attribute theName of theElement
        on error
            return "unavailable"
        end try
    end tell
end readAttribute
on run arguments
    if (count arguments) is not in {1, 2} then error "Expected newly owned system Utility PID and optional Commands view"
    set expectedPID to item 1 of arguments as integer
    set frontApp to current application's NSWorkspace's sharedWorkspace()'s frontmostApplication()
    if frontApp is missing value then error "Cannot verify foreground Utility"
    if (frontApp's processIdentifier() as integer) is not expectedPID then error "Exact owned Utility must be foreground"
    if (frontApp's bundleIdentifier() as text) is not "com.apple.VoiceOverUtility" then error "Unexpected Utility bundle"
    set expectedURL to current application's NSURL's fileURLWithPath:"/System/Applications/Utilities/VoiceOver Utility.app"
    set expectedPath to expectedURL's URLByResolvingSymlinksInPath()'s |path|() as text
    if (frontApp's bundleURL()'s URLByResolvingSymlinksInPath()'s |path|() as text) is not expectedPath then error "Unexpected installed Utility path"
    tell application "System Events"
        set utilityProcess to first application process whose unix id is expectedPID
        if (count windows of utilityProcess) is not 1 then error "Expected exactly one owned Utility window"
        set contentsList to entire contents of window 1 of utilityProcess
    end tell
    if (count contentsList) > 1000 then error "Utility tree exceeds inspection bound"
    if (count arguments) is 2 then
        if item 2 of arguments is not "commands" then error "Only Commands view selection is supported"
        set matches to {}
        repeat with candidate in contentsList
            if my readAttribute(candidate, "AXRole") is "AXStaticText" then
                if my readAttribute(candidate, "AXValue") is in {"指令", "Commands"} then set end of matches to contents of candidate
            end if
        end repeat
        if (count matches) is not 1 then error "Expected exactly one Commands category"
        tell application "System Events"
            set categoryRow to value of attribute "AXParent" of item 1 of matches
            repeat 3 times
                if value of attribute "AXRole" of categoryRow is "AXRow" then exit repeat
                set categoryRow to value of attribute "AXParent" of categoryRow
            end repeat
            if value of attribute "AXRole" of categoryRow is not "AXRow" then error "Commands category is not a sidebar row"
            select categoryRow
        end tell
        delay 0.5
        tell application "System Events" to set contentsList to entire contents of window 1 of utilityProcess
        if (count contentsList) > 1000 then error "Commands tree exceeds inspection bound"
    end if
    set controls to {}
    repeat with candidate in contentsList
        set theRole to my readAttribute(candidate, "AXRole")
        if theRole is not in {"AXTextField", "AXTextArea", "AXRow", "AXCell"} then
            set controlID to my readAttribute(candidate, "AXIdentifier")
            set controlTitle to my readAttribute(candidate, "AXTitle")
            set controlDescription to my readAttribute(candidate, "AXDescription")
            set controlValue to "not-read"
            if theRole is not in {"AXTextField", "AXTextArea"} then set controlValue to my readAttribute(candidate, "AXValue")
            if theRole is in {"AXCheckBox", "AXRadioButton", "AXButton"} then
                set childLabels to {}
                tell application "System Events" to set childElements to UI elements of candidate
                repeat with childElement in childElements
                    if my readAttribute(childElement, "AXRole") is "AXStaticText" then set end of childLabels to my readAttribute(childElement, "AXValue")
                end repeat
                if (count childLabels) > 0 then set controlDescription to childLabels
            end if
            set end of controls to {theRole, controlID, controlTitle, controlDescription, controlValue}
        end if
    end repeat
    return controls
end run
