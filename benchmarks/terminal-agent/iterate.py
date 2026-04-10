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
DEFAULT_CON_BIN = REPO_ROOT / "target" / "debug" / "con"
DEFAULT_CON_CLI_BIN = REPO_ROOT / "target" / "debug" / "con-cli"


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
    parser.add_argument(
        "--socket",
        type=Path,
        help="Reuse an existing Con control socket. Each iteration runs in a fresh tab instead of relaunching the app.",
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


def run_json(socket_path: Path, *args: str) -> dict:
    cli = [str(DEFAULT_CON_CLI_BIN)] if DEFAULT_CON_CLI_BIN.exists() else [
        "cargo",
        "run",
        "-q",
        "-p",
        "con-cli",
        "--",
    ]
    proc = subprocess.run(
        [
            *cli,
            "--json",
            "--socket",
            str(socket_path),
            *args,
        ],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=True,
    )
    return json.loads(proc.stdout)


def run_iteration(
    profile: str,
    suite: str,
    index: int,
    timeout_secs: int,
    socket_override: Path | None,
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

    if socket_override is not None:
        tab_info = run_json(socket_override, "tabs", "new")
        tab_index = int(tab_info["active_tab_index"])
        try:
            bench = subprocess.run(
                [
                    sys.executable,
                    "benchmarks/terminal-agent/run.py",
                    "--socket",
                    str(socket_override),
                    "--tab",
                    str(tab_index),
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
            return {
                "profile": profile,
                "suite": suite,
                "socket_path": str(socket_override),
                "record_path": str(record_path),
                "runtime_root": str(runtime_root),
                "log_path": None,
                "duration_ms": 0,
                "exit_code": bench.returncode,
                "stdout": bench.stdout,
                "status": "pass" if bench.returncode == 0 else "fail",
                "tab_index": tab_index,
            }
        finally:
            try:
                run_json(socket_override, "tabs", "close", "--tab", str(tab_index))
            except Exception:
                pass

    env = os.environ.copy()
    env["CON_SOCKET_PATH"] = str(socket_path)
    env["XDG_DATA_HOME"] = str(data_home)
    env["XDG_CONFIG_HOME"] = str(config_home)
    env["CON_SESSION_PATH"] = str(runtime_root / "session.json")
    env["CON_CONVERSATIONS_DIR"] = str(runtime_root / "conversations")

    attempts: list[dict[str, object]] = []
    for attempt in range(1, 4):
        with log_path.open("w") as log_file:
            app_cmd = [str(DEFAULT_CON_BIN)] if DEFAULT_CON_BIN.exists() else [
                "cargo",
                "run",
                "-q",
                "-p",
                "con",
            ]
            app = subprocess.Popen(
                app_cmd,
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
                stdout = bench.stdout
                attempts.append(
                    {
                        "attempt": attempt,
                        "duration_ms": duration_ms,
                        "exit_code": bench.returncode,
                        "stdout": stdout,
                        "blocked_bootstrap": False,
                    }
                )
                if bench.returncode == 0:
                    return {
                        "profile": profile,
                        "suite": suite,
                        "socket_path": str(socket_path),
                        "record_path": str(record_path),
                        "runtime_root": str(runtime_root),
                        "log_path": str(log_path),
                        "duration_ms": duration_ms,
                        "exit_code": bench.returncode,
                        "stdout": stdout,
                        "status": "pass",
                        "attempts": attempts,
                    }
                log_text = log_path.read_text() if log_path.exists() else ""
                should_retry = "ghostty_surface_new returned null" in log_text
                attempts[-1]["blocked_bootstrap"] = should_retry
                if not should_retry:
                    return {
                        "profile": profile,
                        "suite": suite,
                        "socket_path": str(socket_path),
                        "record_path": str(record_path),
                        "runtime_root": str(runtime_root),
                        "log_path": str(log_path),
                        "duration_ms": duration_ms,
                        "exit_code": bench.returncode,
                        "stdout": stdout,
                        "status": "fail",
                        "attempts": attempts,
                    }
            except Exception as exc:
                duration_ms = int((time.time() - started_at) * 1000)
                attempts.append(
                    {
                        "attempt": attempt,
                        "duration_ms": duration_ms,
                        "exit_code": None,
                        "stdout": str(exc),
                        "blocked_bootstrap": False,
                    }
                )
                log_text = log_path.read_text() if log_path.exists() else ""
                should_retry = "ghostty_surface_new returned null" in log_text
                attempts[-1]["blocked_bootstrap"] = should_retry
                if not should_retry:
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
                        "attempts": attempts,
                    }
            finally:
                terminate_process(app)
                try:
                    socket_path.unlink(missing_ok=True)
                except Exception:
                    pass
        time.sleep(2)

    last = attempts[-1] if attempts else {}
    blocked = bool(attempts) and all(item.get("blocked_bootstrap") for item in attempts)
    return {
        "profile": profile,
        "suite": suite,
        "socket_path": str(socket_path),
        "record_path": str(record_path),
        "runtime_root": str(runtime_root),
        "log_path": str(log_path),
        "duration_ms": int(last.get("duration_ms", 0)),
        "exit_code": last.get("exit_code"),
        "stdout": str(last.get("stdout", "benchmark app failed after retries")),
        "status": "blocked" if blocked else "fail",
        "block_reason": "ghostty_surface_bootstrap_unavailable" if blocked else None,
        "attempts": attempts,
    }


def main() -> int:
    args = parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    stamp = utc_stamp()

    iterations = []
    for idx, profile in enumerate(args.profile, start=1):
        result = run_iteration(profile, args.suite, idx, args.timeout, args.socket)
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
            "blocked": sum(1 for item in iterations if item["status"] == "blocked"),
            "failed": sum(
                1 for item in iterations if item["status"] not in {"pass", "blocked"}
            ),
        },
    }

    out = args.output_dir / f"iteration-batch-{stamp}.json"
    out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(f"Batch: {out}")
    return 0 if summary["counts"]["failed"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
