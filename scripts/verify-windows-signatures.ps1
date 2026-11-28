# Verify the Authenticode signatures on a Windows release (control T-15).
#
# A Windows build is not releasable until every installer, and the executable
# it packages, carries a valid, timestamped signature. Run after
# `yarn tauri build`.
#
# The standalone target/release/lockr.exe is the pre-bundle build output; Tauri
# signs the copy it packages into the installers. That intermediate is not a
# release artefact and is deliberately not checked here. The MSI payload is
# extracted to prove the shipped executable is signed too.

[CmdletBinding()]
param(
  [string]$ReleaseRoot,
  [switch]$SkipMsiPayload
)

$ErrorActionPreference = 'Stop'

if (-not $ReleaseRoot) {
  $scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
  $ReleaseRoot = Join-Path $scriptRoot '..\src-tauri\target\release'
}
$ReleaseRoot = (Resolve-Path -Path $ReleaseRoot).Path

$bundleRoot = Join-Path $ReleaseRoot 'bundle'
if (-not (Test-Path $bundleRoot)) {
  throw "No bundle directory under $ReleaseRoot. Build first with 'yarn tauri build'."
}

$artefacts = @(
  Get-ChildItem -Path $bundleRoot -Recurse -File |
    Where-Object { $_.Extension -in '.msi', '.exe' }
)

if ($artefacts.Count -eq 0) {
  throw "No Windows installers found under $bundleRoot. Build first with 'yarn tauri build'."
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

# The MSI embeds the executable that is actually installed. Verifying the
# installer alone would not prove that the app binary itself is signed.
$msi = $artefacts | Where-Object { $_.Extension -eq '.msi' } | Select-Object -First 1
if ($msi -and -not $SkipMsiPayload) {
  $extract = Join-Path $env:TEMP "lockr-msi-verify-$PID"
  New-Item -ItemType Directory -Path $extract -Force | Out-Null

  try {
    $process = Start-Process msiexec.exe -Wait -PassThru -NoNewWindow -ArgumentList @(
      '/a', "`"$($msi.FullName)`"", '/qn', "TARGETDIR=`"$extract`""
    )
    if ($process.ExitCode -ne 0) {
      throw "msiexec /a failed with exit code $($process.ExitCode); cannot verify the MSI payload"
    }

    $payload = Get-ChildItem -Path $extract -Recurse -File -Filter 'lockr.exe' |
      Select-Object -First 1
    if (-not $payload) {
      throw "The MSI payload does not contain lockr.exe"
    }

    $signature = Get-AuthenticodeSignature -FilePath $payload.FullName
    $timestamped = $null -ne $signature.TimeStamperCertificate
    $suffix = if ($timestamped) { ' (timestamped)' } else { ' (NO TIMESTAMP)' }
    "MSI payload {0}: {1}{2}" -f $payload.Name, $signature.Status, $suffix

    if ($signature.Status -ne 'Valid' -or -not $timestamped) {
      $failed += 1
    }
  } finally {
    Remove-Item -Path $extract -Recurse -Force -ErrorAction SilentlyContinue
  }
}

if ($failed -gt 0) {
  throw "$failed Windows release artefact(s) are not validly signed and timestamped (T-15)"
}

$checked = $artefacts.Count + $(if ($msi -and -not $SkipMsiPayload) { 1 } else { 0 })
Write-Host "All $checked Windows release artefacts carry a valid, timestamped signature."
