#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ExpectedTag,
    [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$manifest = [IO.File]::ReadAllText((Join-Path $project 'Cargo.toml'))
$versionMatch = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"\s*$')
if (-not $versionMatch.Success) { throw 'Cannot read the package version from Cargo.toml.' }
$version = $versionMatch.Groups[1].Value
if ($ExpectedTag -and $ExpectedTag -cne "v$version") {
    throw "Release tag '$ExpectedTag' does not match Cargo.toml version 'v$version'."
}
if ($ValidateOnly) {
    Write-Host "Package version: $version"
    return
}

$dist = Join-Path $project 'dist'
$artifactDirectory = Join-Path $project 'artifacts'
$required = @('kkt-saver.exe', 'kkt-organizer.exe', 'organize-cards.ps1', 'README.md', 'LICENSE', 'THIRD-PARTY-NOTICES.md', 'PDFium-LICENSE.txt')
foreach ($name in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $dist $name) -PathType Leaf)) {
        throw "Missing dist/$name. Run scripts/build.ps1 first."
    }
}
New-Item -ItemType Directory -Path $artifactDirectory -Force | Out-Null
$files = @('kkt-saver.exe', 'kkt-organizer.exe', "kkt-saver-$version-windows-x64.zip")
Copy-Item -LiteralPath (Join-Path $dist 'kkt-saver.exe') -Destination (Join-Path $artifactDirectory $files[0])
Copy-Item -LiteralPath (Join-Path $dist 'kkt-organizer.exe') -Destination (Join-Path $artifactDirectory $files[1])
Compress-Archive -Path (Join-Path $dist '*') -DestinationPath (Join-Path $artifactDirectory $files[2]) -CompressionLevel Optimal -Force

$checksums = foreach ($name in $files) {
    $hash = (Get-FileHash -LiteralPath (Join-Path $artifactDirectory $name) -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $name"
}
# LF and no BOM keep the manifest compatible with sha256sum on the release runner.
[IO.File]::WriteAllText((Join-Path $artifactDirectory 'SHA256SUMS.txt'), ($checksums -join "`n") + "`n", (New-Object Text.UTF8Encoding($false)))
Get-ChildItem -LiteralPath $artifactDirectory -File | Select-Object Name, Length
