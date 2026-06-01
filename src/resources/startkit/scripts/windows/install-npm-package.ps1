$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

$package = $env:STARTKIT_NPM_PACKAGE
$program = $env:STARTKIT_PROGRAM
New-Item -ItemType Directory -Force -Path $env:STARTKIT_NPM_PREFIX | Out-Null

$npm = "npm"
$managedNpm = Join-Path $env:STARTKIT_NODE_DIR "npm.cmd"
if (Test-Path $managedNpm) { $npm = $managedNpm }

& $npm install --global --prefix $env:STARTKIT_NPM_PREFIX --registry $env:STARTKIT_NPM_REGISTRY $package

$cmd = Get-Command $program -ErrorAction SilentlyContinue
if (-not $cmd) {
  $env:PATH = "$env:STARTKIT_NPM_PREFIX;$env:STARTKIT_NODE_DIR;$env:PATH"
  $cmd = Get-Command $program -ErrorAction SilentlyContinue
}

if (-not $cmd) {
  Emit @{ status = "error"; message = "Installed $package but $program is still not on PATH"; actions = @("repair") }
  exit 0
}

$versionArg = if ($env:STARTKIT_VERSION_ARG) { $env:STARTKIT_VERSION_ARG } else { "--version" }
$version = (& $cmd.Source $versionArg 2>&1 | Select-Object -First 1) -join ""
Emit @{ status = "ok"; version = $version; path = $cmd.Source; message = "$program installed"; actions = @() }

