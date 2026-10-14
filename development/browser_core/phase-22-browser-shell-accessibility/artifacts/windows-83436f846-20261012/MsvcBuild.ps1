# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
$ErrorActionPreference='Stop'
$root='B:\BlueIce\workspaces\blueice-downloads'
Push-Location -LiteralPath $root
try{
 & cargo +1.96.0-x86_64-pc-windows-msvc build -p blueice-engine --bin blueice-core --locked
 if($LASTEXITCODE -ne 0){throw 'MSVC Core build failed'}
 & dotnet build (Join-Path $root 'frontend\winui\BlueIce.WinUI.csproj') -c Debug -p:Platform=x64 -p:RestoreLockedMode=true
 if($LASTEXITCODE -ne 0){throw 'WinUI build failed'}
 $output=Join-Path $root 'frontend\winui\bin\x64\Debug\net10.0-windows10.0.22621.0\win-x64'
 Copy-Item -LiteralPath (Join-Path $root 'target\debug\blueice-core.exe') -Destination (Join-Path $output 'blueice-core.exe') -Force
 "FRONTEND_EXE=$(Join-Path $output 'BlueIce.WinUI.exe')"
}finally{Pop-Location}
