#!/usr/bin/env bash
set -euo pipefail

export RUN_CLUSTER_CLI="${RUN_CLUSTER_CLI:-1}"
python3 -m integration_tests.cluster_cli.run "$@"
