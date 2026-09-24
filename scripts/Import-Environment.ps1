$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$secretFile = Join-Path $projectRoot '.local\secrets.dpapi'
if (-not (Test-Path -LiteralPath $secretFile)) { throw 'Run scripts/Initialize.ps1 first.' }
$protected = (Get-Content -LiteralPath $secretFile -Raw).Trim() | ConvertTo-SecureString
$plain = [System.Net.NetworkCredential]::new('', $protected).Password
try {
    $config = $plain | ConvertFrom-Json
    foreach ($property in $config.PSObject.Properties) {
        [Environment]::SetEnvironmentVariable($property.Name, [string]$property.Value, 'Process')
    }
} finally { $plain = $null; $protected = $null; $config = $null }
$env:PATH = "$HOME\.cargo\bin;C:\Program Files\PostgreSQL\18\bin;" + $env:PATH
