# Pack the built confy-desktop.exe into an unsigned .msix (Windows only).
#
#   pack-msix.ps1 -Exe <path\to\confy-desktop.exe> -Version 0.12.2.0 -Out confy-desktop.msix
#
# Identity defaults are CI/sideload placeholders; for a Store submission pass
# the Partner Center values (see STORE.md) or set the MSIX_IDENTITY_NAME /
# MSIX_PUBLISHER / MSIX_PUBLISHER_DISPLAY environment variables.
# The output is deliberately UNSIGNED: the Store signs submissions itself, and
# sideload testing signs with a local self-signed cert (STORE.md).
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$IdentityName = $(if ($env:MSIX_IDENTITY_NAME) { $env:MSIX_IDENTITY_NAME } else { "superyngo.confy" }),
    [string]$Publisher = $(if ($env:MSIX_PUBLISHER) { $env:MSIX_PUBLISHER } else { "CN=00000000-0000-0000-0000-000000000000" }),
    [string]$PublisherDisplay = $(if ($env:MSIX_PUBLISHER_DISPLAY) { $env:MSIX_PUBLISHER_DISPLAY } else { "superyngo" })
)
$ErrorActionPreference = "Stop"

# Locate makeappx.exe in the newest installed Windows SDK.
$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $makeappx) { throw "makeappx.exe not found - is the Windows SDK installed?" }

$msixDir = $PSScriptRoot
$iconsDir = Join-Path $msixDir "..\icons"
$staging = Join-Path ([System.IO.Path]::GetTempPath()) "confy-msix-staging"
if (Test-Path $staging) { Remove-Item $staging -Recurse -Force }
New-Item -ItemType Directory -Path (Join-Path $staging "Assets") | Out-Null

Copy-Item $Exe (Join-Path $staging "confy-desktop.exe")
foreach ($logo in "Square44x44Logo.png", "Square150x150Logo.png", "StoreLogo.png") {
    Copy-Item (Join-Path $iconsDir $logo) (Join-Path $staging "Assets\$logo")
}

(Get-Content (Join-Path $msixDir "AppxManifest.xml") -Raw) `
    -replace "__VERSION__", $Version `
    -replace "__IDENTITY_NAME__", $IdentityName `
    -replace "__PUBLISHER__", $Publisher `
    -replace "__PUBLISHER_DISPLAY__", $PublisherDisplay |
    Set-Content (Join-Path $staging "AppxManifest.xml") -Encoding utf8

& $makeappx.FullName pack /d $staging /p $Out /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }
Write-Host "packed $Out (identity $IdentityName, version $Version, unsigned)"
