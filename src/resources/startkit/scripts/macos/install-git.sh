#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

if command -v git >/dev/null 2>&1; then
  path="$(command -v git)"
  set +e
  version_output="$(git --version 2>&1)"
  version_status=$?
  set -e
  version="$(printf '%s' "$version_output" | head -n 1)"
  if [ "$version_status" -eq 0 ] && ! printf '%s' "$version_output" | grep -Eqi 'xcode-select|developer tools (were )?not found|no developer tools'; then
    printf '{"status":"ok","version":"%s","path":"%s","message":"Git is ready","actions":[]}\n' \
      "$(json_escape "$version")" "$(json_escape "$path")"
    exit 0
  fi
fi

if command -v xcode-select >/dev/null 2>&1; then
  xcode-select --install >/dev/null 2>&1 || true
  printf '{"status":"blocked","message":"macOS Command Line Tools installer was opened. Complete it, then run scan again.","actions":["verify"]}\n'
  exit 0
fi

printf '{"status":"blocked","message":"Git is not installed. Install Apple Command Line Tools, then run scan again.","actions":[]}\n'
