$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

$program = $env:STARTKIT_PROGRAM
$versionArg = if ($env:STARTKIT_VERSION_ARG) { $env:STARTKIT_VERSION_ARG } else { "--version" }
$mode = if ($env:STARTKIT_TOOLCHAIN_MODE) { $env:STARTKIT_TOOLCHAIN_MODE } else { "auto" }
$cmd = $null

if ($mode -eq "managed" -and $env:STARTKIT_ITEM_MANAGED -eq "true") {
  if ($env:STARTKIT_NPM_PACKAGE) {
    $managedCmd = Join-Path $env:STARTKIT_NPM_PREFIX "$program.cmd"
    if (Test-Path $managedCmd) { $cmd = @{ Source = $managedCmd } }
  }
  if (-not $cmd) {
    $managedExe = Join-Path $env:STARTKIT_BIN_DIR "$program.exe"
    if (Test-Path $managedExe) { $cmd = @{ Source = $managedExe } }
  }
} else {
  $cmd = Get-Command $program -ErrorAction SilentlyContinue
}

if (-not $cmd) {
  Emit @{ status = "missing"; message = "$program was not found in PATH"; actions = @("install") }
  exit 0
}

$version = (& $cmd.Source $versionArg 2>&1 | Select-Object -First 1) -join ""
Emit @{ status = "ok"; version = $version; path = $cmd.Source; message = "$program is ready"; actions = @() }
