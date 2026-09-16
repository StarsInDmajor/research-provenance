#!/usr/bin/env python3
"""Run the frozen Phase 1 RP acceptance operations with bounded GNU time measurements."""

from __future__ import annotations

import argparse
import csv
from datetime import date
import hashlib
import json
import math
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from statistics import median
from typing import Any

AS_OF = "2027-01-01T00:00:00Z"
OPERATIONS = (
    "parse-and-schema-validate",
    "whole-project-semantic-validate",
    "build-in-memory-projection",
    "overview",
    "show-exact-id",
    "history-logical-id",
    "query-kind-and-thread",
    "chain-validate",
    "access-explain",
    "export-check",
)
INTERACTIVE = frozenset(OPERATIONS[3:])


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=("smoke", "workstation", "stress"), required=True)
    parser.add_argument("--project", type=Path, required=True)
    parser.add_argument("--rp", type=Path, required=True)
    parser.add_argument("--output-json", type=Path, required=True)
    parser.add_argument("--output-csv", type=Path, required=True)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--measurements", type=int, default=7)
    parser.add_argument("--deadline-seconds", type=int, default=60)
    parser.add_argument(
        "--time-binary",
        type=Path,
        default=Path("/run/current-system/sw/bin/time"),
    )
    args = parser.parse_args()
    if args.warmups < 0 or args.measurements < 1 or args.deadline_seconds < 1:
        parser.error("warmups must be nonnegative; measurements and deadline must be positive")
    return args


def sha256_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def verify_corpus(root: Path, manifest: dict[str, Any]) -> None:
    """Verify the frozen generated tree outside timing, before and after a run.

    Generated entry paths and digests are ASCII strings only, so sorted compact
    JSON is exactly JCS for this restricted manifest-entry shape (not general JCS).
    """
    entries = manifest["sorted_path_sha256_entries"]
    paths = []
    for entry in entries:
        if set(entry) != {"path", "sha256"}:
            raise RuntimeError("invalid manifest entry shape")
        relative, digest = entry["path"], entry["sha256"]
        if (not isinstance(relative, str) or not relative.isascii()
                or not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)):
            raise RuntimeError("invalid generated manifest entry")
        path = Path(relative)
        if path.is_absolute() or ".." in path.parts or path.as_posix() != relative:
            raise RuntimeError("manifest path is not canonical and contained")
        paths.append(relative)
    if paths != sorted(set(paths)):
        raise RuntimeError("manifest entries must be sorted and unique")
    encoded = json.dumps(entries, sort_keys=True, separators=(",", ":")).encode("ascii")
    if sha256_bytes(encoded) != manifest["aggregate_sha256"]:
        raise RuntimeError("manifest aggregate digest mismatch")
    actual = set()
    for directory, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(directory) / name
            if path.is_symlink():
                raise RuntimeError("generated corpus contains a symlink")
        for name in files:
            path = Path(directory) / name
            if not path.is_file():
                raise RuntimeError("generated corpus contains a nonregular file")
            relative = path.relative_to(root).as_posix()
            if relative != "scale-manifest.json":
                actual.add(relative)
    if actual != set(paths):
        raise RuntimeError("generated corpus file set differs from manifest")
    for entry in entries:
        if sha256_bytes((root / entry["path"]).read_bytes()) != entry["sha256"]:
            raise RuntimeError("generated corpus file digest mismatch")


def selected_ids(root: Path, manifest: dict[str, Any]) -> dict[str, str]:
    paths = [entry["path"] for entry in manifest["sorted_path_sha256_entries"]]

    def selected(prefix: str) -> tuple[str, dict[str, Any]]:
        relative = next(path for path in paths if path.startswith(prefix))
        value = read_json(root / relative)
        return relative, value

    _, question = selected(".research/records/questions/question-000000--")
    _, thread = selected(".research/threads/thread-000000--")
    _, chain = selected(".research/claim-chains/chain-000000--")
    return {
        "show_id": question["id"],
        "history_logical_id": question["logical_id"],
        "thread_id": thread["id"],
        "chain_id": chain["id"],
        "chain_profile": chain["validation_policy"]["profile"],
        "access_id": question["id"],
        "export_id": question["id"],
        "query_kind": question["kind"],
    }


