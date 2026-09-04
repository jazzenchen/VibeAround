# Installs the VibeAround-managed Node.js runtime under
# $STARTKIT_HOME\runtime\node, mirroring what the daemon reads back:
#   versions\<version>\   the extracted release
#   current.json          the manifest the daemon treats as "installed"
#   current.env           the same facts for shell consumers
#
# Progress is streamed as NDJSON; the last JSON line is the result.
$ErrorActionPreference = "Stop"

function Emit($obj) { $obj | ConvertTo-Json -Compress }
function Progress($message) { Emit @{ event = "progress"; message = $message } }
function Fail($message) {
  Emit @{ event = "result"; status = "error"; message = $message; actions = @("install") }
  exit 0
}

if (-not $env:STARTKIT_HOME) { Fail "STARTKIT_HOME is required." }
$runtimeDir = Join-Path $env:STARTKIT_HOME "runtime\node"
$versionsDir = Join-Path $runtimeDir "versions"
$cacheDir = if ($env:STARTKIT_CACHE_DIR) { $env:STARTKIT_CACHE_DIR } else { Join-Path $runtimeDir "cache" }
$indexUrl = if ($env:STARTKIT_NODE_INDEX_URL) { $env:STARTKIT_NODE_INDEX_URL } else { "https://nodejs.org/dist/index.json" }
$distBase = if ($env:STARTKIT_NODE_DIST_BASE) { $env:STARTKIT_NODE_DIST_BASE } else { "https://nodejs.org/dist" }
$minVersion = if ($env:STARTKIT_MIN_VERSION) { $env:STARTKIT_MIN_VERSION } else { "22.0.0" }
New-Item -ItemType Directory -Force -Path $versionsDir, $cacheDir | Out-Null

$platform = switch ($env:PROCESSOR_ARCHITECTURE) {
  "ARM64" { "win-arm64" }
  "AMD64" { "win-x64" }
  default { Fail "Unsupported Windows architecture: $($env:PROCESSOR_ARCHITECTURE)" }
}

# index.tab carries the same releases as index.json in tab-separated columns
# (1=version, 10=LTS codename, "-" when not LTS), newest first, so the newest
# qualifying LTS can be picked without parsing the JSON index.
Progress "Resolving the latest Node.js LTS release"
$indexUrlTab = $indexUrl -replace "index\.json$", "index.tab"
try {
  $index = (Invoke-WebRequest -Uri $indexUrlTab -UseBasicParsing).Content
} catch {
  Fail "Failed to download the Node.js version index."
}

$minMajor = [int](($minVersion.TrimStart("v") -split "\.")[0])
$version = $null
foreach ($line in ($index -split "`n" | Select-Object -Skip 1)) {
  $columns = $line -split "`t"
  if ($columns.Count -lt 10) { continue }
  if ($columns[9] -eq "-" -or [string]::IsNullOrWhiteSpace($columns[9])) { continue }
  if ([int](($columns[0].TrimStart("v") -split "\.")[0]) -ge $minMajor) {
    $version = $columns[0]
    break
  }
}
if (-not $version) { Fail "Could not resolve a Node.js LTS release at or above $minVersion." }

$installDir = Join-Path $versionsDir ($version -replace "[^A-Za-z0-9._-]", "_")
$nodeBin = Join-Path $installDir "node.exe"

$installed = $false
if (Test-Path $nodeBin) {
  $current = (& $nodeBin --version 2>$null)
  if ($current -eq $version) { $installed = $true }
}

if (-not $installed) {
  $archiveName = "node-$version-$platform.zip"
  $archivePath = Join-Path $cacheDir $archiveName

  Progress "Downloading Node.js $version"
  try {
    Invoke-WebRequest -Uri "$distBase/$version/$archiveName" -OutFile $archivePath -UseBasicParsing
  } catch {
    Fail "Failed to download $archiveName."
  }

  # Node publishes SHASUMS256.txt next to the archives; verifying it is the only
  # thing standing between a hijacked mirror and an executed binary.
  Progress "Verifying the Node.js archive"
  try {
    $sums = (Invoke-WebRequest -Uri "$distBase/$version/SHASUMS256.txt" -UseBasicParsing).Content
  } catch {
    Fail "Failed to download SHASUMS256.txt for $version."
  }
  $expected = $null
  foreach ($line in ($sums -split "`n")) {
    $parts = $line -split "\s+" | Where-Object { $_ }
    if ($parts.Count -ge 2 -and $parts[1] -eq $archiveName) { $expected = $parts[0]; break }
  }
  if (-not $expected) { Fail "No checksum published for $archiveName." }
  $actual = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLower()
  if ($actual -ne $expected.ToLower()) {
    Remove-Item -Force $archivePath -ErrorAction SilentlyContinue
    Fail "Checksum mismatch for ${archiveName}: expected $expected, got $actual."
  }

  Progress "Extracting Node.js $version"
  $stagingDir = Join-Path $versionsDir ".staging-node"
  if (Test-Path $stagingDir) { Remove-Item -Recurse -Force $stagingDir }
  New-Item -ItemType Directory -Force -Path $stagingDir | Out-Null
  try {
    Expand-Archive -Path $archivePath -DestinationPath $stagingDir -Force
  } catch {
    Fail "Failed to extract $archiveName."
  }
  # The zip wraps everything in node-<version>-<platform>\; strip that root.
  $root = Get-ChildItem -Path $stagingDir -Directory | Select-Object -First 1
  if (-not $root -or -not (Test-Path (Join-Path $root.FullName "node.exe"))) {
    Fail "$archiveName did not contain node.exe."
  }
  if (Test-Path $installDir) { Remove-Item -Recurse -Force $installDir }
  Move-Item -Path $root.FullName -Destination $installDir
  Remove-Item -Recurse -Force $stagingDir -ErrorAction SilentlyContinue
  Remove-Item -Force $archivePath -ErrorAction SilentlyContinue
}

# The shell mirror lands before the manifest, because the manifest is what marks
# the tool as installed.
Progress "Recording the Node.js runtime"
function Quote($value) { "'" + ($value -replace "'", "'\''") + "'" }
$stateLines = @(
  "# Generated by VibeAround. Do not edit; regenerated on every install."
  "VA_TOOL=" + (Quote "node")
  "VA_TOOL_VERSION=" + (Quote $version)
  "VA_TOOL_INSTALL_DIR=" + (Quote $installDir)
  "VA_TOOL_BIN_DIR=" + (Quote $installDir)
)
$stateTmp = Join-Path $runtimeDir "current.env.tmp"
Set-Content -Path $stateTmp -Value ($stateLines -join "`n") -NoNewline -Encoding utf8
Move-Item -Force -Path $stateTmp -Destination (Join-Path $runtimeDir "current.env")

$manifest = [ordered]@{
  version = $version
  install_dir = $installDir
  installed_at_unix_ms = [int64]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
}
$manifestTmp = Join-Path $runtimeDir "current.json.tmp"
Set-Content -Path $manifestTmp -Value ($manifest | ConvertTo-Json) -Encoding utf8
Move-Item -Force -Path $manifestTmp -Destination (Join-Path $runtimeDir "current.json")

Emit @{ event = "result"; status = "ok"; version = $version; path = $nodeBin; message = "Node.js $version is ready"; actions = @() }
