$ErrorActionPreference = "Stop"

$expectedVersion = "v3.13"
$expectedSha256 = "DB9A1038BBC0E53FEF24F423FD609979BD578E942F1BE1198DB186D1C42652A3"
$url = "https://downloads.sourceforge.net/project/nsis/NSIS%203/3.13/nsis-3.13-setup.exe"

function Find-Makensis {
  foreach ($candidate in @(
    $env:ARGUS_NSIS,
    (Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe"),
    (Join-Path $env:ProgramFiles "NSIS\makensis.exe")
  )) {
    if ($candidate -and (Test-Path -LiteralPath $candidate)) {
      return $candidate
    }
  }
  return $null
}

$existing = Find-Makensis
if ($existing) {
  $ver = (& $existing /VERSION).Trim()
  if ($ver -eq $expectedVersion) {
    Write-Host "NSIS $ver already installed at $existing"
    exit 0
  }
}

$tempDir = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
$setup = Join-Path $tempDir "nsis-3.13-setup.exe"
$downloaded = $false

for ($attempt = 1; $attempt -le 3; $attempt++) {
  try {
    Write-Host "Downloading NSIS 3.13 from SourceForge (attempt $attempt/3)..."
    & curl.exe -fsSL --retry 2 -A "Wget/1.21" $url -o $setup
    if ($LASTEXITCODE -ne 0) {
      throw "curl exited with code $LASTEXITCODE"
    }
    $actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $setup).Hash.ToUpperInvariant()
    if ($actualSha256 -ne $expectedSha256) {
      throw "NSIS SHA-256 mismatch: expected $expectedSha256, got $actualSha256"
    }
    $downloaded = $true
    break
  } catch {
    Write-Warning "SourceForge download attempt $attempt failed: $_"
    Remove-Item -LiteralPath $setup -Force -ErrorAction SilentlyContinue
    if ($attempt -lt 3) { Start-Sleep -Seconds (5 * $attempt) }
  }
}

if ($downloaded) {
  $proc = Start-Process -FilePath $setup -ArgumentList "/S" -WindowStyle Hidden -Wait -PassThru
  Remove-Item -LiteralPath $setup -Force -ErrorAction SilentlyContinue
  if ($proc.ExitCode -ne 0) {
    throw "NSIS silent installer exited with code $($proc.ExitCode)"
  }
} else {
  Write-Warning "Falling back to package manager for NSIS 3.13..."
  $installed = $false
  if (Get-Command choco.exe -ErrorAction SilentlyContinue) {
    choco install nsis --version=3.13 -y --no-progress
    if ($LASTEXITCODE -eq 0) { $installed = $true }
  }
  if (-not $installed -and (Get-Command winget.exe -ErrorAction SilentlyContinue)) {
    winget install --id NSIS.NSIS --exact --version 3.13 --architecture x86 --accept-package-agreements --accept-source-agreements --silent
    if ($LASTEXITCODE -eq 0) { $installed = $true }
  }
  if (-not $installed) {
    throw "Failed to install NSIS 3.13 from SourceForge, Chocolatey, and WinGet."
  }
}

$compiler = Find-Makensis
if (-not $compiler) {
  throw "makensis.exe not found after installation"
}
$installedVersion = (& $compiler /VERSION).Trim()
if ($installedVersion -ne $expectedVersion) {
  throw "Unexpected NSIS version: expected $expectedVersion, got $installedVersion"
}
Write-Host "Verified NSIS $installedVersion at $compiler"
