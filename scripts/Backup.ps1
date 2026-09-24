$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
& (Join-Path $projectRoot '.venv\Scripts\python.exe') (Join-Path $PSScriptRoot 'backup.py') create
if($LASTEXITCODE -ne 0){throw 'Encrypted backup failed'}
