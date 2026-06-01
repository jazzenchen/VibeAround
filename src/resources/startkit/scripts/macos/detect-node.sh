#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

version_major() {
  printf '%s' "$1" | sed 's/^v//' | cut -d. -f1
}

min_major="$(version_major "${STARTKIT_MIN_VERSION:-22.0.0}")"
candidate=""

if [ "${STARTKIT_TOOLCHAIN_MODE:-auto}" != "system" ] && [ -x "${STARTKIT_NODE_DIR:-}/bin/node" ]; then
  candidate="$STARTKIT_NODE_DIR/bin/node"
elif [ "${STARTKIT_TOOLCHAIN_MODE:-auto}" != "managed" ] && command -v node >/dev/null 2>&1; then
  candidate="$(command -v node)"
fi

if [ -z "$candidate" ]; then
  if [ "${STARTKIT_TOOLCHAIN_MODE:-auto}" = "managed" ]; then
    printf '{"status":"missing","message":"Managed Node.js was not found","actions":["install"]}\n'
  else
    printf '{"status":"missing","message":"Node.js was not found","actions":["install"]}\n'
  fi
  exit 0
fi

version="$("$candidate" --version 2>/dev/null || true)"
major="$(version_major "$version")"
if [ -z "$major" ] || [ "$major" -lt "$min_major" ] 2>/dev/null; then
  printf '{"status":"outdated","version":"%s","path":"%s","message":"Node.js %s is below the required version","actions":["install"]}\n' \
    "$(json_escape "$version")" "$(json_escape "$candidate")" "$(json_escape "$version")"
  exit 0
fi

printf '{"status":"ok","version":"%s","path":"%s","message":"Node.js is ready","actions":[]}\n' \
  "$(json_escape "$version")" "$(json_escape "$candidate")"
