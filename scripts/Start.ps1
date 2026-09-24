param([switch]$Production)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'Import-Environment.ps1')
Remove-Item Env:BACKUP_DATABASE_URL -ErrorAction SilentlyContinue
foreach ($port in @(3000,3001)) { if (Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue) { throw "Port $port is already in use. Run Stop.ps1 for this project or inspect the existing listener." } }
$apiExe = Join-Path $projectRoot 'target\debug\money-api.exe'
if (-not (Test-Path -LiteralPath $apiExe)) { throw 'Build the API first: cargo build' }
$processes = @()
try {
    $api = Start-Process -FilePath $apiExe -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $projectRoot '.local\api.log') -RedirectStandardError (Join-Path $projectRoot '.local\api-error.log')
    $processes += @{id=$api.Id;name='money-api';started=$api.StartTime.ToUniversalTime().ToString('o')}
    foreach ($secretName in @('DATABASE_URL','AUTH_DATABASE_URL','GOOGLE_CLIENT_SECRET','CONFIG_ENCRYPTION_KEY','CLOUDFLARE_API_TOKEN','PGPASSWORD')) {
        [Environment]::SetEnvironmentVariable($secretName, $null, 'Process')
    }
    $verb = if ($Production) { 'start' } else { 'dev' }
    $web = Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('node_modules/next/dist/bin/next',$verb,'apps/web','--hostname','127.0.0.1','--port','3000') -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $projectRoot '.local\web.log') -RedirectStandardError (Join-Path $projectRoot '.local\web-error.log')
    $processes += @{id=$web.Id;name='node';started=$web.StartTime.ToUniversalTime().ToString('o')}
    $processes | ConvertTo-Json | Set-Content (Join-Path $projectRoot '.local\processes.json')
    Write-Host 'Money Track starting at http://localhost:3000. Run Diagnose.ps1 to verify readiness.'
} catch {
    foreach ($entry in $processes) { Stop-Process -Id $entry.id -ErrorAction SilentlyContinue }
    throw
}
