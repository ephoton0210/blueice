# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
function Measure-ValidationTargetBytes([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return 0L }
    $scanErrors = @()
    $files = @(Get-ChildItem -LiteralPath $Path -File -Recurse -ErrorAction SilentlyContinue -ErrorVariable scanErrors)
    foreach ($scanError in $scanErrors) {
        $exception = $scanError.Exception
        $vanished = $false
        while ($exception) {
            if ($exception -is [IO.DirectoryNotFoundException] -or $exception -is [IO.FileNotFoundException]) {
                $vanished = $true
                break
            }
            $exception = $exception.InnerException
        }
        if (-not $vanished -and $scanError.Exception -is [UnauthorizedAccessException]) {
            # NTFS can report access denied for a directory pending deletion.
            # Only tolerate that error after its exact path is proven absent.
            Start-Sleep -Milliseconds 10
            try { $null = Get-Item -LiteralPath ([string]$scanError.TargetObject) -Force -ErrorAction Stop }
            catch {
                $vanished = $_.CategoryInfo.Category -eq [Management.Automation.ErrorCategory]::ObjectNotFound -or
                    $_.Exception -is [IO.DirectoryNotFoundException] -or $_.Exception -is [IO.FileNotFoundException]
            }
        }
        if (-not $vanished) { throw $scanError }
    }
    $bytes = 0L
    foreach ($file in $files) { $bytes += $file.Length }
    return $bytes
}
