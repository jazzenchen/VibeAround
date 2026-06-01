#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

package="${STARTKIT_NPM_PACKAGE:?missing STARTKIT_NPM_PACKAGE}"
program="${STARTKIT_PROGRAM:?missing STARTKIT_PROGRAM}"
mkdir -p "$STARTKIT_NPM_PREFIX"

npm_bin="npm"
if [ -x "$STARTKIT_NODE_DIR/bin/npm" ]; then
  npm_bin="$STARTKIT_NODE_DIR/bin/npm"
fi

"$npm_bin" install --global --prefix "$STARTKIT_NPM_PREFIX" --registry "${STARTKIT_NPM_REGISTRY:-https://registry.npmjs.org}" "$package"

if ! command -v "$program" >/dev/null 2>&1; then
  export PATH="$STARTKIT_NPM_PREFIX/bin:$STARTKIT_NODE_DIR/bin:$PATH"
fi

path="$(command -v "$program" 2>/dev/null || true)"
if [ -z "$path" ]; then
  printf '{"status":"error","message":"Installed %s but %s is still not on PATH","actions":["repair"]}\n' \
    "$(json_escape "$package")" "$(json_escape "$program")"
  exit 0
fi

version="$("$program" "${STARTKIT_VERSION_ARG:---version}" 2>&1 | head -n 1 || true)"
printf '{"status":"ok","version":"%s","path":"%s","message":"%s installed","actions":[]}\n' \
  "$(json_escape "$version")" "$(json_escape "$path")" "$(json_escape "$program")"

