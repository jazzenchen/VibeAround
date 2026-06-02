$ErrorActionPreference = "Stop"

function Write-StartkitJson($obj) {
  $obj | ConvertTo-Json -Compress
}

if ($env:STARTKIT_TOOLCHAIN_MODE -eq "system") {
  Write-StartkitJson @{
    status = "blocked"
    message = "System-only mode is selected, so managed PATH entries were not written."
    actions = @()
  }
  exit 0
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

for ($i = $paths.Count - 1; $i -ge 0; $i--) {
  $path = $paths[$i]
  $exists = $false
  foreach ($part in $parts) {
    if ([string]::Equals($part, $path, [StringComparison]::OrdinalIgnoreCase)) {
      $exists = $true
      break
    }
  }
  if (-not $exists) {
    $parts = @($path) + $parts
  }
}

[Environment]::SetEnvironmentVariable("Path", ($parts -join ";"), "User")

Write-StartkitJson @{
  status = "ok"
  path = "HKCU:\Environment\Path"
  message = "User PATH configured for VibeAround-managed tools"
  actions = @()
}
