# Helpers for start.bat: session preflight and build cache cleanup.
# Saved as UTF-8 with BOM so Windows PowerShell 5.1 reads the Chinese text correctly.

param(
  [Parameter(Position = 0)][ValidateSet("preflight", "clean")][string]$Action = "preflight",
  [Parameter(Position = 1)][string]$Mode = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "target"
$devExe = Join-Path $target "debug\argus.exe"

function Format-Size([double]$bytes) {
  if ($bytes -ge 1GB) { return "{0:N2} GB" -f ($bytes / 1GB) }
  return "{0:N0} MB" -f ($bytes / 1MB)
}

function Get-DirSize([string]$path) {
  if (-not (Test-Path $path)) { return 0 }
  $sum = (Get-ChildItem $path -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
  if ($sum) { return $sum } else { return 0 }
}

# A console closed mid-session leaves Vite holding port 5173 (tauri dev then
# refuses to start) and can leave the dev app running in the tray.
function Stop-StaleSession {
  $listener = Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($listener) {
    $owner = Get-CimInstance Win32_Process -Filter "ProcessId=$($listener.OwningProcess)"
    if ($owner.CommandLine -like "*$root*") {
      Stop-Process -Id $owner.ProcessId -Force
      Write-Host "[准备] 已结束上次残留的 Vite（PID $($owner.ProcessId)）"
    } else {
      Write-Host "[错误] 端口 5173 被 $($owner.Name)（PID $($owner.ProcessId)）占用，请先关闭它。" -ForegroundColor Red
      exit 1
    }
  }
  Get-CimInstance Win32_Process -Filter "Name='argus.exe'" |
    Where-Object { $_.ExecutablePath -eq $devExe } |
    ForEach-Object {
      Stop-Process -Id $_.ProcessId -Force
      Write-Host "[准备] 已结束上次残留的开发版 Argus（PID $($_.ProcessId)）"
    }
}

function Invoke-Clean([string]$mode) {
  Stop-StaleSession
  Write-Host "[清理] 统计 target 大小……"
  $before = Get-DirSize $target

  if ($mode -eq "all") {
    Push-Location $root
    try { cargo clean } finally { Pop-Location }
    $viteCache = Join-Path $root "node_modules\.vite"
    if (Test-Path $viteCache) { Remove-Item $viteCache -Recurse -Force }
  } else {
    # Cheap to regenerate, and the parts that grow without bound. The debug
    # dependency cache stays, so the next `start.bat` is still a fast build.
    $paths = @(
      "debug\incremental",
      "rust-analyzer\debug\incremental",
      "release",
      "cargo-timings"
    ) | ForEach-Object { Join-Path $target $_ }
    foreach ($p in $paths) {
      if (Test-Path $p) {
        Remove-Item $p -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host "  已删除 $($p.Substring($root.Length + 1))"
      }
    }
  }

  $after = Get-DirSize $target
  Write-Host ("[完成] target：{0} -> {1}，释放 {2}" -f (Format-Size $before), (Format-Size $after), (Format-Size ($before - $after)))
}

switch ($Action) {
  "preflight" { Stop-StaleSession }
  "clean" { Invoke-Clean $Mode }
}
