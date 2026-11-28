# Verify the Authenticode signatures on a Windows release (control T-15).
#
# A Windows build is not releasable until the main binary and every installer
# carry a valid, timestamped signature. Run after `yarn tauri build`.
#
# The signing certificate itself is supplied by the release pipeline; this
# script only inspects what the build produced.

[CmdletBinding()]
param(
  [string]$ReleaseRoot
)

$ErrorActionPreference = 'Stop'

if (-not $ReleaseRoot) {
  $scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
  $ReleaseRoot = Join-Path $scriptRoot '..\src-tauri\target\release'
}
$ReleaseRoot = (Resolve-Path -Path $ReleaseRoot).Path

$binary = Join-Path $ReleaseRoot 'lockr.exe'
$bundleRoot = Join-Path $ReleaseRoot 'bundle'

$artefacts = @()
if (Test-Path $binary) {
  $artefacts += Get-Item $binary
}
if (Test-Path $bundleRoot) {
  $artefacts += Get-ChildItem -Path $bundleRoot -Recurse -File |
    Where-Object { $_.Extension -in '.msi', '.exe' }
}

if ($artefacts.Count -eq 0) {
  throw "No Windows artefacts found under $ReleaseRoot. Build first with 'yarn tauri build'."
}

$failed = 0
foreach ($artefact in $artefacts) {
  $signature = Get-AuthenticodeSignature -FilePath $artefact.FullName
  $timestamped = $null -ne $signature.TimeStamperCertificate
  $suffix = if ($timestamped) { ' (timestamped)' } else { ' (NO TIMESTAMP)' }
  "{0}: {1}{2}" -f $artefact.Name, $signature.Status, $suffix

  if ($signature.Status -ne 'Valid' -or -not $timestamped) {
    $failed += 1
  }
}

if ($failed -gt 0) {
  throw "$failed of $($artefacts.Count) Windows artefacts are not validly signed and timestamped (T-15)"
}

Write-Host "All $($artefacts.Count) Windows artefacts carry a valid, timestamped signature."
