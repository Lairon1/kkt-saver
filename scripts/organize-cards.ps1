#requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet('Analyze','Apply','Verify')][string]$Mode = 'Analyze',
    [string]$Root = (Join-Path ([Environment]::GetFolderPath('Desktop')) 'карточки регистрации'),
    [string]$ReportDir = (Join-Path ([Environment]::GetFolderPath('Desktop')) ('kkt-sort-report-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))),
    [string]$Plan,
    [switch]$BackupConfirmed
)
$ErrorActionPreference = 'Stop'
$OutputEncoding = [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$utility = Join-Path $PSScriptRoot 'kkt-organizer.exe'
if (-not (Test-Path -LiteralPath $utility -PathType Leaf)) {
    $utility = Join-Path (Split-Path $PSScriptRoot -Parent) 'dist\kkt-organizer.exe'
}
if (-not (Test-Path -LiteralPath $utility -PathType Leaf)) { throw 'Рядом со скриптом нужен kkt-organizer.exe из комплекта программы.' }
switch ($Mode) {
    'Analyze' {
        & $utility analyze --root $Root --report-dir $ReportDir
    }
    'Apply' {
        if (-not $Plan) { throw 'Укажите -Plan <полный путь к plan.json из отчёта>.' }
        if (-not $BackupConfirmed) { throw 'Сначала сделайте резервную копию. После этого добавьте -BackupConfirmed.' }
        & $utility apply --plan $Plan --backup-confirmed
    }
    'Verify' {
        if (-not $Plan) { throw 'Укажите -Plan <полный путь к plan.json>.' }
        & $utility verify --plan $Plan
    }
}
if ($LASTEXITCODE -ne 0) { throw "Сортировщик завершился с кодом $LASTEXITCODE. Исходники незавершённых операций сохранены; подробности выше." }
