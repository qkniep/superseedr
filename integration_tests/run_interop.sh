#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-all}"
SCENARIO="${2:-${INTEROP_SCENARIO:-superseedr_to_superseedr}}"
TIMEOUT="${INTEROP_TIMEOUT_SECS:-300}"

python3 -m integration_tests.harness.run \
  --scenario "$SCENARIO" \
  --mode "$MODE" \
  --timeout-secs "$TIMEOUT"
