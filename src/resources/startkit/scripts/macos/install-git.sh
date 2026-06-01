#!/usr/bin/env sh
set -eu

if command -v git >/dev/null 2>&1; then
  path="$(command -v git)"
  version="$(git --version 2>&1 | head -n 1 || true)"
  printf '{"status":"ok","version":"%s","path":"%s","message":"Git is ready","actions":[]}\n' "$version" "$path"
  exit 0
fi

if command -v xcode-select >/dev/null 2>&1; then
  xcode-select --install >/dev/null 2>&1 || true
  printf '{"status":"blocked","message":"macOS Command Line Tools installer was opened. Complete it, then run scan again.","actions":["verify"]}\n'
  exit 0
fi

printf '{"status":"blocked","message":"Git is not installed. Install Apple Command Line Tools, then run scan again.","actions":[]}\n'

