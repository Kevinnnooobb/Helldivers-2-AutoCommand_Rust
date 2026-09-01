# H2AC-RS release builder + GitHub uploader (PowerShell)
# Usage:
#   .\build-release.ps1                       # build portable zip and create/push GitHub release
#   .\build-release.ps1 -SkipPush -SkipRelease # local package only
param(
    [string]$Tag = "1.1.0",
    [string]$PreviousTag = "1.0",
    [string]$ReleaseName = "$Tag Release",
    [string]$Repo = "Kevinnnooobb/Helldivers-2-AutoCommand_Rust",
    [switch]$SkipTests,
    [switch]$SkipPush,
    [switch]$SkipRelease
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $MyInvocation.MyCommand.Path)

if (-not $SkipTests) {
    Write-Host "[1/6] cargo test"
    cargo test --quiet
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }
}

Write-Host "[2/6] cargo build --release"
cargo build --release --quiet
if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }

# ─── Stage portable package ───
$dist = Join-Path (Resolve-Path "target\dist") "h2ac-rs"
$zip = Join-Path (Resolve-Path "target\dist") "h2ac-rs-portable-$Tag.zip"

if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Path $dist | Out-Null

Copy-Item "target\release\h2ac-rs.exe" $dist -Force
if (Test-Path "assets\icons") {
    Copy-Item "assets\icons" (Join-Path $dist "assets\icons") -Recurse -Force
}
if ((Test-Path "plugins") -and (Get-ChildItem "plugins" -File -ErrorAction SilentlyContinue)) {
    Copy-Item "plugins\*" (Join-Path $dist "plugins") -Recurse -Force
}
if (Test-Path "README.md") { Copy-Item "README.md" $dist -Force }

if (Test-Path $zip) { Remove-Item $zip -Force }
Write-Host "[3/6] Compress-Archive -> $zip"
Compress-Archive -Path (Join-Path $dist "*") -DestinationPath $zip -CompressionLevel Optimal

# ─── Optional Inno Setup installer ───
$isccCandidates = @(
    "$env:ProgramFiles(x86)\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
)
$iscc = $isccCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
$setupExe = $null
if ($iscc) {
    Write-Host "[4/6] Inno Setup compile -> installer"
    & $iscc /Qp "installer.iss"
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
    $setupExe = Get-Item "h2ac-rs-setup-$Tag.exe" -ErrorAction SilentlyContinue
} else {
    Write-Host "[4/6] ISCC.exe not found; skipping installer (portable zip only)"
}

# ─── Push source to GitHub ───
if (-not $SkipPush) {
    Write-Host "[5/6] git push origin main"
    $env:GIT_SSL_BACKEND = "openssl"
    git push origin HEAD
    if ($LASTEXITCODE -ne 0) {
        $token = gh auth token 2>$null
        if (-not $token) { throw "git push failed and no gh auth token available" }
        Write-Host "    fallback: push with gh auth token"
        git -c credential.helper= -c http.sslBackend=openssl push "https://x-access-token:$token@github.com/$Repo.git" HEAD:main
        if ($LASTEXITCODE -ne 0) { throw "git push fallback failed" }
    }
} else {
    Write-Host "[5/6] skip git push (-SkipPush)"
}

# ─── Create / update GitHub release ───
if (-not $SkipRelease) {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        throw "gh CLI not found; install GitHub CLI or pass -SkipRelease"
    }

    $notes = (& git log --pretty=format:"- %s" "$PreviousTag..HEAD" 2>$null) -join "`n"
    if ([string]::IsNullOrWhiteSpace($notes)) {
        $notes = "- $ReleaseName`n- 便携包: $([IO.Path]::GetFileName($zip))"
    }

    Write-Host "[6/6] gh release -> $Repo $Tag"
    $existing = gh release view $Tag --repo $Repo --json tagName 2>$null
    if ($LASTEXITCODE -eq 0 -and $existing) {
        gh release upload $Tag $zip --repo $Repo --clobber
        if ($LASTEXITCODE -ne 0) { throw "gh release upload failed" }
        if ($setupExe) {
            gh release upload $Tag $setupExe.FullName --repo $Repo --clobber
        }
    } else {
        $assets = @($zip)
        if ($setupExe) { $assets += $setupExe.FullName }
        gh release create $Tag @assets --repo $Repo --title $ReleaseName --notes $notes
        if ($LASTEXITCODE -ne 0) { throw "gh release create failed" }
    }
    git fetch origin --tags --prune --quiet
    Write-Host "Release: https://github.com/$Repo/releases/tag/$Tag"
} else {
    Write-Host "[6/6] skip GitHub release (-SkipRelease)"
}

Write-Host ""
Write-Host "Artifacts:"
Get-Item $zip | Select-Object FullName, Length, LastWriteTime
if ($setupExe) { $setupExe | Select-Object FullName, Length, LastWriteTime }
