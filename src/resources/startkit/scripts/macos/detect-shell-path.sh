#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

marker="# >>> VibeAround Startkit PATH >>>"
profiles="$HOME/.zprofile $HOME/.bash_profile $HOME/.profile"

for profile in $profiles; do
  if [ -f "$profile" ] && grep -Fq "$marker" "$profile"; then
    printf '{"status":"ok","path":"%s","message":"Shell PATH is configured","actions":[]}\n' \
      "$(json_escape "$profile")"
    exit 0
  fi
done

printf '{"status":"missing","message":"Shell PATH is not configured for VibeAround-managed tools","actions":["install"]}\n'
