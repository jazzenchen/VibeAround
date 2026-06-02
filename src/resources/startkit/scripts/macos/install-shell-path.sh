#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

if [ "${STARTKIT_TOOLCHAIN_MODE:-auto}" = "system" ]; then
  printf '{"status":"blocked","message":"System-only mode is selected, so managed PATH entries were not written.","actions":[]}\n'
  exit 0
fi

start_marker="# >>> VibeAround Startkit PATH >>>"
end_marker="# <<< VibeAround Startkit PATH <<<"

write_profile() {
  profile="$1"
  tmp="${profile}.vibearound.$$"
  mkdir -p "$(dirname "$profile")"

  if [ -f "$profile" ]; then
    awk -v start="$start_marker" -v end="$end_marker" '
      $0 == start { skip = 1; next }
      $0 == end { skip = 0; next }
      skip != 1 { print }
    ' "$profile" > "$tmp"
  else
    : > "$tmp"
  fi

  {
    printf '\n%s\n' "$start_marker"
    printf '# Added by VibeAround Startkit. Remove this block to undo.\n'
    printf 'export PATH="$HOME/.vibearound/bin:$HOME/.vibearound/runtime/node/bin:$HOME/.vibearound/runtime/node:$HOME/.vibearound/npm-global/bin:$HOME/.vibearound/npm-global:$PATH"\n'
    printf '%s\n' "$end_marker"
  } >> "$tmp"

  mv "$tmp" "$profile"
}

write_profile "$HOME/.zprofile"
write_profile "$HOME/.bash_profile"
write_profile "$HOME/.profile"

printf '{"status":"ok","path":"%s","message":"Shell PATH configured for VibeAround-managed tools","actions":[]}\n' \
  "$(json_escape "$HOME/.zprofile")"
