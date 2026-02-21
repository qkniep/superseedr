from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

from integration_tests.harness.config import HarnessDefaults, resolve_paths
from integration_tests.harness.scenarios import (
    qbittorrent_to_superseedr,
    superseedr_to_transmission,
    superseedr_to_qbittorrent,
    superseedr_to_superseedr,
    transmission_to_superseedr,
)

ALL_MODES = ("v1", "v2", "hybrid")
SCENARIOS = {
    "superseedr_to_superseedr": superseedr_to_superseedr,
    "superseedr_to_qbittorrent": superseedr_to_qbittorrent,
    "qbittorrent_to_superseedr": qbittorrent_to_superseedr,
    "superseedr_to_transmission": superseedr_to_transmission,
    "transmission_to_superseedr": transmission_to_superseedr,
}


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Run dockerized interop integration harness")
    p.add_argument(
        "--scenario",
        default="superseedr_to_superseedr",
        choices=sorted(SCENARIOS.keys()),
    )
    p.add_argument("--mode", default="all", choices=["all", *ALL_MODES])
    p.add_argument("--timeout-secs", type=int, default=300)
    p.add_argument("--run-id", default="")
    p.add_argument("--skip-generation", action="store_true")
    return p.parse_args()


def main() -> int:
    args = parse_args()
    paths = resolve_paths()
    defaults = HarnessDefaults()
    scenario_mod = SCENARIOS[args.scenario]

    run_id = args.run_id or time.strftime("run_%Y%m%d_%H%M%S")
    run_root = paths.artifacts_root / "runs" / run_id
    run_root.mkdir(parents=True, exist_ok=True)

    torrents_root = paths.torrents_root
    if not args.skip_generation:
        torrents_root = scenario_mod.generate_fixtures_and_torrents(paths.root, defaults.announce_url)

    scenario_supported_modes = tuple(getattr(scenario_mod, "SUPPORTED_MODES", ALL_MODES))
    modes = list(scenario_supported_modes) if args.mode == "all" else [args.mode]

    if args.mode != "all" and args.mode not in scenario_supported_modes:
        supported = ",".join(scenario_supported_modes)
        raise SystemExit(
            f"Scenario {args.scenario} does not support mode={args.mode}. "
            f"Supported modes: {supported}"
        )
    results = []

    for mode in modes:
        result = scenario_mod.run_mode(
            mode=mode,
            timeout_secs=args.timeout_secs,
            run_root=run_root,
            harness_paths=paths,
            defaults=defaults,
            torrents_root=torrents_root,
        )
        results.append(result)

    summary = {
        "run_id": run_id,
        "scenario": args.scenario,
        "modes": [r.mode for r in results],
        "ok": all(r.ok for r in results),
        "results": [
            {
                "mode": r.mode,
                "ok": r.ok,
                "duration_secs": round(r.duration_secs, 3),
                "missing": r.missing,
                "mismatched": r.mismatched,
                "extra": r.extra,
            }
            for r in results
        ],
    }
    (run_root / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")

    if summary["ok"]:
        print(f"HARNESS_RESULT PASS run_id={run_id} artifacts={run_root}")
        return 0

    print(f"HARNESS_RESULT FAIL run_id={run_id} artifacts={run_root}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
