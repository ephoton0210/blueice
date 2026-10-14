# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

param(
    [string]$StorageRoot = 'B:\BlueIce',
    [string]$Workspace = (Join-Path $PSScriptRoot '..'),
    [Parameter(Mandatory = $true, Position = 0, ValueFromRemainingArguments = $true)]
    [string[]]$Command
)

$ErrorActionPreference = 'Stop'
$workspacePath = (Resolve-Path -LiteralPath $Workspace).Path.TrimEnd('\')
$storagePath = [IO.Path]::GetFullPath($StorageRoot).TrimEnd('\')
$drive = [IO.Path]::GetPathRoot($storagePath)
if ($drive -notmatch '^[A-Za-z]:\\$' -or $drive -eq 'C:\') {
    throw 'BlueIce tests require a dedicated local data drive.'
}
$volume = Get-Volume -DriveLetter $drive[0]
if ($volume.FileSystem -ne 'NTFS' -or $volume.FileSystemLabel -ne 'BlueIceTests320') {
    throw 'The test drive must be the BlueIceTests320 NTFS volume.'
}
if (-not $workspacePath.StartsWith($drive, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Move the workspace to the dedicated test drive before running tests.'
}
if ($volume.SizeRemaining -lt 50GB) {
    throw 'The test drive must retain at least 50 GiB free.'
}
$variables = @{
    CARGO_TARGET_DIR = Join-Path $workspacePath 'target'
    CARGO_HOME = Join-Path $storagePath 'cache\cargo'
    RUSTUP_HOME = Join-Path $storagePath 'cache\rustup'
    NUGET_PACKAGES = Join-Path $storagePath 'cache\nuget'
    DOTNET_CLI_HOME = Join-Path $storagePath 'cache\dotnet'
    TEMP = Join-Path $storagePath 'tmp'
    TMP = Join-Path $storagePath 'tmp'
    CARGO_INCREMENTAL = '0'
    CARGO_PROFILE_DEV_DEBUG = '0'
    CARGO_PROFILE_TEST_DEBUG = '0'
    DOTNET_CLI_TELEMETRY_OPTOUT = '1'
    DOTNET_NOLOGO = '1'
}
$previous = @{}
foreach ($name in $variables.Keys) {
    $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
foreach ($name in @('CARGO_TARGET_DIR', 'CARGO_HOME', 'RUSTUP_HOME', 'NUGET_PACKAGES', 'DOTNET_CLI_HOME', 'TEMP')) {
    $null = New-Item -ItemType Directory -Path $variables[$name] -Force
}
Push-Location -LiteralPath $workspacePath
try {
    foreach ($name in $variables.Keys) {
        [Environment]::SetEnvironmentVariable($name, $variables[$name], 'Process')
    }
    Write-Host "BlueIce test storage: $storagePath; workspace: $workspacePath"
    $executable = $Command[0]
    $arguments = @($Command | Select-Object -Skip 1)
    $global:LASTEXITCODE = 0
    & $executable @arguments
    $exitCode = $LASTEXITCODE
} finally {
    foreach ($name in $previous.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process')
    }
    Pop-Location
}
exit $exitCode
