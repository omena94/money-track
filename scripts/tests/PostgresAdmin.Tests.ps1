$ErrorActionPreference = 'Stop'
$testProjectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$testLocalRoot = Join-Path $testProjectRoot '.local'
$testRoot = Join-Path $testLocalRoot ('pgadmin-tests-' + [guid]::NewGuid().ToString('N'))
$testKeys = @('PGHOST', 'PGPORT', 'PGUSER', 'PGPASSWORD', 'PGCLIENTENCODING', 'PGCONNECT_TIMEOUT')
$testOriginal = @{}
foreach ($key in $testKeys) { $testOriginal[$key] = [Environment]::GetEnvironmentVariable($key, 'Process') }
function Assert-Condition([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Read-Host { throw 'Unexpected credential prompt in cached or explicit authentication' }
try {
    New-Item -ItemType Directory -Path (Join-Path $testRoot 'scripts'), (Join-Path $testRoot '.local') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $testProjectRoot 'scripts/PostgresAdmin.ps1') -Destination (Join-Path $testRoot 'scripts/PostgresAdmin.ps1')
    . (Join-Path $testRoot 'scripts/PostgresAdmin.ps1')
    $testPassword = [guid]::NewGuid().ToString('N')
    $testFile = Join-Path $testRoot '.local/postgres-admin.dpapi'
    @{ Version=1; HostName='localhost'; Port=5432; Username='fixture_admin'; Password=$testPassword } |
        ConvertTo-Json | ConvertTo-SecureString -AsPlainText -Force | ConvertFrom-SecureString |
        Set-Content -LiteralPath $testFile -Encoding ASCII
    Assert-Condition (-not (Get-Content -LiteralPath $testFile -Raw).Contains($testPassword)) 'Credential file contains plaintext'
    foreach ($key in $testKeys) { [Environment]::SetEnvironmentVariable($key, $null, 'Process') }
    $env:PGHOST = 'prior-host'
    Invoke-PostgresAdmin -Action {
        Assert-Condition ($env:PGPASSWORD -ceq $testPassword) 'Cached password was not loaded'
        Assert-Condition ($env:PGHOST -eq 'localhost' -and $env:PGUSER -eq 'fixture_admin' -and $env:PGPORT -eq '5432') 'Cached connection settings were not loaded'
    }
    Assert-Condition ($env:PGHOST -eq 'prior-host' -and -not $env:PGPASSWORD -and -not $env:PGUSER -and -not $env:PGPORT) 'Environment was not restored after success'
    try {
        Invoke-PostgresAdmin -Action { throw 'expected-action-error' }
        throw 'Action error was swallowed'
    } catch {
        Assert-Condition ($_.Exception.Message -eq 'expected-action-error') 'Unexpected error result'
    }
    Assert-Condition ($env:PGHOST -eq 'prior-host' -and -not $env:PGPASSWORD) 'Environment was not restored after failure'
    'invalid-cipher' | Set-Content -LiteralPath $testFile -Encoding ASCII
    $env:PGPASSWORD = 'fixture-explicit'
    Invoke-PostgresAdmin -Action {
        Assert-Condition ($env:PGPASSWORD -eq 'fixture-explicit' -and $env:PGHOST -eq 'prior-host') 'Explicit environment should override cached credentials'
    }
    Assert-Condition ($env:PGPASSWORD -eq 'fixture-explicit') 'Existing caller password was not restored'
    Remove-Item Env:PGPASSWORD
    try {
        Invoke-PostgresAdmin -Action { throw 'Unexpected action execution' }
        throw 'Invalid store was accepted'
    } catch {
        Assert-Condition ($_.Exception.Message.StartsWith('PostgreSQL credential store could not be read.')) 'Invalid store did not produce a sanitized error'
    }
    Assert-Condition (-not $env:PGPASSWORD -and $env:PGHOST -eq 'prior-host') 'Failed decryption changed the environment'
    Write-Host 'PASS: DPAPI cache, no repeated prompt, explicit override, error handling and environment restoration.'
} finally {
    foreach ($key in $testKeys) { [Environment]::SetEnvironmentVariable($key, $testOriginal[$key], 'Process') }
    if (Test-Path -LiteralPath $testRoot) {
        $resolvedTestRoot = (Resolve-Path -LiteralPath $testRoot).Path
        if ((Split-Path $resolvedTestRoot -Parent) -ne (Resolve-Path -LiteralPath $testLocalRoot).Path -or
            (Split-Path $resolvedTestRoot -Leaf) -notlike 'pgadmin-tests-*') { throw 'Unexpected test cleanup path' }
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
    }
}
