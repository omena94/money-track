param(
    [string]$HostName = 'localhost',
    [ValidateRange(1, 65535)][int]$Port = 5432,
    [string]$Username = 'postgres'
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'PostgresAdmin.ps1')
Save-PostgresAdmin -HostName $HostName -Port $Port -Username $Username
