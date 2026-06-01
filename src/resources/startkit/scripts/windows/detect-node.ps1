$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

function Major($version) {
  if (-not $version) { return 0 }
  return [int](($version.TrimStart("v") -split "\.")[0])
}

$minMajor = Major($env:STARTKIT_MIN_VERSION)
$candidate = $null
if ($env:STARTKIT_NODE_DIR) {
  $managed = Join-Path $env:STARTKIT_NODE_DIR "node.exe"
  if (Test-Path $managed) { $candidate = $managed }
}
if (-not $candidate) {
  $cmd = Get-Command node -ErrorAction SilentlyContinue
  if ($cmd) { $candidate = $cmd.Source }
}

if (-not $candidate) {
  Emit @{ status = "missing"; message = "Node.js was not found"; actions = @("install") }
  exit 0
}

$version = & $candidate --version 2>$null
if ((Major $version) -lt $minMajor) {
  Emit @{ status = "outdated"; version = $version; path = $candidate; message = "Node.js $version is below the required version"; actions = @("install") }
  exit 0
}

Emit @{ status = "ok"; version = $version; path = $candidate; message = "Node.js is ready"; actions = @() }

