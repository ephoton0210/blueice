# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

param([ValidateSet('Debug','Release')][string]$Configuration = 'Debug')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Push-Location $root
try {
    $cargoArguments = @('+1.96.0','build','-p','blueice-engine','--bin','blueice-core','--locked')
    if ($Configuration -eq 'Release') { $cargoArguments += '--release' }
    & cargo @cargoArguments
    if ($LASTEXITCODE -ne 0) { throw 'Core build failed' }
    dotnet build (Join-Path $PSScriptRoot 'BlueIce.WinUI.csproj') -c $Configuration -p:Platform=x64 -p:RestoreLockedMode=true
    if ($LASTEXITCODE -ne 0) { throw 'WinUI build failed' }
    $output = Join-Path $PSScriptRoot "bin\x64\$Configuration\net10.0-windows10.0.22621.0\win-x64"
    $target = Join-Path $root "target\$($Configuration.ToLowerInvariant())\blueice-core.exe"
    Copy-Item $target (Join-Path $output 'blueice-core.exe') -Force
    "FRONTEND_EXE=$(Join-Path $output 'BlueIce.WinUI.exe')"
} finally { Pop-Location }
