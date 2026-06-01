#!/usr/bin/env sh
set -eu

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

mkdir -p "$STARTKIT_NODE_DIR" "$STARTKIT_CACHE_DIR"

node_os="darwin"
case "$(uname -m)" in
  arm64|aarch64) node_arch="arm64" ;;
  x86_64|amd64) node_arch="x64" ;;
  *) printf '{"status":"blocked","message":"Unsupported macOS architecture","actions":[]}\n'; exit 0 ;;
esac

index_file="$STARTKIT_CACHE_DIR/node-index.json"
curl -fsSL "${STARTKIT_NODE_INDEX_URL:-https://nodejs.org/dist/index.json}" -o "$index_file"

min_major="$(printf '%s' "${STARTKIT_MIN_VERSION:-22.0.0}" | sed 's/^v//' | cut -d. -f1)"
node_version="$(
  tr '{' '\n' < "$index_file" | awk -F'"' -v min_major="$min_major" '
    /"version"/ && /"lts":/ && !/"lts":false/ {
      for (i = 1; i <= NF; i++) {
        if ($i == "version") {
          version = $(i + 2)
          major = version
          sub(/^v/, "", major)
          sub(/\..*/, "", major)
          if ((major + 0) >= (min_major + 0)) {
            print version
            exit
          }
        }
      }
    }
  '
)"

if [ -z "$node_version" ]; then
  printf '{"status":"error","message":"Could not resolve a Node.js LTS version","actions":["install"]}\n'
  exit 0
fi

tarball="node-${node_version}-${node_os}-${node_arch}.tar.gz"
download_url="${STARTKIT_NODE_DIST_BASE:-https://nodejs.org/dist}/${node_version}/${tarball}"
tmp_file="$STARTKIT_CACHE_DIR/$tarball"
tmp_dir="$STARTKIT_CACHE_DIR/node-extract"
rm -rf "$tmp_dir"
mkdir -p "$tmp_dir"

curl -fL "$download_url" -o "$tmp_file"
tar -xzf "$tmp_file" -C "$tmp_dir" --strip-components=1
rm -rf "$STARTKIT_NODE_DIR"
mkdir -p "$STARTKIT_NODE_DIR"
cp -R "$tmp_dir"/. "$STARTKIT_NODE_DIR"/

version="$("$STARTKIT_NODE_DIR/bin/node" --version)"
printf '{"status":"ok","version":"%s","path":"%s","message":"Node.js installed","actions":[]}\n' \
  "$(json_escape "$version")" "$(json_escape "$STARTKIT_NODE_DIR/bin/node")"
