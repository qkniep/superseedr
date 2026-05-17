#!/usr/bin/env bash
set -euo pipefail

SCENARIO="${1:-${LIBTORRENT_LAB_SCENARIO:-basic_ul_dl}}"
TIMEOUT="${LIBTORRENT_LAB_TIMEOUT_SECS:-120}"

python3 -m integration_tests.libtorrent_lab.run \
  --scenario "$SCENARIO" \
  --timeout-secs "$TIMEOUT"
