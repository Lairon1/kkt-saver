param([switch]$DebugBuild,[switch]$SkipTests)
$ErrorActionPreference='Stop'
$project=Split-Path $PSScriptRoot -Parent
$localCargo=Join-Path $project '.tools\cargo\bin'
if(Test-Path -LiteralPath (Join-Path $localCargo 'cargo.exe')) {
    $env:CARGO_HOME=Join-Path $project '.tools\cargo'
    $env:RUSTUP_HOME=Join-Path $project '.tools\rustup'
    $env:PATH=$localCargo+';'+$env:PATH
}
Push-Location $project
try {
    if(-not $SkipTests){ & cargo test --locked; if($LASTEXITCODE -ne 0){throw 'Tests failed'} }
    $cargoArgs=@('build','--locked')
    if(-not $DebugBuild){$cargoArgs+='--release'}
    & cargo @cargoArgs
    if($LASTEXITCODE -ne 0){throw 'Build failed'}
    $profile=if($DebugBuild){'debug'}else{'release'}
    New-Item -ItemType Directory -Path 'dist' -Force | Out-Null
    Copy-Item -LiteralPath "target\$profile\kkt-saver.exe" -Destination 'dist\kkt-saver.exe'
    Copy-Item -LiteralPath "target\$profile\kkt-organizer.exe" -Destination 'dist\kkt-organizer.exe'
    # Windows PowerShell 5.1 needs a UTF-8 BOM for Cyrillic script literals.
    $launcher = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'organize-cards.ps1'), [Text.Encoding]::UTF8)
    [IO.File]::WriteAllText((Join-Path $project 'dist\organize-cards.ps1'), $launcher, (New-Object Text.UTF8Encoding($true)))
    Copy-Item -LiteralPath 'README.md','THIRD-PARTY-NOTICES.md','LICENSE' -Destination 'dist'
    if(Test-Path 'assets\licenses'){
        New-Item -ItemType Directory -Path 'dist\licenses' -Force | Out-Null
        Copy-Item -Path 'assets\licenses\*' -Destination 'dist\licenses' -Recurse -Force
    }
    Copy-Item -LiteralPath 'assets\LICENSE' -Destination 'dist\PDFium-LICENSE.txt'
    Get-Item 'dist\kkt-saver.exe'
}finally{Pop-Location}
