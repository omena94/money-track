param([string]$Origin = 'http://localhost:3000', [string]$AdminEmail = '')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'PostgresAdmin.ps1')
Invoke-PostgresAdmin -Action {
$projectRoot = Split-Path $PSScriptRoot -Parent
$localPath = Join-Path $projectRoot '.local'
New-Item -ItemType Directory -Path $localPath -Force | Out-Null
$psql = 'C:\Program Files\PostgreSQL\18\bin\psql.exe'
$env:PGCLIENTENCODING = 'UTF8'
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
function Invoke-Database([string]$Database, [string]$Sql) {
    $Sql | & $psql -X -q -w -d $Database -v ON_ERROR_STOP=1
    if ($LASTEXITCODE -ne 0) { throw 'Database initialization failed.' }
}
function New-RandomSecret {
    $buffer = New-Object byte[] 32
    $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($buffer); return ([BitConverter]::ToString($buffer)).Replace('-', '').ToLowerInvariant() } finally { $rng.Dispose() }
}
$secretFile = Join-Path $localPath 'secrets.dpapi'
if (-not (Test-Path -LiteralPath $secretFile)) {
    $appSecret = New-RandomSecret
    $authSecret = New-RandomSecret
    $config = @{
        DATABASE_URL = "postgresql://money_app:${appSecret}@127.0.0.1:5432/money_track"
        AUTH_DATABASE_URL = "postgresql://money_auth:${authSecret}@127.0.0.1:5432/money_track"
        APP_ORIGIN = $Origin
        ADMIN_EMAIL = $AdminEmail
        REDIS_URL = 'redis://127.0.0.1:6379/'
        GOOGLE_CLIENT_ID = ''
        GOOGLE_CLIENT_SECRET = ''
        CONFIG_ENCRYPTION_KEY = (New-RandomSecret)
    }
    $config | ConvertTo-Json | ConvertTo-SecureString -AsPlainText -Force | ConvertFrom-SecureString | Set-Content -LiteralPath $secretFile -Encoding ASCII
}
. (Join-Path $PSScriptRoot 'Import-Environment.ps1')
if (-not $env:BACKUP_DATABASE_URL) {
    $backupSecret = New-RandomSecret
    $protectedSettings = (Get-Content -LiteralPath $secretFile -Raw).Trim() | ConvertTo-SecureString
    $settings = [System.Net.NetworkCredential]::new('', $protectedSettings).Password | ConvertFrom-Json
    $settings | Add-Member -NotePropertyName BACKUP_DATABASE_URL -NotePropertyValue "postgresql://money_backup:${backupSecret}@127.0.0.1:5432/money_track" -Force
    $settings | ConvertTo-Json | ConvertTo-SecureString -AsPlainText -Force | ConvertFrom-SecureString | Set-Content -LiteralPath $secretFile -Encoding ASCII
    $env:BACKUP_DATABASE_URL = $settings.BACKUP_DATABASE_URL
    $settings=$null; $protectedSettings=$null
}
$backupSecret = ([uri]$env:BACKUP_DATABASE_URL).UserInfo.Split(':')[1]
$appSecret = ([uri]$env:DATABASE_URL).UserInfo.Split(':')[1]
$authSecret = ([uri]$env:AUTH_DATABASE_URL).UserInfo.Split(':')[1]
# Values below are cryptographically generated hex, never user-supplied SQL.
$roleSql = @'
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='money_app') THEN CREATE ROLE money_app LOGIN; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='money_auth') THEN CREATE ROLE money_auth LOGIN; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='money_backup') THEN CREATE ROLE money_backup LOGIN; END IF;
END $$;
'@
Invoke-Database 'postgres' ($roleSql + "`nALTER ROLE money_app WITH NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS PASSWORD '$appSecret';`nALTER ROLE money_auth WITH NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS PASSWORD '$authSecret';")
Invoke-Database 'postgres' "ALTER ROLE money_backup WITH NOSUPERUSER NOCREATEDB NOCREATEROLE BYPASSRLS CONNECTION LIMIT 2 PASSWORD '$backupSecret'; ALTER ROLE money_backup SET default_transaction_read_only=on;"
$exists = & $psql -X -q -t -A -w -d postgres -c "SELECT 1 FROM pg_database WHERE datname='money_track'"
if ($LASTEXITCODE -ne 0) { throw 'Database check failed.' }
if ($exists -ne '1') { Invoke-Database 'postgres' 'CREATE DATABASE money_track;' }
Invoke-Database 'money_track' 'REVOKE CREATE ON SCHEMA public FROM PUBLIC; CREATE TABLE IF NOT EXISTS money_migrations(name text PRIMARY KEY, applied_at timestamptz NOT NULL DEFAULT now());'
foreach ($migration in (Get-ChildItem (Join-Path $projectRoot 'apps\api\migrations\*.sql') | Sort-Object Name)) {
    $name = $migration.Name
    $applied = & $psql -X -q -t -A -w -d money_track -c "SELECT 1 FROM money_migrations WHERE name='$name'"
    if ($applied -ne '1') {
        $sql = Get-Content -LiteralPath $migration.FullName -Raw -Encoding UTF8
        Invoke-Database 'money_track' ("BEGIN;`n" + $sql + "`nINSERT INTO money_migrations(name) VALUES('$name');`nCOMMIT;")
        Write-Host "Applied $name"
    }
}
Invoke-Database 'money_track' @'
GRANT CONNECT ON DATABASE money_track TO money_app,money_auth;
GRANT CONNECT ON DATABASE money_track TO money_backup;
GRANT USAGE ON SCHEMA public TO money_backup;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO money_backup;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT SELECT ON TABLES TO money_backup;
GRANT SELECT ON ALL SEQUENCES IN SCHEMA public TO money_backup;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT SELECT ON SEQUENCES TO money_backup;
GRANT USAGE ON SCHEMA public TO money_app,money_auth;
GRANT SELECT,INSERT,UPDATE,DELETE ON users,sessions,oauth_attempts TO money_auth;
GRANT SELECT,INSERT ON audit_events TO money_auth;
GRANT SELECT,INSERT,UPDATE,DELETE ON investment_positions,investment_trades,market_quotes,market_updates TO money_app;
GRANT SELECT,INSERT,UPDATE,DELETE ON account_summaries TO money_app;
GRANT SELECT,INSERT,UPDATE,DELETE ON source_batches,source_records,source_balance_history,investment_value_snapshots TO money_app;
GRANT SELECT,INSERT,UPDATE,DELETE ON accounts,transactions,shares,budgets,goals,connections,sync_jobs,notifications,recurring_entries TO money_app;
GRANT SELECT,INSERT,DELETE ON household_viewers TO money_app;
GRANT SELECT,INSERT,DELETE ON sync_job_events TO money_app;
GRANT SELECT,INSERT,UPDATE,DELETE ON category_rules,source_tx_labels TO money_app;
GRANT SELECT ON category_keywords TO money_app;
REVOKE UPDATE ON shares FROM money_app;
GRANT SELECT ON shares TO money_auth;
GRANT UPDATE(accepted_by,accepted_at) ON shares TO money_auth;
DROP POLICY IF EXISTS share_accept ON shares;
CREATE POLICY share_accept ON shares FOR UPDATE TO money_auth
 USING(recipient_email=current_email() AND owner_id<>current_owner())
 WITH CHECK(recipient_email=current_email() AND accepted_by=current_owner());
'@
$appSecret = $null; $authSecret = $null
Write-Host 'Local database and DPAPI runtime settings are ready. OAuth credentials have not been created by this script.'
}
