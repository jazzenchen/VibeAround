$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

$program = $env:STARTKIT_PROGRAM
$versionArg = if ($env:STARTKIT_VERSION_ARG) { $env:STARTKIT_VERSION_ARG } else { "--version" }
$cmd = Get-Command $program -ErrorAction SilentlyContinue

if (-not $cmd) {
  Emit @{ status = "missing"; message = "$program was not found in PATH"; actions = @("install") }
  exit 0
}

$version = (& $cmd.Source $versionArg 2>&1 | Select-Object -First 1) -join ""
Emit @{ status = "ok"; version = $version; path = $cmd.Source; message = "$program is ready"; actions = @() }