def command_arguments(operation: str, root: Path, ids: dict[str, str]) -> list[str]:
    project = str(root)
    # Phase 1A exposes only integrated validation. The first three frozen labels
    # intentionally use the same public validation command and are reported as
    # conservative end-to-end proxies for their non-public stage boundaries.
    if operation in OPERATIONS[:3]:
        return ["validate", "--project", project, "--json"]
    if operation == "overview":
        return ["overview", "--project", project, "--as-of", AS_OF, "--json"]
    if operation == "show-exact-id":
        return ["show", "--project", project, ids["show_id"], "--as-of", AS_OF, "--json"]
    if operation == "history-logical-id":
        return ["history", "--project", project, ids["history_logical_id"], "--json"]
    if operation == "query-kind-and-thread":
        return [
            "query",
            "--project",
            project,
            "--kind",
            ids["query_kind"],
            "--thread",
            ids["thread_id"],
            "--as-of",
            AS_OF,
            "--limit",
            "10000",
            "--json",
        ]
    if operation == "chain-validate":
        return [
            "chain",
            "validate",
            "--project",
            project,
            ids["chain_id"],
            "--profile",
            ids["chain_profile"],
            "--json",
        ]
    if operation == "access-explain":
        return ["access", "explain", "--project", project, ids["access_id"], "--json"]
    if operation == "export-check":
        return [
            "export",
            "check",
            "--project",
            project,
            ids["export_id"],
            "--level-ceiling",
            "public",
            "--as-of",
            AS_OF,
            "--json",
        ]
    raise ValueError(f"unknown operation: {operation}")


def sanitized_command(operation: str, ids: dict[str, str]) -> list[str]:
    actual = command_arguments(operation, Path("<PROJECT_ROOT>"), ids)
    return ["<RP_BINARY>", *actual]


def parse_elapsed(value: str) -> float:
    parts = value.strip().split(":")
    if len(parts) == 2:
        minutes, seconds = parts
        return int(minutes) * 60 + float(seconds)
    if len(parts) == 3:
        hours, minutes, seconds = parts
        return int(hours) * 3600 + int(minutes) * 60 + float(seconds)
    return float(value)


def parse_time_report(path: Path) -> tuple[float, int]:
    elapsed = None
    rss_kib = None
    for line in path.read_text(encoding="utf-8").splitlines():
        if "Elapsed (wall clock) time" in line:
            elapsed = parse_elapsed(line.rsplit(": ", 1)[1])
        elif "Maximum resident set size (kbytes)" in line:
            rss_kib = int(line.rsplit(":", 1)[1].strip())
    if elapsed is None or rss_kib is None:
        raise RuntimeError(f"GNU time report is incomplete: {path.name}")
    return elapsed, rss_kib * 1024


