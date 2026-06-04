#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

program="${STARTKIT_PROGRAM:?missing STARTKIT_PROGRAM}"
version_arg="${STARTKIT_VERSION_ARG:---version}"
mode="${STARTKIT_TOOLCHAIN_MODE:-auto}"
candidate=""

package_name() {
  package="$1"
  case "$package" in
    @*/*)
      scope="${package%%/*}"
      rest="${package#*/}"
      case "$rest" in
        *@*) printf '%s/%s' "$scope" "${rest%@*}" ;;
        *) printf '%s' "$package" ;;
      esac
      ;;
    *@*)
      printf '%s' "${package%@*}"
      ;;
    *)
      printf '%s' "$package"
      ;;
  esac
}

requested_package_version() {
  package="$1"
  case "$package" in
    @*/*)
      rest="${package#*/}"
      if [ "$rest" != "${rest%@*}" ]; then
        printf '%s' "${rest##*@}"
      fi
      ;;
    *@*)
      printf '%s' "${package##*@}"
      ;;
  esac
}

node_bin() {
  if [ -x "${STARTKIT_NODE_DIR:-}/bin/node" ]; then
    printf '%s' "$STARTKIT_NODE_DIR/bin/node"
  elif command -v node >/dev/null 2>&1; then
    command -v node
  fi
}

npm_bin() {
  if [ -x "${STARTKIT_NODE_DIR:-}/bin/npm" ]; then
    printf '%s' "$STARTKIT_NODE_DIR/bin/npm"
  elif command -v npm >/dev/null 2>&1; then
    command -v npm
  fi
}

local_package_version() {
  package="$(package_name "$1")"
  node="$(node_bin || true)"
  [ -n "$node" ] || return 1
  "$node" - "$package" "${STARTKIT_NPM_PREFIX:-}" <<'NODE'
const fs = require("fs");
const path = require("path");
const name = process.argv[2];
const prefix = process.argv[3];
const roots = [
  path.join(prefix, "lib", "node_modules"),
  path.join(prefix, "node_modules"),
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
NODE
}

latest_package_version() {
  package="$(package_name "$1")"
  npm="$(npm_bin || true)"
  [ -n "$npm" ] || return 1
  "$npm" view "$package" version --registry "${STARTKIT_NPM_REGISTRY:-https://registry.npmjs.org}" 2>/dev/null | tail -n 1
}

is_managed_candidate() {
  case "$candidate" in
    "${STARTKIT_NPM_PREFIX:-}"/*|"${STARTKIT_BIN_DIR:-}"/*) return 0 ;;
    *) return 1 ;;
  esac
}

if [ "$mode" != "system" ] && [ "${STARTKIT_ITEM_MANAGED:-false}" = "true" ]; then
  if [ -n "${STARTKIT_NPM_PACKAGE:-}" ] && [ -x "${STARTKIT_NPM_PREFIX:-}/bin/$program" ]; then
    candidate="$STARTKIT_NPM_PREFIX/bin/$program"
  elif [ -x "${STARTKIT_BIN_DIR:-}/$program" ]; then
    candidate="$STARTKIT_BIN_DIR/$program"
  fi
fi

if [ -z "$candidate" ] && [ "$mode" != "managed" ] && command -v "$program" >/dev/null 2>&1; then
  candidate="$(command -v "$program")"
fi

if [ -z "$candidate" ]; then
  printf '{"status":"missing","message":"%s was not found in PATH","actions":["install"]}\n' "$(json_escape "$program")"
  exit 0
fi

path="$candidate"
set +e
version_output="$("$candidate" "$version_arg" 2>&1)"
version_status=$?
set -e
version="$(printf '%s' "$version_output" | head -n 1)"

if [ "$version_status" -ne 0 ] || printf '%s' "$version_output" | grep -Eqi 'xcode-select|developer tools (were )?not found|no developer tools'; then
  printf '{"status":"missing","version":"%s","path":"%s","message":"%s is present but not usable","actions":["install"]}\n' \
    "$(json_escape "$version")" "$(json_escape "$path")" "$(json_escape "$program")"
  exit 0
fi

if [ -n "${STARTKIT_NPM_PACKAGE:-}" ] && is_managed_candidate; then
  local_version="$(local_package_version "$STARTKIT_NPM_PACKAGE" 2>/dev/null || true)"
  requested_version="$(requested_package_version "$STARTKIT_NPM_PACKAGE" || true)"
  if [ -n "$requested_version" ]; then
    desired_version="$requested_version"
  else
    desired_version="$(latest_package_version "$STARTKIT_NPM_PACKAGE" 2>/dev/null || true)"
  fi
  if [ -n "$local_version" ] && [ -n "$desired_version" ] && [ "$local_version" != "$desired_version" ]; then
    printf '{"status":"outdated","version":"%s","path":"%s","message":"%s %s is below the latest available version %s","actions":["install"]}\n' \
      "$(json_escape "$version")" "$(json_escape "$path")" "$(json_escape "$program")" "$(json_escape "$local_version")" "$(json_escape "$desired_version")"
    exit 0
  fi
fi

printf '{"status":"ok","version":"%s","path":"%s","message":"%s is ready","actions":[]}\n' \
  "$(json_escape "$version")" "$(json_escape "$path")" "$(json_escape "$program")"
