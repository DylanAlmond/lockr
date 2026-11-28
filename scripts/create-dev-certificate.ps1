<#
.SYNOPSIS
  Create and trust a self-signed code-signing certificate for local builds.

.DESCRIPTION
  DEVELOPMENT ONLY. The certificate is self-signed, so Windows reports it as
  untrusted on any other machine and it does NOT satisfy control T-15. Use a
  CA-issued certificate or Azure Trusted Signing for a release.

  The certificate's CN is the publisher name Windows shows; O is the
  organisation. The script trusts the certificate on this machine, writes
  windows-signing.local.json next to the repository root, and prints the
  removal commands.

.PARAMETER Subject
  Certificate subject. Defaults to CN=Lockr, O=Dylan Almond.

.PARAMETER ValidYears
  Validity period in years. Defaults to 2.

.PARAMETER NoTrust
  Skip adding the certificate to the current user's Trusted Root store. The
  build still signs, but Authenticode reports the signature as untrusted.

.PARAMETER Exportable
  Make the private key exportable so `Export-PfxCertificate` can simulate the
  release workflow's PFX import. Default is non-exportable.
#>
[CmdletBinding()]
param(
  [string]$Subject = 'CN=Lockr, O=Dylan Almond',
  [int]$ValidYears = 2,
  [switch]$NoTrust,
  [switch]$Exportable
)

$ErrorActionPreference = 'Stop'

$keyExportPolicy = if ($Exportable) { 'Exportable' } else { 'NonExportable' }

$cert = New-SelfSignedCertificate `
  -Type CodeSigningCert `
  -Subject $Subject `
  -FriendlyName 'Lockr (self-signed development)' `
  -CertStoreLocation Cert:\CurrentUser\My `
  -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
  -KeyExportPolicy $keyExportPolicy `
  -NotAfter (Get-Date).AddYears($ValidYears)

if (-not $NoTrust) {
  $publicCert = Join-Path $env:TEMP "lockr-dev-$($cert.Thumbprint).cer"
  Export-Certificate -Cert $cert -FilePath $publicCert | Out-Null
  Import-Certificate -FilePath $publicCert -CertStoreLocation Cert:\CurrentUser\Root | Out-Null
  Remove-Item -Path $publicCert
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$config = Join-Path $repoRoot 'windows-signing.local.json'
@{
  bundle = @{
    windows = @{
      certificateThumbprint = $cert.Thumbprint
    }
  }
} | ConvertTo-Json -Depth 5 | Set-Content -Path $config -Encoding utf8

[pscustomobject]@{
  Subject    = $cert.Subject
  Thumbprint = $cert.Thumbprint
  NotAfter   = $cert.NotAfter
  Trusted    = -not $NoTrust
  Config     = $config
} | Format-List

Write-Host "Next: yarn tauri build --config windows-signing.local.json"
Write-Host "Then: pwsh scripts/verify-windows-signatures.ps1"
Write-Host ""
Write-Host "To remove this development certificate:"
Write-Host "  Remove-Item 'Cert:\CurrentUser\Root\$($cert.Thumbprint)'"
Write-Host "  Remove-Item 'Cert:\CurrentUser\My\$($cert.Thumbprint)'"
Write-Host "  Remove-Item '$config'"
