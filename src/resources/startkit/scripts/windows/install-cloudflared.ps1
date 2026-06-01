$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

New-Item -ItemType Directory -Force -Path $env:STARTKIT_BIN_DIR, $env:STARTKIT_CACHE_DIR | Out-Null
$arch = if ($env:PROCESSOR_ARCHITECTURE -match "ARM64") { "arm64" } else { "amd64" }
$url = "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-$arch.exe"
$target = Join-Path $env:STARTKIT_BIN_DIR "cloudflared.exe"
Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $target
$version = (& $target --version 2>&1 | Select-Object -First 1) -join ""
Emit @{ status = "ok"; version = $version; path = $target; message = "cloudflared installed"; actions = @() }

