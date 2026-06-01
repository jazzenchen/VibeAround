#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

program="${STARTKIT_PROGRAM:?missing STARTKIT_PROGRAM}"
version_arg="${STARTKIT_VERSION_ARG:---version}"

if ! command -v "$program" >/dev/null 2>&1; then
  printf '{"status":"missing","message":"%s was not found in PATH","actions":["install"]}\n' "$(json_escape "$program")"
  exit 0
fi

path="$(command -v "$program")"
version="$("$program" "$version_arg" 2>&1 | head -n 1 || true)"
printf '{"status":"ok","version":"%s","path":"%s","message":"%s is ready","actions":[]}\n' \
  "$(json_escape "$version")" "$(json_escape "$path")" "$(json_escape "$program")"

