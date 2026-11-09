param([string]$Account = 'ArgusLab')
$ErrorActionPreference = 'Stop'
if ((Get-ItemProperty 'HKLM:\HARDWARE\DESCRIPTION\System\BIOS').SystemManufacturer -notlike '*QEMU*') { throw 'This setup is restricted to the dedicated QEMU VM.' }
$root = Split-Path -Parent $PSScriptRoot
$node = Join-Path $root 'node.exe'
$agent = Join-Path $PSScriptRoot 'vm-desktop-agent.mjs'
if (!(Test-Path -LiteralPath $node) -or !(Test-Path -LiteralPath $agent)) { throw 'Copy the desktop agent and Node runtime first.' }
$accountInfo = Get-LocalUser -Name $Account
$command = "& '$node' '$agent' 1>> '$root\target\desktop-agent.stdout.log' 2>> '$root\target\desktop-agent.stderr.log'; exit `$LASTEXITCODE"
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
$action = New-ScheduledTaskAction -Execute "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument ('-NoProfile -WindowStyle Hidden -EncodedCommand ' + $encoded) -WorkingDirectory $root
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $accountInfo.SID.Value
$principal = New-ScheduledTaskPrincipal -UserId $accountInfo.SID.Value -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName 'Argus VM Desktop Agent' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName 'Argus VM Desktop Agent'
Get-ScheduledTask -TaskName 'Argus VM Desktop Agent' | Select-Object TaskName, State
