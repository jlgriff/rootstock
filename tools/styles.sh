#!/bin/sh
set -eu
cd -- "$(dirname -- "$0")/.."

version=4.3.4
case "$(uname -s):$(uname -m)" in
  Darwin:arm64) platform=macos-arm64; checksum=a516a11b439bfbb5ecd4a2d13b75e6cdcc284dc710aed8731e31d2e5c03895a4 ;;
  Darwin:x86_64) platform=macos-x64; checksum=7ec771593a1fb8bbd204f70429361b0158f7989e4136bc68d04513bf543a15ba ;;
  Linux:aarch64|Linux:arm64) platform=linux-arm64; checksum=f6d7baae1d350ed3a2defe98d0b5cc58e37400c0173b2e38d42f690ccc4bbfe1 ;;
  Linux:x86_64) platform=linux-x64; checksum=149a8ece5b855587a9c10de6da9dcbcfad0e47c4ed5dca219f721b3eae39d817 ;;
  *) echo 'Tailwind asset builds support macOS and glibc Linux on ARM64 or x86-64.' >&2; exit 1 ;;
esac

binary="target/tools/tailwindcss-$version-$platform"
if [ ! -x "$binary" ]; then
  mkdir -p target/tools
  download_file=$(mktemp "$binary.XXXXXX")
  trap 'rm -f "$download_file"' EXIT
  curl --fail --location --silent --show-error \
    "https://github.com/tailwindlabs/tailwindcss/releases/download/v$version/tailwindcss-$platform" \
    --output "$download_file"
  if command -v sha256sum >/dev/null 2>&1; then
    actual_checksum=$(sha256sum "$download_file" | cut -d ' ' -f 1)
  else
    actual_checksum=$(shasum -a 256 "$download_file" | cut -d ' ' -f 1)
  fi
  if [ "$actual_checksum" != "$checksum" ]; then
    echo 'Tailwind download checksum does not match the pinned release.' >&2
    exit 1
  fi
  chmod +x "$download_file"
  mv "$download_file" "$binary"
  trap - EXIT
fi

output=src/dashboard.css
if [ "${1-}" = --check ]; then
  shift
  output=$(mktemp)
  trap 'rm -f "$output"' EXIT
fi
"$binary" --input styles/dashboard.css --output "$output" --minify "$@"
if [ "$output" != src/dashboard.css ] && ! cmp -s "$output" src/dashboard.css; then
  echo 'Dashboard CSS is stale. Run ./tools/styles.sh and include src/dashboard.css.' >&2
  exit 1
fi
