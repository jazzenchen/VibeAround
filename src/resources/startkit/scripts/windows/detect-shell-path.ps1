$ErrorActionPreference = "Stop"

function Write-StartkitJson($obj) {
  $obj | ConvertTo-Json -Compress
}

$paths = @(
  $env:STARTKIT_BIN_DIR,
  (Join-Path $env:STARTKIT_NODE_DIR "bin"),
  $env:STARTKIT_NODE_DIR,
  (Join-Path $env:STARTKIT_NPM_PREFIX "bin"),
  $env:STARTKIT_NPM_PREFIX
) | Where-Object { $_ -and $_.Trim().Length -gt 0 }

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$parts = @()
if ($userPath) {
  $parts = $userPath -split ";" | Where-Object { $_ -and $_.Trim().Length -gt 0 }
}

$missing = @()
foreach ($path in $paths) {
  $exists = $false
  foreach ($part in $parts) {
    if ([string]::Equals($part, $path, [StringComparison]::OrdinalIgnoreCase)) {
      $exists = $true
      break
    }
  }
  if (-not $exists) {
    $missing += $path
  }
}

if ($missing.Count -eq 0) {
  Write-StartkitJson @{
    status = "ok"
    path = "HKCU:\Environment\Path"
    message = "User PATH is configured"
    actions = @()
  }
} else {
  Write-StartkitJson @{
    status = "missing"
    message = "User PATH is not configured for VibeAround-managed tools"
    actions = @("install")
  }
}
