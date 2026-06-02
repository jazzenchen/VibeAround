#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

git_status_json() {
  if ! command -v git >/dev/null 2>&1; then
    return 1
  fi

  git_path="$(command -v git)"
  set +e
  version_output="$(git --version 2>&1)"
  version_status=$?
  set -e
  version="$(printf '%s' "$version_output" | head -n 1)"
  if [ "$version_status" -eq 0 ] && ! printf '%s' "$version_output" | grep -Eqi 'xcode-select|developer tools (were )?not found|no developer tools'; then
    printf '{"status":"ok","version":"%s","path":"%s","message":"Git is ready","actions":[]}\n' \
      "$(json_escape "$version")" "$(json_escape "$git_path")"
    return 0
  fi

  return 1
}

find_clt_label() {
  softwareupdate -l 2>/dev/null | awk '
    /^[[:space:]]*\*/ {
      label = $0
      sub(/^[[:space:]]*\*[[:space:]]*/, "", label)
      sub(/^Label:[[:space:]]*/, "", label)
      pending = label
    }
    /Command Line Tools/ && pending != "" {
      print pending
      pending = ""
    }
  ' | sort -V | tail -n 1
}

install_clt_with_softwareupdate() {
  if ! command -v softwareupdate >/dev/null 2>&1; then
    return 1
  fi

  marker="/tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress"
  touch "$marker" 2>/dev/null || true
  trap 'rm -f /tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress' EXIT HUP INT TERM

  clt_label="$(find_clt_label || true)"
  if [ -z "$clt_label" ]; then
    rm -f "$marker"
    trap - EXIT HUP INT TERM
    return 1
  fi

  printf 'Installing Apple Command Line Tools: %s\n' "$clt_label"
  set +e
  softwareupdate --verbose --install "$clt_label" 2>&1
  install_status=$?
  set -e

  rm -f "$marker"
  trap - EXIT HUP INT TERM

  if [ "$install_status" -ne 0 ]; then
    return 1
  fi

  waited=0
  while [ "$waited" -lt 120 ]; do
    if xcode-select -p >/dev/null 2>&1 && git_status_json; then
      return 0
    fi
    sleep 5
    waited=$((waited + 5))
  done

  return 1
}

if git_status_json; then
  exit 0
fi

if install_clt_with_softwareupdate; then
  exit 0
fi

if command -v xcode-select >/dev/null 2>&1; then
  xcode-select --install >/dev/null 2>&1 || true
  printf '{"status":"blocked","message":"Apple Command Line Tools installer was opened. Complete it, then run scan again.","actions":["verify"]}\n'
  exit 0
fi

printf '{"status":"blocked","message":"Git is not installed and Apple Command Line Tools could not be installed automatically.","actions":[]}\n'
