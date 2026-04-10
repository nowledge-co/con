#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT_DIR = REPO_ROOT / ".context" / "benchmarks" / "batches"


def utc_stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run isolated multi-iteration terminal-agent benchmark batches.",
    )
    parser.add_argument(
        "--profile",
        action="append",
        required=True,
        help="Benchmark profile to run. Repeat to build the batch sequence.",
    )
    parser.add_argument(
        "--suite",
        default="operator",
        choices=("strict", "agent", "operator", "all"),
        help="Benchmark suite to run for each profile.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_OUTPUT_DIR,
        help="Directory for batch summaries.",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=900,
        help="Max wall-clock seconds for one benchmark iteration, including app startup.",
    )
    return parser.parse_args()


def wait_for_socket(path: Path, timeout_secs: float) -> None:
    deadline = time.time() + timeout_secs
    while time.time() < deadline:
        if path.exists():
            return
        time.sleep(0.25)
    raise RuntimeError(f"socket did not appear: {path}")


def terminate_process(proc: subprocess.Popen[str]) -> None:
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=10)
        return
    except subprocess.TimeoutExpired:
        proc.send_signal(signal.SIGKILL)
        proc.wait(timeout=5)


def run_iteration(
    profile: str,
    suite: str,
    index: int,
    timeout_secs: int,
) -> dict:
    stamp = utc_stamp()
    runtime_root = REPO_ROOT / ".context" / "bench-runtime" / f"{stamp}-{index:02d}-{profile}"
    data_home = runtime_root / "data"
    config_home = runtime_root / "config"
    log_path = runtime_root / "con.log"
    record_path = REPO_ROOT / ".context" / "benchmarks" / f"{stamp}-{index:02d}-{profile}.json"
    socket_path = Path(f"/tmp/con-bench-{stamp}-{index:02d}.sock")

    data_home.mkdir(parents=True, exist_ok=True)
    config_home.mkdir(parents=True, exist_ok=True)
    runtime_root.mkdir(parents=True, exist_ok=True)

    env = os.environ.copy()
    env["CON_SOCKET_PATH"] = str(socket_path)
    env["XDG_DATA_HOME"] = str(data_home)
    env["XDG_CONFIG_HOME"] = str(config_home)

    with log_path.open("w") as log_file:
        app = subprocess.Popen(
            ["cargo", "run", "-q", "-p", "con"],
            cwd=REPO_ROOT,
            env=env,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            text=True,
        )

        started_at = time.time()
        try:
            wait_for_socket(socket_path, timeout_secs=min(90, timeout_secs))
            bench = subprocess.run(
                [
                    sys.executable,
                    "benchmarks/terminal-agent/run.py",
                    "--socket",
                    str(socket_path),
                    "--profile",
                    profile,
                    "--suite",
                    suite,
                    "--record",
                    str(record_path),
                ],
                cwd=REPO_ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=timeout_secs,
            )
            duration_ms = int((time.time() - started_at) * 1000)
            return {
                "profile": profile,
                "suite": suite,
                "socket_path": str(socket_path),
                "record_path": str(record_path),
                "runtime_root": str(runtime_root),
                "log_path": str(log_path),
                "duration_ms": duration_ms,
                "exit_code": bench.returncode,
                "stdout": bench.stdout,
                "status": "pass" if bench.returncode == 0 else "fail",
            }
        except Exception as exc:
            duration_ms = int((time.time() - started_at) * 1000)
            return {
                "profile": profile,
                "suite": suite,
                "socket_path": str(socket_path),
                "record_path": str(record_path),
                "runtime_root": str(runtime_root),
                "log_path": str(log_path),
                "duration_ms": duration_ms,
                "exit_code": None,
                "stdout": str(exc),
                "status": "fail",
            }
        finally:
            terminate_process(app)
            try:
                socket_path.unlink(missing_ok=True)
            except Exception:
                pass


def main() -> int:
    args = parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    stamp = utc_stamp()

    iterations = []
    for idx, profile in enumerate(args.profile, start=1):
        result = run_iteration(profile, args.suite, idx, args.timeout)
        iterations.append(result)
        marker = "PASS" if result["status"] == "pass" else "FAIL"
        print(f"[{marker}] {profile}: {result['record_path']}")

    summary = {
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "suite": args.suite,
        "iterations": iterations,
        "counts": {
            "total": len(iterations),
            "passed": sum(1 for item in iterations if item["status"] == "pass"),
            "failed": sum(1 for item in iterations if item["status"] != "pass"),
        },
    }

    out = args.output_dir / f"iteration-batch-{stamp}.json"
    out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(f"Batch: {out}")
    return 0 if summary["counts"]["failed"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
