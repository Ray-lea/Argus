param(
    [Parameter(Mandatory = $true)][string]$Directory
)

$ErrorActionPreference = "Stop"
if ($env:GITHUB_ACTIONS -ne "true" -or $env:RUNNER_ENVIRONMENT -ne "github-hosted") {
    throw "Test-certificate trust is restricted to an ephemeral GitHub-hosted runner."
}

$stage = Join-Path $PSScriptRoot "..\target\signpath"
$certificatePath = Join-Path $stage "test-certificate.cer"
$reportPath = Join-Path $stage "signatures.json"
$files = @(Get-ChildItem -LiteralPath $Directory -File -Filter "*.exe")
if ($files.Count -eq 0) { throw "No signed executables were returned." }

$certificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new($certificatePath)
# Preflight fetched this public certificate from the selected SignPath policy.
# Trust it only on this ephemeral runner, before validating Authenticode.
Import-Certificate -FilePath $certificatePath -CertStoreLocation Cert:\CurrentUser\Root | Out-Null

foreach ($file in $files) {
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ($null -eq $signature.SignerCertificate) { throw "Missing signature: $($file.Name)" }
    $signer = $signature.SignerCertificate
    if ($signer.Subject -ne $signer.Issuer) {
        throw "The test workflow requires the provided self-signed test certificate."
    }
    if ($signer.Thumbprint -ne $certificate.Thumbprint) {
        throw "Signing certificate changed between phases: $($file.Name)"
    }
    $verified = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ($verified.Status -ne "Valid") {
        throw "Invalid signature on $($file.Name): $($verified.Status)"
    }
    $report = @()
    if (Test-Path -LiteralPath $reportPath) {
        $report = @(Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json)
    }
    $report += [pscustomobject]@{
        file = $file.Name
        status = [string]$verified.Status
        sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
        subject = $signer.Subject
        thumbprint = $signer.Thumbprint
        timestamped = $null -ne $verified.TimeStamperCertificate
    }
    $report | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $reportPath -Encoding utf8
    Write-Output "$($file.Name): Valid; certificate $($signer.Thumbprint)"
}
