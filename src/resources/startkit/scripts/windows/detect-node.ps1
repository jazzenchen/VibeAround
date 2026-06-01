$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

function Major($version) {
  if (-not $version) { return 0 }
  return [int](($version.TrimStart("v") -split "\.")[0])
}

$minMajor = Major($env:STARTKIT_MIN_VERSION)
$mode = if ($env:STARTKIT_TOOLCHAIN_MODE) { $env:STARTKIT_TOOLCHAIN_MODE } else { "auto" }
$candidate = $null
if ($mode -ne "system" -and $env:STARTKIT_NODE_DIR) {
  $managed = Join-Path $env:STARTKIT_NODE_DIR "node.exe"
  if (Test-Path $managed) { $candidate = $managed }
}
if (-not $candidate -and $mode -ne "managed") {
  $cmd = Get-Command node -ErrorAction SilentlyContinue
  if ($cmd) { $candidate = $cmd.Source }
}

if (-not $candidate) {
  $message = if ($mode -eq "managed") { "Managed Node.js was not found" } else { "Node.js was not found" }
  Emit @{ status = "missing"; message = $message; actions = @("install") }
  exit 0
}

$version = & $candidate --version 2>$null
if ((Major $version) -lt $minMajor) {
  Emit @{ status = "outdated"; version = $version; path = $candidate; message = "Node.js $version is below the required version"; actions = @("install") }
  exit 0
}

Emit @{ status = "ok"; version = $version; path = $candidate; message = "Node.js is ready"; actions = @() }
