param([string]$ClientId, [string]$Origin = '')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Import-Environment.ps1')
if (-not $Origin) { $Origin = $env:APP_ORIGIN }
if (-not $Origin) { $Origin = 'http://localhost:3000' }
$originUri = [uri]$Origin
if (-not $originUri.IsAbsoluteUri -or ($originUri.Scheme -ne 'https' -and -not ($originUri.Scheme -eq 'http' -and $originUri.Host -in @('localhost','127.0.0.1'))) -or $originUri.AbsolutePath -ne '/' -or $originUri.Query -or $originUri.Fragment -or $originUri.UserInfo) { throw 'Origin requires HTTPS or localhost, without credentials, path, query or fragment.' }
if (-not $ClientId) { $ClientId = Read-Host 'Google OAuth client ID' }
$secret = Read-Host 'Google OAuth client secret (hidden)' -AsSecureString
$secretFile = Join-Path (Split-Path $PSScriptRoot -Parent) '.local\secrets.dpapi'
$config = [System.Net.NetworkCredential]::new('', ((Get-Content -LiteralPath $secretFile -Raw).Trim() | ConvertTo-SecureString)).Password | ConvertFrom-Json
$config.GOOGLE_CLIENT_ID = $ClientId
$config.GOOGLE_CLIENT_SECRET = [System.Net.NetworkCredential]::new('', $secret).Password
$config.APP_ORIGIN = $Origin.TrimEnd('/')
$config | ConvertTo-Json | ConvertTo-SecureString -AsPlainText -Force | ConvertFrom-SecureString | Set-Content -LiteralPath $secretFile -Encoding ASCII
$config = $null; $secret = $null
Write-Host 'OAuth settings saved using Windows DPAPI. Restart the API to load them.'
