$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'Import-Environment.ps1')
if (Get-NetTCPConnection -State Listen -LocalPort 3001 -ErrorAction SilentlyContinue) { throw 'Stop Money Track before database fixtures: scripts/Stop.ps1' }
Push-Location $projectRoot
try {
    & cargo test
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
    & cargo test --test member_activity -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Member activity tests failed' }
    & cargo test --test isolation -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Database isolation tests failed' }
    & cargo test --test financial_sources -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Financial source tests failed' }
    & cargo test --test bank_calendar -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Calendar and bank login guard tests failed' }
    & cargo test --test household -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Household tests failed' }
    & cargo test --test categories -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Category tests failed' }
    & cargo test --test subscriptions -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Subscription tests failed' }
    & npm run typecheck
    if ($LASTEXITCODE -ne 0) { throw 'Typecheck failed' }
} finally { Pop-Location }
