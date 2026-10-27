#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
exec dbus-run-session -- env NETPOLICY_TEST_BUS=private cargo test --test dbus -- --ignored --test-threads=1
