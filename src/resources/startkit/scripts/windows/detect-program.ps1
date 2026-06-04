$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

$program = $env:STARTKIT_PROGRAM
$versionArg = if ($env:STARTKIT_VERSION_ARG) { $env:STARTKIT_VERSION_ARG } else { "--version" }
$mode = if ($env:STARTKIT_TOOLCHAIN_MODE) { $env:STARTKIT_TOOLCHAIN_MODE } else { "auto" }
$cmd = $null

function PackageName($package) {
  if (-not $package) { return $null }
  if ($package.StartsWith("@")) {
    $slash = $package.IndexOf("/")
    if ($slash -ge 0) {
      $scope = $package.Substring(0, $slash)
      $rest = $package.Substring($slash + 1)
      $at = $rest.LastIndexOf("@")
      if ($at -gt 0) { return "$scope/$($rest.Substring(0, $at))" }
    }
    return $package
  }
  $plainAt = $package.LastIndexOf("@")
  if ($plainAt -gt 0) { return $package.Substring(0, $plainAt) }
  return $package
}

function RequestedPackageVersion($package) {
  if (-not $package) { return $null }
  if ($package.StartsWith("@")) {
    $slash = $package.IndexOf("/")
    if ($slash -ge 0) {
      $rest = $package.Substring($slash + 1)
      $at = $rest.LastIndexOf("@")
      if ($at -gt 0) { return $rest.Substring($at + 1) }
    }
    return $null
  }
  $plainAt = $package.LastIndexOf("@")
  if ($plainAt -gt 0) { return $package.Substring($plainAt + 1) }
  return $null
}

function ManagedCommand($program) {
  if ($env:STARTKIT_NPM_PACKAGE) {
    $managedCmd = Join-Path $env:STARTKIT_NPM_PREFIX "$program.cmd"
    if (Test-Path $managedCmd) { return @{ Source = $managedCmd } }
    $managedBinCmd = Join-Path (Join-Path $env:STARTKIT_NPM_PREFIX "bin") "$program.cmd"
    if (Test-Path $managedBinCmd) { return @{ Source = $managedBinCmd } }
  }
  $managedExe = Join-Path $env:STARTKIT_BIN_DIR "$program.exe"
  if (Test-Path $managedExe) { return @{ Source = $managedExe } }
  $managedPlain = Join-Path $env:STARTKIT_BIN_DIR $program
  if (Test-Path $managedPlain) { return @{ Source = $managedPlain } }
  return $null
}

function NodeCommand() {
  $managedNode = Join-Path $env:STARTKIT_NODE_DIR "node.exe"
  if (Test-Path $managedNode) { return $managedNode }
  $node = Get-Command node -ErrorAction SilentlyContinue
  if ($node) { return $node.Source }
  return $null
}

function NpmCommand() {
  $managedNpm = Join-Path $env:STARTKIT_NODE_DIR "npm.cmd"
  if (Test-Path $managedNpm) { return $managedNpm }
  $npm = Get-Command npm -ErrorAction SilentlyContinue
  if ($npm) { return $npm.Source }
  return $null
}

function LocalPackageVersion($package) {
  $name = PackageName $package
  $node = NodeCommand
  if (-not $name -or -not $node) { return $null }
  $script = @'
const fs = require("fs");
const path = require("path");
const name = process.argv[1];
const prefix = process.argv[2];
const roots = [
  path.join(prefix, "node_modules"),
  path.join(prefix, "lib", "node_modules"),
];
for (const root of roots) {
  const file = path.join(root, ...name.split("/"), "package.json");
  if (!fs.existsSync(file)) continue;
  const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
  if (pkg.version) {
    console.log(pkg.version);
    process.exit(0);
  }
}
process.exit(1);
'@
  $value = (& $node -e $script $name $env:STARTKIT_NPM_PREFIX 2>$null | Select-Object -First 1) -join ""
  if ($LASTEXITCODE -eq 0 -and $value) { return $value }
  return $null
}

function LatestPackageVersion($package) {
  $name = PackageName $package
  $npm = NpmCommand
  if (-not $name -or -not $npm) { return $null }
  $registry = if ($env:STARTKIT_NPM_REGISTRY) { $env:STARTKIT_NPM_REGISTRY } else { "https://registry.npmjs.org" }
  $value = (& $npm view $name version --registry $registry 2>$null | Select-Object -Last 1) -join ""
  if ($LASTEXITCODE -eq 0 -and $value) { return $value }
  return $null
}

function IsManagedPath($path) {
  if (-not $path) { return $false }
  foreach ($prefix in @($env:STARTKIT_NPM_PREFIX, $env:STARTKIT_BIN_DIR)) {
    if ($prefix -and $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
      return $true
    }
  }
  return $false
}

if ($mode -ne "system" -and $env:STARTKIT_ITEM_MANAGED -eq "true") {
  $cmd = ManagedCommand $program
}
if (-not $cmd -and $mode -ne "managed") {
  $cmd = Get-Command $program -ErrorAction SilentlyContinue
}

if (-not $cmd) {
  Emit @{ status = "missing"; message = "$program was not found in PATH"; actions = @("install") }
  exit 0
}

$version = (& $cmd.Source $versionArg 2>&1 | Select-Object -First 1) -join ""
if ($env:STARTKIT_NPM_PACKAGE -and (IsManagedPath $cmd.Source)) {
  $localVersion = LocalPackageVersion $env:STARTKIT_NPM_PACKAGE
  $requestedVersion = RequestedPackageVersion $env:STARTKIT_NPM_PACKAGE
  $desiredVersion = if ($requestedVersion) { $requestedVersion } else { LatestPackageVersion $env:STARTKIT_NPM_PACKAGE }
  if ($localVersion -and $desiredVersion -and $localVersion -ne $desiredVersion) {
    Emit @{
      status = "outdated"
      version = $version
      path = $cmd.Source
      message = "$program $localVersion is below the latest available version $desiredVersion"
      actions = @("install")
    }
    exit 0
  }
}

Emit @{ status = "ok"; version = $version; path = $cmd.Source; message = "$program is ready"; actions = @() }
