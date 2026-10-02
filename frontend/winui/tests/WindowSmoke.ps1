# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

param(
    [Parameter(Mandatory=$true)][string]$AppExe,
    [string]$CoreExe,
    [Parameter(Mandatory=$true)][string]$ArtifactDirectory
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class BlueIceWindow {
    [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr window, int x, int y, int width, int height, bool repaint);
    [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    [DllImport("dwmapi.dll")] public static extern int DwmFlush();
}
'@
New-Item -ItemType Directory -Force $ArtifactDirectory | Out-Null
$process = $null
$window = $null
$results = New-Object System.Collections.Generic.List[string]
function Find([string]$id) {
    $condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, $id)
    $element = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
    if (-not $element) { throw "Missing control: $id" }
    return $element
}
function Invoke([string]$id) { (Find $id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke() }
function AddressValue { return (Find 'AddressBar').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value }
function WaitFor([scriptblock]$predicate, [string]$description) {
    for ($i = 0; $i -lt 60; $i++) { if (& $predicate) { return }; Start-Sleep -Milliseconds 250 }
    throw "Timed out: $description"
}
function TabCount {
    $condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem)
    return $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition).Count
}
try {
    [BlueIceWindow]::SetThreadExecutionState(2147483651) | Out-Null
    if ($CoreExe) { $process = Start-Process $AppExe -ArgumentList @('--core-exe', $CoreExe) -PassThru }
    else { $process = Start-Process $AppExe -PassThru }
    $condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $process.Id)
    WaitFor { $script:window = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children, $condition); return $null -ne $script:window } 'visible WinUI window'
    [BlueIceWindow]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
    # UI Automation alone does not dismiss an idle desktop screen saver.
    [BlueIceWindow]::keybd_event(16,0,0,[UIntPtr]::Zero)
    [BlueIceWindow]::keybd_event(16,0,2,[UIntPtr]::Zero)
    [BlueIceWindow]::MoveWindow([IntPtr]$window.Current.NativeWindowHandle,20,30,960,640,$true) | Out-Null
    WaitFor { (Find 'BrowserStatus').Current.Name -eq 'Ready' } 'initial core frame'
    if ((AddressValue) -ne 'about:credits') { throw 'Initial URL does not match the core page' }
    $results.Add('PASS real WinUI window and core-rendered credits')
    (Find 'AddressBar').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('http://127.0.0.1:9/unsubmitted')
    Invoke 'ReloadButton'
    WaitFor { (AddressValue) -eq 'about:credits' } 'reload committed URL without submitting an address draft'
    $results.Add('PASS reload preserves the committed URL')
    Invoke 'SettingsButton'
    WaitFor { (AddressValue) -eq 'about:settings' -and (Find 'BackButton').Current.IsEnabled } 'settings and history'
    Invoke 'BackButton'
    WaitFor { (AddressValue) -eq 'about:credits' -and (Find 'ForwardButton').Current.IsEnabled } 'back history'
    Invoke 'ForwardButton'
    WaitFor { (AddressValue) -eq 'about:settings' } 'forward history'
    $results.Add('PASS settings, back and forward')
    Invoke 'AddButton'
    WaitFor { (TabCount) -eq 2 -and (AddressValue) -eq 'about:credits' } 'second tab'
    $tabCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem)
    $items = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabCondition)
    foreach ($item in $items) {
        if ($item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected) {
            $closeCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, 'CloseButton')
            $item.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $closeCondition).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
            break
        }
    }
    WaitFor { (TabCount) -eq 1 -and (AddressValue) -eq 'about:settings' } 'close selected tab and restore selection'
    $results.Add('PASS create, select and close tabs')
    (Find 'AddressBar').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('http://127.0.0.1:9/')
    Invoke 'GoButton'
    WaitFor { (Find 'BrowserStatus').Current.Name -like 'Navigation blocked:*' } 'unavailable gatekeeper response'
    $results.Add('PASS external navigation fails closed visibly')
    (Find 'AddressBar').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('about:credits')
    Invoke 'GoButton'
    WaitFor { (AddressValue) -eq 'about:credits' -and (Find 'BrowserStatus').Current.Name -eq 'Ready' } 'restore credits'
    $results.Add('PAGE_BOUNDS=' + (Find 'PageImage').Current.BoundingRectangle.ToString())
    $results.Add('ADDRESS_BOUNDS=' + (Find 'AddressBar').Current.BoundingRectangle.ToString())
    $results | Set-Content (Join-Path $ArtifactDirectory 'results.txt') -Encoding UTF8
    Start-Sleep -Seconds 5
    [BlueIceWindow]::DwmFlush() | Out-Null
    $bounds = $window.Current.BoundingRectangle
    $bitmap = New-Object System.Drawing.Bitmap([int]$bounds.Width, [int]$bounds.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen([int]$bounds.X,[int]$bounds.Y,0,0,$bitmap.Size)
    $page = (Find 'PageImage').Current.BoundingRectangle
    $darkPixels = 0
    for ($y = 16; $y -lt $page.Height - 16; $y += 4) {
        for ($x = 16; $x -lt $page.Width - 16; $x += 4) {
            $pixel = $bitmap.GetPixel([int]($page.X - $bounds.X + $x), [int]($page.Y - $bounds.Y + $y))
            if (($pixel.R + $pixel.G + $pixel.B) -lt 384) { $darkPixels++ }
        }
    }
    $bitmap.Save((Join-Path $ArtifactDirectory 'window.png'), [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    if ($darkPixels -lt 100) { throw 'Core page pixels are not visible on the Windows desktop' }
    $results.Add("PASS visible rendered page pixels ($darkPixels dark samples)")
    $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($process.Id)" | Where-Object { $_.Name -eq 'blueice-core.exe' })
    $directories = @($children | ForEach-Object { if ($_.CommandLine -match '--frame-dir\s+("[^"]+"|\S+)') { $matches[1].Trim('"') } })
    $window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
    WaitFor { $process.HasExited } 'frontend exit after window closes'
    foreach ($child in $children) { if (Get-Process -Id $child.ProcessId -ErrorAction SilentlyContinue) { throw 'Owned core process survived window close' } }
    foreach ($directory in $directories) { if (Test-Path $directory) { throw 'Frame directory survived window close' } }
    $results.Add('PASS window close stops core and removes frames')
} catch {
    $results.Add('FAILED ' + $_.Exception.Message)
    if ($window) { try { $results.Add('STATUS=' + (Find 'BrowserStatus').Current.Name) } catch { } }
    throw
} finally {
    [BlueIceWindow]::SetThreadExecutionState(2147483648) | Out-Null
    $results | Set-Content (Join-Path $ArtifactDirectory 'results.txt') -Encoding UTF8
    if ($process -and -not $process.HasExited) {
        $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($process.Id)" | Where-Object { $_.Name -eq 'blueice-core.exe' })
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        foreach ($child in $children) { Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue }
    }
}
