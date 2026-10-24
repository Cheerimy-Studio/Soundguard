#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
test_dir="$(mktemp -d)"
trap 'rm -rf "$test_dir"' EXIT
for module in audio_core security config_file; do
  rustc --edition 2021 --test "client/common/$module.rs" -o "$test_dir/$module"
  "$test_dir/$module"
done
