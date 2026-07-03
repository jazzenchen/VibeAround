#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
SRC_ROOT="$(cd -- "${PACKAGE_ROOT}/../.." && pwd)"
REPO_ROOT="$(cd -- "${SRC_ROOT}/.." && pwd)"

TAG=""
SKIP_BUILD=0
NO_UPLOAD=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag)
      TAG="${2:-}"
      if [[ -z "${TAG}" ]]; then
        echo "upload-macos-release: --tag requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --no-upload)
      NO_UPLOAD=1
      shift
      ;;
    --help|-h)
      cat <<'USAGE'
Usage: upload-macos-release.sh [--tag va-vX.Y.Z] [--skip-build] [--no-upload]

Builds the Darwin arm64 VA CLI payload locally, signs the native binaries,
submits a ZIP payload to Apple notarization, then uploads it to the GitHub
release as VibeAround-CLI-darwin-arm64-<va-version>.zip.

Secrets are loaded from src/apple-sign.config when present, or from the
environment. Required variables:
  APPLE_SIGNING_IDENTITY
  APPLE_ID
  APPLE_APP_SPECIFIC_PASSWORD or APPLE_PASSWORD
  APPLE_TEAM_ID
USAGE
      exit 0
      ;;
    *)
      echo "upload-macos-release: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "upload-macos-release: macOS is required" >&2
  exit 2
fi

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "upload-macos-release: Apple Silicon arm64 host is required for darwin-arm64" >&2
  exit 2
fi

if [[ -f "${SRC_ROOT}/apple-sign.config" ]]; then
  set -a
  # shellcheck source=/dev/null
  . "${SRC_ROOT}/apple-sign.config"
  set +a
fi

export APPLE_PASSWORD="${APPLE_PASSWORD:-${APPLE_APP_SPECIFIC_PASSWORD:-}}"

missing=()
for key in APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
  if [[ -z "${!key:-}" ]]; then
    missing+=("${key}")
  fi
done

if (( ${#missing[@]} > 0 )); then
  echo "upload-macos-release: missing required signing/notarization env: ${missing[*]}" >&2
  exit 2
fi

for tool in node cargo codesign ditto xcrun gh; do
  if ! command -v "${tool}" >/dev/null 2>&1; then
    echo "upload-macos-release: required tool not found: ${tool}" >&2
    exit 2
  fi
done

VA_VERSION="$(node -p "require('${PACKAGE_ROOT}/package.json').version")"
TAG="${TAG:-va-v${VA_VERSION}}"

prepare_args=(
  "${PACKAGE_ROOT}/scripts/prepare-package.mjs"
  "--skip-web"
  "--platform" "darwin"
  "--arch" "arm64"
  "--sign"
)

if [[ "${SKIP_BUILD}" == "1" ]]; then
  prepare_args+=("--skip-build")
fi

node "${prepare_args[@]}"

NATIVE_DIR="${PACKAGE_ROOT}/bin/native/darwin-arm64"
DIST_DIR="${PACKAGE_ROOT}/.dist/macos-release"
PAYLOAD_ROOT="${DIST_DIR}/payload"
ASSET="${DIST_DIR}/VibeAround-CLI-darwin-arm64-${VA_VERSION}.zip"

rm -rf "${DIST_DIR}"
mkdir -p "${PAYLOAD_ROOT}/bin/native"
cp -R "${NATIVE_DIR}" "${PAYLOAD_ROOT}/bin/native/"

for binary in va-native va-tui va-launch vibearound-server; do
  codesign --verify --verbose=2 "${PAYLOAD_ROOT}/bin/native/darwin-arm64/${binary}"
done

(
  cd "${PAYLOAD_ROOT}"
  ditto -c -k --sequesterRsrc --keepParent bin "${ASSET}"
)

xcrun notarytool submit "${ASSET}" \
  --apple-id "${APPLE_ID}" \
  --password "${APPLE_PASSWORD}" \
  --team-id "${APPLE_TEAM_ID}" \
  --wait

if [[ "${NO_UPLOAD}" == "1" ]]; then
  echo "prepared notarized macOS CLI asset: ${ASSET}"
  exit 0
fi

(
  cd "${REPO_ROOT}"
  gh release upload "${TAG}" "${ASSET}" --clobber
)

echo "uploaded ${ASSET} to ${TAG}"