def run_once(
    *,
    rp: Path,
    arguments: list[str],
    time_binary: Path,
    deadline_seconds: int,
    scratch: Path,
    label: str,
) -> dict[str, Any]:
    stdout_path = scratch / f"{label}.stdout"
    stderr_path = scratch / f"{label}.stderr"
    time_path = scratch / f"{label}.time"
    timeout_binary = shutil.which("timeout")
    if timeout_binary is None:
        raise RuntimeError("timeout is required to enforce command deadlines")
    command = [
        timeout_binary,
        "--signal=TERM",
        "--kill-after=5s",
        f"{deadline_seconds}s",
        str(time_binary),
        "-v",
        "-o",
        str(time_path),
        str(rp),
        *arguments,
    ]
    environment = os.environ.copy()
    environment["LC_ALL"] = "C"
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        completed = subprocess.run(command, stdout=stdout, stderr=stderr, env=environment, check=False)
    if not time_path.exists():
        raise RuntimeError(f"command deadline/launcher failure for {label}: exit {completed.returncode}")
    wall_seconds, peak_rss_bytes = parse_time_report(time_path)
    stdout = stdout_path.read_bytes()
    stderr = stderr_path.read_bytes()
    try:
        envelope = json.loads(stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{label} did not emit one JSON envelope") from error
    if completed.returncode != 0 or envelope.get("status") != "ok" or envelope.get("exit_code") != 0:
        findings = [item.get("error_code") for item in envelope.get("findings", [])]
        raise RuntimeError(
            f"{label} correctness failure: process={completed.returncode} "
            f"status={envelope.get('status')} findings={findings}"
        )
    if stdout.count(b"\n") != 1 or not stdout.endswith(b"\n"):
        raise RuntimeError(f"{label} output is not exactly one newline-terminated JSON object")
    if stderr:
        raise RuntimeError(f"{label} unexpectedly wrote stderr ({len(stderr)} bytes)")
    return {
        "wall_seconds": wall_seconds,
        "peak_rss_bytes": peak_rss_bytes,
        "process_exit_code": completed.returncode,
        "envelope_status": envelope["status"],
        "envelope_exit_code": envelope["exit_code"],
        "stdout_sha256": sha256_bytes(stdout),
        "stdout_bytes": len(stdout),
        "canonical_object_count": envelope.get("data", {}).get("canonical_object_count"),
    }


def nearest_rank_p95(values: list[float]) -> float:
    ordered = sorted(values)
    return ordered[math.ceil(0.95 * len(ordered)) - 1]


def host_metadata(rp: Path, project: Path, time_binary: Path) -> dict[str, Any]:
    os_release: dict[str, str] = {}
    for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            os_release[key] = value.strip().strip('"')
    cpu_model = "unknown"
    for line in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines():
        if line.startswith("model name"):
            cpu_model = line.split(":", 1)[1].strip()
            break
    memory_total_bytes = 0
    for line in Path("/proc/meminfo").read_text(encoding="utf-8").splitlines():
        if line.startswith("MemTotal:"):
            memory_total_bytes = int(line.split()[1]) * 1024
            break
    fs_type = subprocess.run(
        ["stat", "-f", "-c", "%T", str(project)],
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()
    rp_version = subprocess.run([str(rp), "--version"], text=True, capture_output=True, check=True).stdout.strip()
    time_version = subprocess.run(
        [str(time_binary), "--version"], text=True, capture_output=True, check=True
    ).stdout.splitlines()[0]
    return {
        "host": platform.node(),
        "operating_system": os_release.get("PRETTY_NAME", platform.platform()),
        "kernel": platform.release(),
        "cpu_model": cpu_model,
        "logical_core_count": os.cpu_count(),
        "memory_total_bytes": memory_total_bytes,
        "filesystem_type": fs_type,
        "page_cache_condition": "warm; no cache drop; two command-specific warmups precede measurements",
        "runtime": {
            "rp": rp_version,
            "python": platform.python_version(),
            "gnu_time": time_version,
        },
    }


def main() -> int:
    args = parse_args()
    if not args.rp.is_file() or not os.access(args.rp, os.X_OK):
        raise RuntimeError("--rp must name an executable file")
    if not args.time_binary.is_file() or not os.access(args.time_binary, os.X_OK):
        raise RuntimeError("--time-binary must name an executable GNU time binary")
    manifest_path = args.project / "scale-manifest.json"
    manifest_bytes = manifest_path.read_bytes()
    manifest = json.loads(manifest_bytes)
    if manifest["profile"] != args.profile:
        raise RuntimeError("manifest profile does not match --profile")
    if args.profile == "stress":
        raise RuntimeError("stress acceptance is unsupported until explicit bounded CLI limits are available")
    verify_corpus(args.project, manifest)
    ids = selected_ids(args.project, manifest)
    object_count = sum(manifest["object_counts"].values())
    result: dict[str, Any] = {
        "schema": "rp/phase1-acceptance-benchmark/v1",
        "benchmark_date": date.today().isoformat(),
        "binary_sha256": sha256_bytes(args.rp.read_bytes()),
        "profile": args.profile,
        "seed": manifest["seed"],
        "as_of": manifest["as_of"],
        "manifest": {
            "aggregate_sha256": manifest["aggregate_sha256"],
            "file_sha256": sha256_bytes(manifest_bytes),
            "entry_count": len(manifest["sorted_path_sha256_entries"]),
            "object_count": object_count,
        },
        "host": host_metadata(args.rp, args.project, args.time_binary),
        "protocol": {
            "warmups": args.warmups,
            "measurements": args.measurements,
            "deadline_seconds": args.deadline_seconds,
            "p95_method": "nearest-rank",
            "single_thread_baseline": True,
            "validation_stage_mapping": (
                "The public Phase 1A CLI exposes integrated validation only; the first three "
                "operation labels use the same end-to-end `rp validate` invocation as conservative proxies."
            ),
        },
        "selected_ids": ids,
        "operations": [],
    }
    csv_rows: list[dict[str, Any]] = []
    args.output_json.parent.mkdir(parents=True, exist_ok=True)
    args.output_csv.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rp-phase1-bench-") as temporary:
        scratch = Path(temporary)
        for operation in OPERATIONS:
            arguments = command_arguments(operation, args.project, ids)
            preflight = run_once(
                rp=args.rp,
                arguments=arguments,
                time_binary=args.time_binary,
                deadline_seconds=args.deadline_seconds,
                scratch=scratch,
                label=f"{operation}-preflight",
            )
            if operation in OPERATIONS[:3] and preflight["canonical_object_count"] != object_count:
                raise RuntimeError("validated object count differs from manifest")
            warmups = []
            for repetition in range(args.warmups):
                warmups.append(
                    run_once(
                        rp=args.rp,
                        arguments=arguments,
                        time_binary=args.time_binary,
                        deadline_seconds=args.deadline_seconds,
                        scratch=scratch,
                        label=f"{operation}-warmup-{repetition + 1}",
                    )
                )
            measurements = []
            for repetition in range(args.measurements):
                item = run_once(
                    rp=args.rp,
                    arguments=arguments,
                    time_binary=args.time_binary,
                    deadline_seconds=args.deadline_seconds,
                    scratch=scratch,
                    label=f"{operation}-measurement-{repetition + 1}",
                )
                item["repetition"] = repetition + 1
                item["objects_per_second"] = object_count / item["wall_seconds"]
                measurements.append(item)
                csv_rows.append(
                    {
                        "profile": args.profile,
                        "operation": operation,
                        "repetition": repetition + 1,
                        **item,
                    }
                )
            expected_digest = preflight["stdout_sha256"]
            observed_digests = {
                item["stdout_sha256"] for item in [preflight, *warmups, *measurements]
            }
            if observed_digests != {expected_digest}:
                raise RuntimeError(f"{operation} stdout changed across repetitions")
            wall_values = [item["wall_seconds"] for item in measurements]
            rss_values = [item["peak_rss_bytes"] for item in measurements]
            throughput_values = [item["objects_per_second"] for item in measurements]
            result["operations"].append(
                {
                    "operation": operation,
                    "interactive": operation in INTERACTIVE,
                    "command": sanitized_command(operation, ids),
                    "preflight": preflight,
                    "warmup_wall_seconds": [item["wall_seconds"] for item in warmups],
                    "measurements": measurements,
                    "summary": {
                        "median_wall_seconds": median(wall_values),
                        "p95_wall_seconds": nearest_rank_p95(wall_values),
                        "peak_rss_bytes": max(rss_values),
                        "median_objects_per_second": median(throughput_values),
                    },
                }
            )
            print(
                f"{args.profile} {operation}: median={median(wall_values):.2f}s "
                f"p95={nearest_rank_p95(wall_values):.2f}s rss={max(rss_values)}",
                flush=True,
            )
    verify_corpus(args.project, manifest)
    if manifest_path.read_bytes() != manifest_bytes:
        raise RuntimeError("manifest changed during benchmark")
    args.output_json.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    fieldnames = [
        "profile",
        "operation",
        "repetition",
        "wall_seconds",
        "peak_rss_bytes",
        "objects_per_second",
        "process_exit_code",
        "envelope_status",
        "envelope_exit_code",
        "stdout_sha256",
        "stdout_bytes",
        "canonical_object_count",
    ]
    with args.output_csv.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(csv_rows)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError, KeyError, StopIteration) as error:
        print(f"run_acceptance.py: {error}", file=sys.stderr)
        raise SystemExit(1)
