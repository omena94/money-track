param([switch]$Remove)
$ErrorActionPreference='Stop'
$taskUser=[Security.Principal.WindowsIdentity]::GetCurrent().Name
$appTask='Money Track - App'
$backupTask='Money Track - Backup'
if($Remove){
 foreach($name in @($appTask,$backupTask)){Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue}
 Write-Host 'Money Track scheduled tasks removed.'
 return
}
$principal=New-ScheduledTaskPrincipal -UserId $taskUser -LogonType Interactive -RunLevel Limited
$powershell=(Get-Command powershell.exe).Source
$appScript=Join-Path $PSScriptRoot 'Start-IfNeeded.ps1'
$backupScript=Join-Path $PSScriptRoot 'Backup.ps1'
$appAction=New-ScheduledTaskAction -Execute $powershell -Argument "-NoProfile -WindowStyle Hidden -File `"$appScript`""
$backupAction=New-ScheduledTaskAction -Execute $powershell -Argument "-NoProfile -WindowStyle Hidden -File `"$backupScript`""
$appTrigger=New-ScheduledTaskTrigger -AtLogOn -User $taskUser
$backupTrigger=New-ScheduledTaskTrigger -Daily -At '03:00'
$appSettings=New-ScheduledTaskSettingsSet -StartWhenAvailable -MultipleInstances IgnoreNew -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
$backupSettings=New-ScheduledTaskSettingsSet -StartWhenAvailable -MultipleInstances IgnoreNew -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 15)
Register-ScheduledTask -TaskName $appTask -Action $appAction -Trigger $appTrigger -Principal $principal -Settings $appSettings -Description 'Start this local Money Track website and HTTP synchronization after Windows login.' -Force | Out-Null
Register-ScheduledTask -TaskName $backupTask -Action $backupAction -Trigger $backupTrigger -Principal $principal -Settings $backupSettings -Description 'Daily encrypted local PostgreSQL backup; 30 day retention; requires this Windows user session.' -Force | Out-Null
Get-ScheduledTask -TaskName $appTask,$backupTask | Select-Object TaskName,State
