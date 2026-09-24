param([string]$Origin = $env:APP_ORIGIN)
$ErrorActionPreference='Continue'
$projectRoot=Split-Path $PSScriptRoot -Parent
Get-Service -Name postgresql-x64-18,Redis,Cloudflared | Select-Object Name,Status | Format-Table -AutoSize
$addresses=@('http://127.0.0.1:3001/api/v1/health','http://localhost:3000/api/v1/health')
if($Origin){$addresses+=($Origin.TrimEnd('/')+'/api/v1/health')}
$checks=foreach($address in ($addresses | Select-Object -Unique)){
 try{$h=Invoke-RestMethod -Uri $address -TimeoutSec 10;[pscustomobject]@{url=$address;status=$h.status;database=$h.database;redis=$h.redis;google=$h.google_configured}}
 catch{[pscustomobject]@{url=$address;status='unreachable';database=$false;redis=$false;google=$false}}
}
$checks | Format-Table -AutoSize
Get-ScheduledTask -TaskName 'Money Track - App','Money Track - Backup' -ErrorAction SilentlyContinue | ForEach-Object{
 $info=$_ | Get-ScheduledTaskInfo
 [pscustomobject]@{task=$_.TaskName;state=$_.State;last_result=$info.LastTaskResult;next_run=$info.NextRunTime}
} | Format-Table -AutoSize
$latest=Get-ChildItem -LiteralPath (Join-Path $projectRoot '.local\backups') -Filter '*.mtbackup' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if($latest){Write-Host "Latest encrypted backup: $($latest.Name) at $($latest.LastWriteTime)"}
