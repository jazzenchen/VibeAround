$ErrorActionPreference = "Stop"

function Emit($obj) {
  $obj | ConvertTo-Json -Compress
}

$git = Get-Command git -ErrorAction SilentlyContinue
if ($git) {
  $version = (& $git.Source --version 2>&1 | Select-Object -First 1) -join ""
  Emit @{ status = "ok"; version = $version; path = $git.Source; message = "Git is ready"; actions = @() }
  exit 0
}

$winget = Get-Command winget -ErrorAction SilentlyContinue
if ($winget) {
  & $winget.Source install --id Git.Git --source winget --accept-package-agreements --accept-source-agreements --silent
  $git = Get-Command git -ErrorAction SilentlyContinue
  if ($git) {
    $version = (& $git.Source --version 2>&1 | Select-Object -First 1) -join ""
    Emit @{ status = "ok"; version = $version; path = $git.Source; message = "Git installed"; actions = @() }
    exit 0
  }
}

Emit @{ status = "blocked"; message = "Git is not installed. Install Git for Windows, then run scan again."; actions = @() }

