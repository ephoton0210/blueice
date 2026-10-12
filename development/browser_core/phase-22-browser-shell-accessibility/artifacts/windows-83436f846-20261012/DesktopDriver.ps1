# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
$ErrorActionPreference='Stop'
$root='B:\BlueIce\workspaces\blueice-downloads'
$meta=Join-Path $root '.blueice-validation\acceptance-20261012-winui-json'
$artifacts=Join-Path $meta 'winui-artifacts'
$log=Join-Path $meta 'winui-desktop.log'
$code=1;$message=$null
function Trace([string]$phase){[IO.File]::AppendAllText((Join-Path $meta 'desktop-phase.log'),[DateTime]::UtcNow.ToString('o')+' '+$phase+"`n",[Text.UTF8Encoding]::new($false))}
Trace ('started apartment='+[Threading.Thread]::CurrentThread.ApartmentState)
try {
 if([Diagnostics.Process]::GetCurrentProcess().SessionId -ne 1){throw 'UI test must use desktop session 1'}
 $env:CARGO_TARGET_DIR=Join-Path $root 'target'
 $app=Join-Path $root 'frontend\winui\bin\x64\Debug\net10.0-windows10.0.22621.0\win-x64\BlueIce.WinUI.exe'
 Trace 'before-smoke'
 & "$meta\WindowSmoke-fixed.ps1" -AppExe $app -CoreExe "$root\target\debug\blueice-core.exe" -ArtifactDirectory $artifacts *> $log
 Trace 'smoke-returned'
 $code=0
} catch {$message=$_.Exception.Message;$_|Out-File -LiteralPath $log -Append}
Trace 'before-results-read'
$raw=if(Test-Path "$artifacts\results.txt"){[IO.File]::ReadAllText("$artifacts\results.txt",[Text.Encoding]::UTF8)}else{''}
Trace 'results-read'
$hashes=[ordered]@{}
foreach($path in @($log,"$artifacts\results.txt","$artifacts\window.png")){if(Test-Path -LiteralPath $path){$hashes[$path]=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}}
$result=[ordered]@{exit_code=$code;error=$message;pid=$PID;session=[Diagnostics.Process]::GetCurrentProcess().SessionId;apartment=[Threading.Thread]::CurrentThread.ApartmentState.ToString();pass_count=[regex]::Matches($raw,'(?m)^PASS ').Count;results=$raw;artifacts=$hashes}
Trace 'before-publish'
$serialized=($result|ConvertTo-Json -Depth 8)+"`n"
Trace 'serialized'
[IO.File]::WriteAllText((Join-Path $meta 'winui-desktop-result.json'),$serialized,[Text.UTF8Encoding]::new($false))
Trace 'result-published'
exit $code
