param([string]$Archive='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'PostgresAdmin.ps1')
Invoke-PostgresAdmin -Action {
 $arguments=@((Join-Path $PSScriptRoot 'backup.py'),'restore-check')
 if($Archive){$arguments+=@('--archive',$Archive)}
 & (Join-Path $projectRoot '.venv\Scripts\python.exe') @arguments
 if($LASTEXITCODE -ne 0){throw 'Restore verification failed'}
}
