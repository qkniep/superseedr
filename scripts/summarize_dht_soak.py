#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2025 The superseedr Contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Summarize DHT soak status samples and planner trace logs.

The script is intentionally read-only unless cleanup flags are provided. It
expects status samples as JSON lines matching `superseedr --json status` derived
fields, and an optional app log containing `superseedr::dht_planner` traces.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from statistics import mean
from typing import Any


START_LOOKUP_RE = re.compile(r'stage="emit" effect="start_lookup"')
PEERS_RECEIVED_RE = re.compile(r'action="peers_received"')
DRAIN_RECORDED_RE = re.compile(r'stage="emit" effect="drain_peers_recorded"')
DRAIN_FINALIZED_RE = re.compile(r'stage="apply" effect="drain_finalized"')
FIELD_RE_TEMPLATE = r"{field}=Some\((\d+)\)"
CLASS_RE = re.compile(r"demand_class=Some\(([^)]+)\)")
SLICE_CLASS_RE = re.compile(r"slice_class=Some\(([^)]+)\)")


def load_samples(path: Path) -> list[dict[str, Any]]:
    samples: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8-sig") as handle:
        for line_number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                samples.append(json.loads(line))
            except json.JSONDecodeError as error:
                raise SystemExit(f"{path}:{line_number}: invalid JSON: {error}") from error
    return samples


def lines_in_window(path: Path, start: str | None, end: str | None) -> list[str]:
    lines: list[str] = []
    with path.open("r", encoding="utf-8-sig", errors="replace") as handle:
        for line in handle:
            if start is not None and line < start:
                continue
            if end is not None and line > end:
                continue
            lines.append(line.rstrip("\n"))
    return lines


def sum_field(lines: list[str], field: str) -> tuple[int, int]:
    pattern = re.compile(FIELD_RE_TEMPLATE.format(field=re.escape(field)))
    total = 0
    maximum = 0
    for line in lines:
        match = pattern.search(line)
        if match is None:
            continue
        value = int(match.group(1))
        total += value
        maximum = max(maximum, value)
    return total, maximum


def summarize_samples(samples: list[dict[str, Any]]) -> dict[str, Any]:
    if not samples:
        return {
            "status_samples": 0,
            "sample_errors": 0,
        }

    routes = [int(sample["routes"]) for sample in samples]
    queries = [int(sample["q"]) for sample in samples]
    lookups = [int(sample["lookups"]) for sample in samples]
    bootstrap = [int(sample["bootstrap"]) for sample in samples]
    warnings = [sample for sample in samples if sample.get("warning") is not None]

    return {
        "status_samples": len(samples),
        "sample_errors": 0,
        "runtime_first": int(samples[0]["runtime_s"]),
        "runtime_last": int(samples[-1]["runtime_s"]),
        "enabled_all": all(bool(sample["enabled"]) for sample in samples),
        "routes_avg": round(mean(routes), 1),
        "routes_min": min(routes),
        "routes_max": max(routes),
        "q_avg": round(mean(queries), 1),
        "q_max": max(queries),
        "q_last": queries[-1],
        "lookups_avg": round(mean(lookups), 1),
        "lookups_max": max(lookups),
        "bootstrap_min": min(bootstrap),
        "bootstrap_last": bootstrap[-1],
        "status_warnings": len(warnings),
    }


