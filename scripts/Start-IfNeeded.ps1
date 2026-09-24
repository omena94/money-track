$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$record=Join-Path $projectRoot '.local\processes.json'
if(Test-Path -LiteralPath $record){
 $entries=@(Get-Content -LiteralPath $record -Raw | ConvertFrom-Json)
 $alive=@($entries | Where-Object {
  $p=Get-Process -Id $_.id -ErrorAction SilentlyContinue
  $p -and $p.ProcessName -eq $_.name -and $p.StartTime.ToUniversalTime().ToString('o') -eq $_.started
 })
 if($alive.Count -eq 2){
  try {if((Invoke-RestMethod 'http://127.0.0.1:3000/api/v1/health' -TimeoutSec 5).status -eq 'ok'){return}}catch{}
 }
 & (Join-Path $PSScriptRoot 'Stop.ps1')
}
& (Join-Path $PSScriptRoot 'Start.ps1') -Production
