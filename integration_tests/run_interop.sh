#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-all}"
TIMEOUT="${INTEROP_TIMEOUT_SECS:-300}"

python3 -m integration_tests.harness.run \
  --scenario superseedr_to_superseedr \
  --mode "$MODE" \
  --timeout-secs "$TIMEOUT"
