$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$pidFile = Join-Path $projectRoot '.local\processes.json'
if (-not (Test-Path -LiteralPath $pidFile)) { Write-Host 'No processes recorded for this project.'; return }
foreach ($entry in (Get-Content $pidFile -Raw | ConvertFrom-Json)) {
    $process = Get-Process -Id $entry.id -ErrorAction SilentlyContinue
    if ($process -and $process.ProcessName -eq $entry.name -and $process.StartTime.ToUniversalTime().ToString('o') -eq $entry.started) {
        # Only this project's recorded process and children, with start-time verification.
        & taskkill.exe /PID $entry.id /T /F | Out-Null
    }
}
Remove-Item -LiteralPath $pidFile
Write-Host 'Recorded Money Track processes stopped.'
