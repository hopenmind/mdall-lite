#!/usr/bin/env bash
# Fetch a headless rendering engine for the given platform into ./chromium, so the
# General-converter PDF tier ships with a bundled engine on Linux and macOS.
#
# The engine source exposes stable, versioned, per-platform download URLs through
# a public JSON index, which is far more reproducible in CI than scraping a
# browser release page. Windows x64 uses its own stripped build instead
# (scripts/setup-engine.ps1); there is no ARM64-Windows build, so that target
# ships lean and renders PDF through the pure-Rust Typst tier.
#
# Usage: fetch-engine.sh <platform>   where <platform> is linux64 | mac-arm64 | mac-x64
set -euo pipefail

plat="${1:?usage: fetch-engine.sh <linux64|mac-arm64|mac-x64>}"
index="https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json"

url=$(curl -fsSL "$index" | jq -r --arg p "$plat" \
  '.channels.Stable.downloads.chrome[] | select(.platform==$p) | .url')
if [ -z "$url" ] || [ "$url" = "null" ]; then
  echo "error: no engine build for platform '$plat'" >&2
  exit 1
fi

echo "Downloading rendering engine ($plat)"
echo "  $url"
curl -fsSL "$url" -o cft.zip
unzip -q cft.zip

# The archive extracts to chrome-<platform>/ ; normalize it to ./chromium so the
# binary's find_bundled_engine() locates it next to the executable.
rm -rf chromium
mv "chrome-$plat" chromium
rm -f cft.zip

echo "Engine ready at ./chromium"
ls chromium
