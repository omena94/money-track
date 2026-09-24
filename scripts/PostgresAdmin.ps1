# PostgreSQL maintenance credentials stay separate from application runtime settings.
function Invoke-PostgresAdmin {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [hashtable]$Settings
    )
    $pgAdminFile = Join-Path (Split-Path $PSScriptRoot -Parent) '.local\postgres-admin.dpapi'
    $pgAdminKeys = @('PGHOST', 'PGPORT', 'PGUSER', 'PGPASSWORD', 'PGCLIENTENCODING', 'PGCONNECT_TIMEOUT')
    $pgAdminPrevious = @{}
    $pgAdminProtected = $null
    foreach ($pgAdminKey in $pgAdminKeys) {
        $pgAdminPrevious[$pgAdminKey] = [Environment]::GetEnvironmentVariable($pgAdminKey, 'Process')
    }
    try {
        if (-not $Settings) {
            if ($env:PGPASSWORD) {
                $Settings = @{
                    HostName = $(if ($env:PGHOST) { $env:PGHOST } else { 'localhost' })
                    Port = $(if ($env:PGPORT) { $env:PGPORT } else { '5432' })
                    Username = $(if ($env:PGUSER) { $env:PGUSER } else { 'postgres' })
                    Password = $env:PGPASSWORD
                }
            } else {
                if (-not (Test-Path -LiteralPath $pgAdminFile)) {
                    Save-PostgresAdmin
                }
                try {
                    $pgAdminProtected = (Get-Content -LiteralPath $pgAdminFile -Raw -ErrorAction Stop).Trim() | ConvertTo-SecureString -ErrorAction Stop
                    $pgAdminPlain = [System.Net.NetworkCredential]::new('', $pgAdminProtected).Password
                    $pgAdminStored = $pgAdminPlain | ConvertFrom-Json -ErrorAction Stop
                    if ($pgAdminStored.Version -ne 1) { throw 'Unsupported credential format' }
                    $Settings = @{
                        HostName = $pgAdminStored.HostName
                        Port = $pgAdminStored.Port
                        Username = $pgAdminStored.Username
                        Password = $pgAdminStored.Password
                    }
                } catch {
                    throw 'PostgreSQL credential store could not be read. Run scripts/Set-PostgresAdmin.ps1 with this Windows user to replace it.'
                }
            }
        }
        if ([string]::IsNullOrWhiteSpace($Settings.HostName) -or
            [string]::IsNullOrWhiteSpace($Settings.Username) -or
            [string]::IsNullOrEmpty($Settings.Password) -or
            [string]$Settings.Port -notmatch '^\d{1,5}$' -or
            [int]$Settings.Port -lt 1 -or [int]$Settings.Port -gt 65535) {
            throw 'PostgreSQL maintenance connection settings are invalid.'
        }
        $env:PGHOST = [string]$Settings.HostName
        $env:PGPORT = [string]$Settings.Port
        $env:PGUSER = [string]$Settings.Username
        $env:PGPASSWORD = [string]$Settings.Password
        $env:PGCLIENTENCODING = 'UTF8'
        $env:PGCONNECT_TIMEOUT = '10'
        & $Action
    } finally {
        foreach ($pgAdminKey in $pgAdminKeys) {
            [Environment]::SetEnvironmentVariable($pgAdminKey, $pgAdminPrevious[$pgAdminKey], 'Process')
        }
        $Settings = $null
        $pgAdminPrevious = $null
        $pgAdminPlain = $null
        $pgAdminStored = $null
        if ($pgAdminProtected) { $pgAdminProtected.Dispose() }
    }
}

function Save-PostgresAdmin {
    param(
        [string]$HostName = 'localhost',
        [ValidateRange(1, 65535)][int]$Port = 5432,
        [string]$Username = 'postgres',
        [System.Security.SecureString]$Password
    )
    if (-not $Password) {
        $Password = Read-Host 'PostgreSQL admin password (hidden; saved with Windows DPAPI)' -AsSecureString
    }
    try {
        $pgAdminSettings = @{
            Version = 1
            HostName = $HostName
            Port = $Port
            Username = $Username
            Password = [System.Net.NetworkCredential]::new('', $Password).Password
        }
        Invoke-PostgresAdmin -Settings $pgAdminSettings -Action {
            $pgAdminPsql = 'C:\Program Files\PostgreSQL\18\bin\psql.exe'
            $pgAdminCheck = & $pgAdminPsql -X -q -t -A -w -d postgres -c 'SELECT 1' 2>$null
            if ($LASTEXITCODE -ne 0 -or "$pgAdminCheck".Trim() -ne '1') {
                throw 'PostgreSQL authentication check failed; saved credentials were not changed.'
            }
        }
        $pgAdminDirectory = Join-Path (Split-Path $PSScriptRoot -Parent) '.local'
        New-Item -ItemType Directory -Path $pgAdminDirectory -Force | Out-Null
        $pgAdminCipher = $pgAdminSettings | ConvertTo-Json -Compress | ConvertTo-SecureString -AsPlainText -Force | ConvertFrom-SecureString
        $pgAdminCipher | Set-Content -LiteralPath (Join-Path $pgAdminDirectory 'postgres-admin.dpapi') -Encoding ASCII -ErrorAction Stop
        Write-Host 'PostgreSQL maintenance credentials verified and saved with Windows DPAPI.'
    } finally {
        $pgAdminSettings = $null
        $pgAdminCipher = $null
    }
}
