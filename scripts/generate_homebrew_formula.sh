#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "Usage: $0 <version-without-v> <tap-repo-path>"
  echo "Example: $0 0.1.9 ~/Dev/homebrew-masix"
  exit 1
fi

VERSION="$1"
TAP_DIR="$2"
TAG="v${VERSION}"
URL="https://github.com/DioNanos/masix/archive/refs/tags/${TAG}.tar.gz"
FORMULA_DIR="${TAP_DIR}/Formula"
FORMULA_PATH="${FORMULA_DIR}/masix.rb"

mkdir -p "${FORMULA_DIR}"

TMP_FILE="$(mktemp)"
cleanup() {
  rm -f "${TMP_FILE}"
}
trap cleanup EXIT

echo "Fetching ${URL}"
curl -fsSL "${URL}" -o "${TMP_FILE}"

if command -v sha256sum >/dev/null 2>&1; then
  SHA256="$(sha256sum "${TMP_FILE}" | awk '{print $1}')"
else
  SHA256="$(shasum -a 256 "${TMP_FILE}" | awk '{print $1}')"
fi

cat > "${FORMULA_PATH}" <<EOF
class Masix < Formula
  desc "Rust-first messaging automation runtime (Telegram/MCP/Cron)"
  homepage "https://github.com/DioNanos/masix"
  url "${URL}"
  sha256 "${SHA256}"
  license "MIT"

  depends_on "rust" => :build
  depends_on "pkg-config" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/masix-cli")
  end

  test do
    assert_match "MIT Messaging Agent", shell_output("#{bin}/masix --help")
  end
end
EOF

echo "Wrote formula: ${FORMULA_PATH}"
echo "Version: ${VERSION}"
echo "SHA256: ${SHA256}"
