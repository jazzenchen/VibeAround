$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

function Major($version) {
  if (-not $version) { return 0 }
  return [int](($version.TrimStart("v") -split "\.")[0])
}

New-Item -ItemType Directory -Force -Path $env:STARTKIT_NODE_DIR, $env:STARTKIT_CACHE_DIR | Out-Null

$arch = if ($env:PROCESSOR_ARCHITECTURE -match "ARM64") { "arm64" } else { "x64" }
$indexPath = Join-Path $env:STARTKIT_CACHE_DIR "node-index.json"
Invoke-WebRequest -UseBasicParsing -Uri $env:STARTKIT_NODE_INDEX_URL -OutFile $indexPath
$items = Get-Content $indexPath -Raw | ConvertFrom-Json
$minMajor = Major $env:STARTKIT_MIN_VERSION
$selected = $items | Where-Object {
  (Major $_.version) -ge $minMajor -and $_.lts
} | Select-Object -First 1

if (-not $selected) {
  Emit @{ status = "error"; message = "Could not resolve a Node.js LTS version"; actions = @("install") }
  exit 0
}

$version = $selected.version
$zipName = "node-$version-win-$arch.zip"
$url = "$($env:STARTKIT_NODE_DIST_BASE)/$version/$zipName"
$zipPath = Join-Path $env:STARTKIT_CACHE_DIR $zipName
$extractDir = Join-Path $env:STARTKIT_CACHE_DIR "node-extract"
Remove-Item -Recurse -Force $extractDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zipPath
Expand-Archive -Force -Path $zipPath -DestinationPath $extractDir

$inner = Get-ChildItem $extractDir | Where-Object { $_.PSIsContainer } | Select-Object -First 1
Remove-Item -Recurse -Force $env:STARTKIT_NODE_DIR -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $env:STARTKIT_NODE_DIR | Out-Null
Copy-Item -Recurse -Force (Join-Path $inner.FullName "*") $env:STARTKIT_NODE_DIR

$node = Join-Path $env:STARTKIT_NODE_DIR "node.exe"
$installed = & $node --version
Emit @{ status = "ok"; version = $installed; path = $node; message = "Node.js installed"; actions = @() }