def summarize_log(lines: list[str]) -> dict[str, Any]:
    planner = [line for line in lines if "superseedr::dht_planner" in line]
    actor = [line for line in lines if "superseedr::dht_actor" in line]
    starts = [line for line in planner if START_LOOKUP_RE.search(line)]

    classes = {
        "AwaitingMetadata": 0,
        "NoConnectedPeers": 0,
        "RoutineRefresh": 0,
        "Other": 0,
    }
    for line in starts:
        class_match = CLASS_RE.search(line) or SLICE_CLASS_RE.search(line)
        class_name = class_match.group(1) if class_match else "Other"
        classes[class_name if class_name in classes else "Other"] += 1

    peer_actions = [line for line in planner if PEERS_RECEIVED_RE.search(line)]
    drain_recorded = [line for line in planner if DRAIN_RECORDED_RE.search(line)]
    drain_finalized = [line for line in planner if DRAIN_FINALIZED_RE.search(line)]
    peers_delivered, peers_delivered_max = sum_field(peer_actions, "peer_count")
    drain_unique_added, drain_unique_added_max = sum_field(drain_recorded, "unique_added")
    drain_finalized_unique, _ = sum_field(drain_finalized, "unique_peers")

    return {
        "run_lines": len(lines),
        "actor_lines": len(actor),
        "planner_lines": len(planner),
        "selected_launches": len(starts),
        "awaiting_metadata": classes["AwaitingMetadata"],
        "no_peer": classes["NoConnectedPeers"],
        "routine": classes["RoutineRefresh"],
        "other_launch": classes["Other"],
        "launch_failures": sum("launch_failed" in line for line in planner),
        "launch_skipped": sum("launch_skipped" in line for line in planner),
        "peer_batches_dropped": sum("drop_batch" in line for line in planner),
        "peers_received_events": len(peer_actions),
        "peers_delivered": peers_delivered,
        "peers_delivered_max_batch": peers_delivered_max,
        "drain_peers_recorded_events": len(drain_recorded),
        "drain_unique_added": drain_unique_added,
        "drain_unique_added_max": drain_unique_added_max,
        "drain_finalized_events": len(drain_finalized),
        "drain_finalized_unique_sum": drain_finalized_unique,
        "errors": sum("ERROR" in line for line in lines),
        "warnings": sum(" WARN " in line for line in lines),
        "service_actor": sum('domain="service"' in line for line in actor),
        "lifecycle_actor": sum('domain="lifecycle"' in line for line in actor),
        "demand_actor": sum('domain="demand_command"' in line for line in actor),
        "runtime_actor": sum('domain="runtime_command"' in line for line in actor),
    }


def cleanup(args: argparse.Namespace) -> dict[str, Any]:
    result: dict[str, Any] = {}
    if args.trim_log_to_length is not None:
        if args.log is None:
            raise SystemExit("--trim-log-to-length requires --log")
        before = args.log.stat().st_size if args.log.exists() else 0
        with args.log.open("r+b") as handle:
            handle.truncate(args.trim_log_to_length)
        result["log_bytes_before_cleanup"] = before
        result["log_bytes_after_cleanup"] = args.log.stat().st_size
        result["log_bytes_removed"] = before - result["log_bytes_after_cleanup"]

    removed: list[str] = []
    for path in args.delete:
        if path.exists():
            path.unlink()
            removed.append(str(path))
    result["deleted_files"] = removed
    return result


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=Path, help="JSONL status sample file")
    parser.add_argument("--log", type=Path, help="app log containing DHT traces")
    parser.add_argument("--start", help="inclusive ISO timestamp prefix for log window")
    parser.add_argument("--end", help="exclusive ISO timestamp prefix for log window")
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    parser.add_argument(
        "--trim-log-to-length",
        type=int,
        help="truncate --log back to this byte length after summarizing",
    )
    parser.add_argument(
        "--delete",
        type=Path,
        action="append",
        default=[],
        help="delete generated artifact after summarizing; can be repeated",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    summary: dict[str, Any] = {}

    if args.samples is not None:
        summary.update(summarize_samples(load_samples(args.samples)))
    if args.log is not None:
        summary.update(summarize_log(lines_in_window(args.log, args.start, args.end)))
    if args.trim_log_to_length is not None or args.delete:
        summary["cleanup"] = cleanup(args)

    if args.json:
        print(json.dumps(summary, indent=2, sort_keys=True))
        return

    print(f"Status samples: {summary.get('status_samples', 0)}")
    if "runtime_last" in summary:
        print(
            "Runtime: "
            f"{summary['runtime_first']}s..{summary['runtime_last']}s, "
            f"enabled_all={summary['enabled_all']}"
        )
        print(
            "Routes: "
            f"avg {summary['routes_avg']}, "
            f"range {summary['routes_min']}..{summary['routes_max']}"
        )
        print(
            "Query pressure: "
            f"avg {summary['q_avg']}, max {summary['q_max']}, last {summary['q_last']}"
        )
    if "selected_launches" in summary:
        launches = summary["selected_launches"]
        print(f"Selected launches: {launches}")
        if launches:
            print(
                "Launch mix: "
                f"{summary['no_peer'] / launches:.1%} no-peer, "
                f"{summary['routine'] / launches:.1%} routine, "
                f"{summary['awaiting_metadata'] / launches:.1%} awaiting-metadata"
            )
        print(
            "Failures/skips/drops: "
            f"{summary['launch_failures']}/"
            f"{summary['launch_skipped']}/"
            f"{summary['peer_batches_dropped']}"
        )
        print(f"Peers delivered: {summary['peers_delivered']}")
        print(f"Drain unique added: {summary['drain_unique_added']}")
        print(f"Trace errors/warnings: {summary['errors']}/{summary['warnings']}")
    if "cleanup" in summary:
        print(f"Cleanup: {json.dumps(summary['cleanup'], sort_keys=True)}")


if __name__ == "__main__":
    main()
