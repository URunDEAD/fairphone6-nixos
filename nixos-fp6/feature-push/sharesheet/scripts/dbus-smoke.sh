#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# This test creates its own bus if one was not explicitly supplied by the build.
cargo test --locked --test session -- --nocapture
