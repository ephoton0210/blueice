# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding($false)
$root='B:\BlueIce\workspaces\blueice-downloads'
$meta=Join-Path $root '.blueice-validation\acceptance-20261012-winui-json'
$resultPath=Join-Path $meta 'acceptance-result.json'
if(Test-Path -LiteralPath $resultPath){throw 'An acceptance attempt already exists'}
$manifest=Get-Content -LiteralPath (Join-Path $meta 'manifest.json') -Encoding UTF8 -Raw | ConvertFrom-Json
$state=[ordered]@{status='running';started=[DateTime]::UtcNow.ToString('o');pid=$PID;root=$root;parent_commit=$manifest.parent_commit;manifest_sha256=(Get-FileHash (Join-Path $meta 'manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant();steps=@();target=(Join-Path $root 'target');source_files=@($manifest.files.PSObject.Properties).Count;program_files=@($manifest.program_files.PSObject.Properties).Count;ui_task=$null}
 . (Join-Path $meta 'state-store30.ps1')
 . (Join-Path $meta 'budget-probe.ps1')
function Save-State([switch]$AllowDeferred) {
 $published=Write-ValidationStateAtomic $resultPath $state -AllowDeferred:$AllowDeferred
 if(-not $published){
  [IO.File]::AppendAllText((Join-Path $meta 'state-publication-deferred.log'),[DateTime]::UtcNow.ToString('o')+" transient sharing lock; runtime remains monitored`n",(New-Object Text.UTF8Encoding($false)))
 }
}
function Verify-Source {
 foreach($entry in $manifest.files.PSObject.Properties){
  $path=Join-Path $root $entry.Name
  if((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value.sha256){throw ('Changed source: '+$entry.Name)}
 }
 $link=Get-Item -LiteralPath (Join-Path $root $manifest.corpus_path) -Force
 if($link.LinkType -ne 'Junction' -or @($link.Target)[0] -ne 'B:\BlueIce\corpora\test262-72faf8ec-verified'){throw 'Unexpected corpus junction'}
 if((Get-FileHash (Join-Path $link.FullName '.bluejs-manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant() -ne '8abd6392abcdde3aa17326391ed008a0336a253364c1676caa1af31b8a5bffd8'){throw 'Changed corpus manifest'}
}
function Check-Budget {
 $freeB=(Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='B:'").FreeSpace
 $freeC=(Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'").FreeSpace
 $bytes=0
 $bytes=Measure-ValidationTargetBytes $env:CARGO_TARGET_DIR
 if($freeB -lt 50GB -or $freeC -lt 4GB -or $bytes -gt 24GB){throw "Disk budget exceeded: B=$freeB C=$freeC target=$bytes"}
 return [ordered]@{free_b=$freeB;free_c=$freeC;target_bytes=$bytes;max_target_bytes=24GB;min_free_b=50GB;min_free_c=4GB}
}
function Run-Step([string]$name,[string]$command) {
 Verify-Source
 $state.current=$name;Save-State
 $log=Join-Path $meta ($name+'.log')
 $stdoutLog=Join-Path $meta ($name+'.stdout.log')
 $stderrLog=Join-Path $meta ($name+'.stderr.log')
 $wrapper=Join-Path $meta ($name+'-command.ps1')
 $body="`$ErrorActionPreference='Stop'`n[Console]::OutputEncoding=New-Object Text.UTF8Encoding(`$false)`nSet-Location -LiteralPath '$root'`ntry { $command; if(`$LASTEXITCODE -ne 0){exit `$LASTEXITCODE}; exit 0 } catch { `$_ | Out-String | Write-Output; exit 1 }`n"
 [IO.File]::WriteAllText($wrapper,$body,(New-Object Text.UTF8Encoding($false)))
 $started=Get-Date
 $child=Start-Process -FilePath "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$wrapper) -WorkingDirectory $root -RedirectStandardOutput $stdoutLog -RedirectStandardError $stderrLog -PassThru
 $null=$child.Handle
 $state.child_pid=$child.Id;$state.child_started=$child.StartTime.ToUniversalTime().ToString('o');Save-State -AllowDeferred
 try {while(-not $child.HasExited){$state.disk=Check-Budget;Save-State -AllowDeferred;Start-Sleep -Seconds 2;$child.Refresh()}}
 catch {if(-not $child.HasExited){& taskkill.exe /PID $child.Id /T /F | Out-Null;$child.WaitForExit()};throw}
 $child.WaitForExit()
 $encoding=New-Object Text.UTF8Encoding($false)
 $raw=[IO.File]::ReadAllText($stdoutLog,$encoding)+[IO.File]::ReadAllText($stderrLog,$encoding)
 [IO.File]::WriteAllText($log,$raw,[Text.Encoding]::Unicode)
 $counts=[regex]::Matches($raw,'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;')
 $pass=0;$fail=0;$ignore=0
 foreach($m in $counts){$pass+=[int]$m.Groups[2].Value;$fail+=[int]$m.Groups[3].Value;$ignore+=[int]$m.Groups[4].Value}
 $record=[ordered]@{name=$name;command=$command;exit_code=$child.ExitCode;seconds=((Get-Date)-$started).TotalSeconds;log=$log;log_sha256=(Get-FileHash $log -Algorithm SHA256).Hash.ToLowerInvariant();rust_counts=@{passed=$pass;failed=$fail;ignored=$ignore;groups=$counts.Count}}
 $state.steps+=@($record);Save-State
 if($child.ExitCode -ne 0){throw "Failed step: $name, exit $($child.ExitCode)"}
 if($name -eq 'rust-test' -and ($counts.Count -eq 0 -or $fail -ne 0)){throw 'No valid Rust workspace test summary'}
 if($name -eq 'winui-protocol' -and ($raw -notmatch '(?m)^5 tests passed\s*$' -or @([regex]::Matches($raw,'(?m)^PASS ')).Count -ne 5)){throw 'Expected five WinUI protocol cases'}
 Verify-Source
}
$taskName=$null
try {
 $state.process_execution_policy=(Get-ExecutionPolicy -Scope Process).ToString()
 $state.execution_policy_scopes_before=@(Get-ExecutionPolicy -List | Where-Object {$_.Scope -ne 'Process'} | ForEach-Object {[ordered]@{scope=$_.Scope.ToString();policy=$_.ExecutionPolicy.ToString()}})
 Verify-Source
 $busy=@(Get-CimInstance Win32_Process | Where-Object {$_.Name -match '^(cargo|rustc|blueice-core|BlueIce.WinUI|testhost|dotnet)\.exe$'})
 if($busy.Count){throw ('Existing related runtime processes: '+(($busy|Select-Object ProcessId,Name,CommandLine|ConvertTo-Json -Compress)))}
 $env:PATH='B:\BlueIce\cache\cargo\bin;'+$env:PATH
 $originalToolchain=$env:RUSTUP_TOOLCHAIN
 $beforeToolchains=(& rustup toolchain list | Out-String)
 $state.toolchains_before=$beforeToolchains
 $env:RUSTUP_TOOLCHAIN='1.96.0-x86_64-pc-windows-msvc'
 $env:CARGO_TARGET_DIR=Join-Path $root 'target'
 $env:CARGO_INCREMENTAL='0';$env:CARGO_PROFILE_DEV_DEBUG='0';$env:CARGO_PROFILE_TEST_DEBUG='0'
 $env:DOTNET_CLI_TELEMETRY_OPTOUT='1';$env:DOTNET_NOLOGO='1'
 $devCmd=@('X:\BuildTools\Common7\Tools\VsDevCmd.bat','C:\BuildTools\Common7\Tools\VsDevCmd.bat','C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat','C:\VS\BuildTools\Common7\Tools\VsDevCmd.bat') | Where-Object {Test-Path -LiteralPath $_} | Select-Object -First 1
 if(-not $devCmd){throw 'Missing Visual Studio build environment'}
 $lines=& $env:ComSpec /d /s /c ('""'+$devCmd+'" -arch=x64 -host_arch=x64 >nul && set"')
 if($LASTEXITCODE -ne 0){throw 'VS environment initialization failed'}
 foreach($line in $lines){if($line -match '^([^=]+)=(.*)$'){[Environment]::SetEnvironmentVariable($matches[1],$matches[2],'Process')}}
 $state.vs_dev_command=$devCmd;$state.rustc=(& rustc --version | Out-String).Trim();$state.dotnet=(& dotnet --version | Out-String).Trim();Save-State
 $previousPath=Join-Path $root '.blueice-validation\acceptance-20261012-winui-mta\acceptance-result.json'
 $previous=Get-Content -LiteralPath $previousPath -Encoding UTF8 -Raw|ConvertFrom-Json
 if($previous.parent_commit -ne $state.parent_commit -or $previous.status -ne 'stopped' -or $previous.current -ne 'winui-desktop'){throw 'Unexpected previous acceptance source/state'}
 $reused=@()
 foreach($name in @('rust-fmt','rust-clippy','rust-build','rust-test','winui-build','winui-protocol')){
  $step=@($previous.steps|Where-Object name -eq $name)
  if($step.Count -ne 1 -or $step[0].exit_code -ne 0){throw ('Missing completed Rust gate: '+$name)}
  if((Get-FileHash -LiteralPath $step[0].log -Algorithm SHA256).Hash.ToLowerInvariant() -ne $step[0].log_sha256){throw ('Changed previous gate log: '+$name)}
  $reused+=@($step[0])
 }
 if($reused[3].rust_counts.passed -ne 5670 -or $reused[3].rust_counts.failed -ne 0 -or $reused[3].rust_counts.groups -ne 495){throw 'Unexpected full Rust test result'}
 $state.reused_rust_acceptance=[ordered]@{source=$previousPath;sha256=(Get-FileHash -LiteralPath $previousPath -Algorithm SHA256).Hash.ToLowerInvariant();steps=$reused;reason='Same frozen product source; completed Rust/build/protocol logs verified before retrying all seven desktop cases with the same corrected owned-window close driver and an MTA desktop client, direct phase traces and plain-string JSON publication'}
 $state.steps=$reused;Save-State
 $explorer=@(Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" | Where-Object {$_.SessionId -gt 0})
 if($explorer.Count -ne 1 -or $explorer[0].SessionId -ne 1){throw 'Unexpected logged in desktop'}
 $owner=Invoke-CimMethod -InputObject $explorer[0] -MethodName GetOwner
 if($owner.ReturnValue -ne 0 -or $owner.User -ne 'ephoton'){throw 'Unexpected desktop owner'}
 $taskName='BlueIce-Followup-20261012-'+[Guid]::NewGuid().ToString('N')
 $uiScript=Join-Path $meta 'winui-desktop15.ps1'
 $storageWrapper=Join-Path $root 'scripts\test-with-windows-storage.ps1'
 $action=New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument ('-NoProfile -ExecutionPolicy Bypass -File "'+$storageWrapper+'" -Workspace "'+$root+'" powershell.exe -MTA -NoProfile -ExecutionPolicy Bypass -File "'+$uiScript+'"') -WorkingDirectory $root
 $principal=New-ScheduledTaskPrincipal -UserId ($owner.Domain+'\'+$owner.User) -LogonType Interactive -RunLevel Limited
 $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
 Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings | Out-Null
 $state.current='winui-desktop';$state.ui_task=$taskName;$state.desktop_session=1;Save-State
 Start-ScheduledTask -TaskName $taskName
 $deadline=(Get-Date).AddMinutes(11);$uiResult=Join-Path $meta 'winui-desktop-result.json'
 while(-not (Test-Path -LiteralPath $uiResult)){
  if((Get-Date) -gt $deadline){throw 'Owned UI task exceeded its execution limit'}
  $state.disk=Check-Budget;Save-State;Start-Sleep -Seconds 2
 }
 while((Get-ScheduledTask -TaskName $taskName).State -eq 'Running'){Start-Sleep -Seconds 1}
 $ui=Get-Content -LiteralPath $uiResult -Encoding UTF8 -Raw | ConvertFrom-Json
 $info=Get-ScheduledTaskInfo -TaskName $taskName
 $state.ui=$ui;$state.ui_task_exit=$info.LastTaskResult;Save-State
 if($ui.exit_code -ne 0 -or $info.LastTaskResult -ne 0 -or $ui.pass_count -ne 7 -or $ui.session -ne 1){throw 'WinUI desktop test failed'}
 Verify-Source
 $env:RUSTUP_TOOLCHAIN=$originalToolchain
 $state.toolchains_after=(& rustup toolchain list | Out-String)
 if($state.toolchains_after -ne $beforeToolchains){throw 'Installed/default toolchains changed'}
 $state.execution_policy_scopes_after=@(Get-ExecutionPolicy -List | Where-Object {$_.Scope -ne 'Process'} | ForEach-Object {[ordered]@{scope=$_.Scope.ToString();policy=$_.ExecutionPolicy.ToString()}})
 if(($state.execution_policy_scopes_before|ConvertTo-Json -Compress) -ne ($state.execution_policy_scopes_after|ConvertTo-Json -Compress)){throw 'Persistent execution policies changed'}
 $state.source_unchanged=$true;$state.status='windows-acceptance-passed'
} catch {$state.status='stopped';$state.error=$_.Exception.Message}
finally {
 if($taskName){
  $task=Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
  if($task -and $task.State -eq 'Running'){Stop-ScheduledTask -TaskName $taskName}
  if($task){Unregister-ScheduledTask -TaskName $taskName -Confirm:$false}
  $state.ui_task_removed=(-not [bool](Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue))
 }
 $ownedUI=@(Get-CimInstance Win32_Process | Where-Object {$_.Name -in @('BlueIce.WinUI.exe','blueice-core.exe') -and $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase) -and $_.CreationDate.ToUniversalTime() -ge [DateTime]::Parse($state.started).ToUniversalTime()})
 $state.ui_cleanup_candidates=@($ownedUI | Select-Object ProcessId,ParentProcessId,CreationDate,ExecutablePath)
 if($state.status -eq 'windows-acceptance-passed' -and $ownedUI.Count){$state.status='stopped';$state.error='Unexpected owned UI process after successful close'}
 foreach($process in $ownedUI){
  $current=Get-CimInstance Win32_Process -Filter ("ProcessId="+$process.ProcessId) -ErrorAction SilentlyContinue
  if($current -and $current.CreationDate -eq $process.CreationDate -and $current.ExecutablePath -eq $process.ExecutablePath){Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue}
 }
 $state.remaining_owned_ui=@(Get-CimInstance Win32_Process | Where-Object {$_.Name -in @('BlueIce.WinUI.exe','blueice-core.exe') -and $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)} | Select-Object ProcessId,CreationDate,ExecutablePath)
 $state.finished=[DateTime]::UtcNow.ToString('o');Save-State
}
$state|ConvertTo-Json -Depth 12 -Compress
if($state.status -ne 'windows-acceptance-passed'){exit 1}
