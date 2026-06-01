#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

mkdir -p "$STARTKIT_BIN_DIR" "$STARTKIT_CACHE_DIR"
case "$(uname -m)" in
  arm64|aarch64) arch="arm64" ;;
  x86_64|amd64) arch="amd64" ;;
  *) printf '{"status":"blocked","message":"Unsupported macOS architecture","actions":[]}\n'; exit 0 ;;
esac

url="https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-${arch}.tgz"
archive="$STARTKIT_CACHE_DIR/cloudflared-darwin-${arch}.tgz"
tmp_dir="$STARTKIT_CACHE_DIR/cloudflared"
rm -rf "$tmp_dir"
mkdir -p "$tmp_dir"
curl -fL "$url" -o "$archive"
tar -xzf "$archive" -C "$tmp_dir"
install -m 0755 "$tmp_dir/cloudflared" "$STARTKIT_BIN_DIR/cloudflared"

version="$("$STARTKIT_BIN_DIR/cloudflared" --version 2>&1 | head -n 1 || true)"
printf '{"status":"ok","version":"%s","path":"%s","message":"cloudflared installed","actions":[]}\n' \
  "$(json_escape "$version")" "$(json_escape "$STARTKIT_BIN_DIR/cloudflared")"

