#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SOCKET = os.environ.get("CON_SOCKET_PATH", "/tmp/con.sock")
DEFAULT_RECORD_DIR = REPO_ROOT / ".context" / "benchmarks"
PROFILE_DIR = REPO_ROOT / "benchmarks" / "terminal-agent" / "profiles"


class BenchError(RuntimeError):
    pass


@dataclass
class CaseResult:
    name: str
    status: str
    duration_ms: int
    note: str
    details: dict[str, Any]


@dataclass
class Profile:
    name: str
    description: str
    recommended_suite: str
    env_checks: list[dict[str, Any]]
    playbooks: list[dict[str, Any]]
    operator_scenarios: list[dict[str, Any]]


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


class BenchmarkContext:
    def __init__(self, socket_path: str, default_tab: int | None) -> None:
        self.socket_path = socket_path
        self.default_tab = default_tab
        override = os.environ.get("CON_BENCH_CON_CLI")
        if override:
            self.cli_base = shlex.split(override)
        elif shutil.which("con-cli"):
            self.cli_base = ["con-cli"]
        else:
            self.cli_base = ["cargo", "run", "-q", "-p", "con-cli", "--"]

    def run_json(self, *args: str) -> dict[str, Any]:
        cmd = [*self.cli_base, "--json", "--socket", self.socket_path, *args]
        proc = subprocess.run(
            cmd,
            cwd=REPO_ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        if proc.returncode != 0:
            raise BenchError(
                f"`{' '.join(cmd)}` failed with exit {proc.returncode}: {proc.stderr.strip() or proc.stdout.strip()}"
            )
        try:
            return json.loads(proc.stdout)
        except json.JSONDecodeError as exc:
            raise BenchError(
                f"`{' '.join(cmd)}` did not return valid JSON: {exc}: {proc.stdout!r}"
            ) from exc

    def identify(self) -> dict[str, Any]:
        return self.run_json("identify")

    def tabs_list(self) -> dict[str, Any]:
        return self.run_json("tabs", "list")

    def panes_list(self, tab_index: int | None = None) -> dict[str, Any]:
        args = ["panes", "list"]
        if tab_index is not None:
            args.extend(["--tab", str(tab_index)])
        return self.run_json(*args)

    def panes_exec(self, tab_index: int, pane_id: int, *command: str) -> dict[str, Any]:
        return self.run_json(
            "panes",
            "exec",
            "--tab",
            str(tab_index),
            "--pane-id",
            str(pane_id),
            "--",
            *command,
        )

    def panes_wait(
        self,
        tab_index: int,
        pane_id: int,
        *,
        pattern: str | None = None,
        timeout_secs: int | None = None,
    ) -> dict[str, Any]:
        args = [
            "panes",
            "wait",
            "--tab",
            str(tab_index),
            "--pane-id",
            str(pane_id),
        ]
        if pattern is not None:
            args.extend(["--pattern", pattern])
        if timeout_secs is not None:
            args.extend(["--timeout", str(timeout_secs)])
        return self.run_json(*args)

    def agent_ask(
        self,
        tab_index: int,
        prompt: str,
        *,
        request_timeout_secs: float | None = None,
        wait_for_turn_secs: float = 30.0,
        poll_interval_secs: float = 1.0,
    ) -> dict[str, Any]:
        deadline = time.time() + wait_for_turn_secs
        while True:
            cmd = [
                *self.cli_base,
                "--json",
                "--socket",
                self.socket_path,
                "agent",
                "ask",
                "--tab",
                str(tab_index),
            ]
            if request_timeout_secs is not None:
                cmd.extend(["--timeout", str(int(request_timeout_secs))])
            cmd.append(prompt)
            try:
                proc = subprocess.run(
                    cmd,
                    cwd=REPO_ROOT,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    timeout=(request_timeout_secs + 5.0)
                    if request_timeout_secs is not None
                    else None,
                )
            except subprocess.TimeoutExpired as exc:
                raise BenchError(
                    "agent ask subprocess exceeded the benchmark timeout after "
                    f"{request_timeout_secs:.0f}s: {prompt!r}"
                ) from exc
            if proc.returncode == 0:
                try:
                    return json.loads(proc.stdout)
                except json.JSONDecodeError as exc:
                    raise BenchError(
                        f"`{' '.join(cmd)}` did not return valid JSON: {exc}: {proc.stdout!r}"
                    ) from exc

            message = proc.stderr.strip() or proc.stdout.strip()
            pending_turn = "pending con-cli agent request" in message
            if not pending_turn:
                raise BenchError(
                    f"`{' '.join(cmd)}` failed with exit {proc.returncode}: {message}"
                )
            if time.time() >= deadline:
                raise BenchError(
                    "the target tab stayed busy with another agent request for too long; "
                    "use an idle benchmark tab or wait for the in-tab conversation to finish"
                )
            time.sleep(poll_interval_secs)

    def active_tab_index(self) -> int:
        if self.default_tab is not None:
            return self.default_tab
        identify = self.identify()
        return int(identify["active_tab_index"])

    def choose_exec_visible_shell(self, tab_index: int) -> tuple[int, int, dict[str, Any]]:
        panes = self.panes_list(tab_index)
        for pane in panes.get("panes", []):
            capabilities = pane.get("control_capabilities", [])
            if (
                "exec_visible_shell" in capabilities
                and pane.get("is_alive")
                and pane.get("surface_ready", False)
            ):
                return int(pane["pane_id"]), int(pane["index"]), pane
        raise BenchError(
            f"tab {tab_index} does not expose a live, surface-ready pane with exec_visible_shell"
        )

    def wait_for_pane_capability(
        self,
        tab_index: int,
        pane_id: int,
        capability: str,
        *,
        timeout_secs: float = 5.0,
        poll_interval_secs: float = 0.25,
    ) -> dict[str, Any]:
        deadline = time.time() + timeout_secs
        last_pane: dict[str, Any] | None = None
        while time.time() < deadline:
            panes = self.panes_list(tab_index)
            pane = next(
                (item for item in panes.get("panes", []) if item.get("pane_id") == pane_id),
                None,
            )
            if pane is None:
                raise BenchError(f"pane {pane_id} disappeared while waiting for {capability}")
            last_pane = pane
            if capability in pane.get("control_capabilities", []):
                return pane
            time.sleep(poll_interval_secs)
        raise BenchError(
            f"pane {pane_id} did not regain {capability} within {timeout_secs:.1f}s"
        ) from None

    def shell_command(self, command: list[str], *, timeout_secs: float = 8.0) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            command,
            cwd=REPO_ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout_secs,
        )


def load_profile(profile_name: str) -> Profile:
    profile_path = PROFILE_DIR / f"{profile_name}.json"
    if not profile_path.exists():
        raise BenchError(f"unknown profile `{profile_name}`; expected {profile_path}")
    data = json.loads(profile_path.read_text())
    return Profile(
        name=data["name"],
        description=data["description"],
        recommended_suite=data.get("recommended_suite", "strict"),
        env_checks=data.get("env_checks", []),
        playbooks=data.get("playbooks", []),
        operator_scenarios=data.get("operator_scenarios", []),
    )


def list_profiles() -> list[Profile]:
    profiles: list[Profile] = []
    for profile_path in sorted(PROFILE_DIR.glob("*.json")):
        data = json.loads(profile_path.read_text())
        profiles.append(
            Profile(
                name=data["name"],
                description=data["description"],
                recommended_suite=data.get("recommended_suite", "strict"),
                env_checks=data.get("env_checks", []),
                playbooks=data.get("playbooks", []),
                operator_scenarios=data.get("operator_scenarios", []),
            )
        )
    return profiles


def env_case_for(check: dict[str, Any]) -> tuple[str, Callable[[BenchmarkContext], tuple[str, dict[str, Any]]]]:
    kind = check["kind"]
    name = check["name"]
    value = check["value"]

    def case_command(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
        resolved = shutil.which(value)
        if not resolved:
            raise BenchError(f"required command `{value}` is not available in PATH")
        return (f"`{value}` resolved to {resolved}", {"command": value, "resolved_path": resolved})

    def case_path_exists(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
        path = Path(os.path.expanduser(value))
        if not path.exists():
            raise BenchError(f"required path does not exist: {path}")
        return (f"{path} exists", {"path": str(path)})

    def case_ssh_host(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
        command = [
            "ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            value,
            "printf ok",
        ]
        proc = ctx.shell_command(command, timeout_secs=8.0)
        if proc.returncode != 0 or proc.stdout.strip() != "ok":
            stderr = proc.stderr.strip() or proc.stdout.strip()
            raise BenchError(f"ssh host `{value}` is not reachable in batch mode: {stderr}")
        return (f"`{value}` reachable over batch SSH", {"host": value})

    handlers: dict[str, Callable[[BenchmarkContext], tuple[str, dict[str, Any]]]] = {
        "command": case_command,
        "path_exists": case_path_exists,
        "ssh_host": case_ssh_host,
    }
    if kind not in handlers:
        raise BenchError(f"unsupported env check kind `{kind}` in profile")
    return (f"env_{name}", handlers[kind])


def run_case(
    ctx: BenchmarkContext,
    name: str,
    case_fn: Callable[[BenchmarkContext], tuple[str, dict[str, Any]]],
) -> CaseResult:
    started = time.time()
    try:
        note, details = case_fn(ctx)
        status = "pass"
    except BenchError as exc:
        note = str(exc)
        details = {}
        status = "fail"
    except Exception as exc:  # pragma: no cover - benchmark runner should not hide crashes
        note = f"unexpected error: {exc}"
        details = {}
        status = "fail"
    duration_ms = int((time.time() - started) * 1000)
    return CaseResult(
        name=name,
        status=status,
        duration_ms=duration_ms,
        note=note,
        details=details,
    )


def case_socket_identify(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
    identify = ctx.identify()
    if identify.get("app") != "con":
        raise BenchError(f"identify returned unexpected app: {identify.get('app')!r}")
    methods = identify.get("methods", [])
    if not methods:
        raise BenchError("identify returned no control methods")
    return (
        f"active tab {identify['active_tab_index']} on {identify['socket_path']}",
        {
            "active_tab_index": identify["active_tab_index"],
            "tab_count": identify["tab_count"],
            "method_count": len(methods),
        },
    )


def case_tabs_list(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
    tabs = ctx.tabs_list()
    listed = tabs.get("tabs", [])
    if not listed:
        raise BenchError("tabs list returned no tabs")
    active_tabs = [tab for tab in listed if tab.get("is_active")]
    if len(active_tabs) != 1:
        raise BenchError(f"expected exactly one active tab, found {len(active_tabs)}")
    active = active_tabs[0]
    return (
        f"{len(listed)} tabs; active tab {active['index']} has {active['pane_count']} pane(s)",
        {
            "tab_count": len(listed),
            "active_tab_index": active["index"],
            "active_pane_count": active["pane_count"],
        },
    )


def case_panes_list(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
    tab_index = ctx.active_tab_index()
    panes = ctx.panes_list(tab_index)
    listed = panes.get("panes", [])
    if not listed:
        raise BenchError(f"tab {tab_index} returned no panes")
    alive = [pane for pane in listed if pane.get("is_alive")]
    if not alive:
        raise BenchError(f"tab {tab_index} returned no live panes")
    unready = [
        pane["pane_id"]
        for pane in alive
        if pane.get("surface_ready", False) is not True
    ]
    if unready:
        raise BenchError(
            f"tab {tab_index} returned live panes without initialized surfaces: {unready}"
        )
    focused = [pane for pane in listed if pane.get("is_focused")]
    if len(focused) != 1:
        raise BenchError(f"expected one focused pane, found {len(focused)}")
    return (
        f"tab {tab_index} has {len(listed)} pane(s); focused pane {focused[0]['pane_id']}",
        {
            "tab_index": tab_index,
            "pane_count": len(listed),
            "focused_pane_id": focused[0]["pane_id"],
            "live_surface_ready_panes": [pane["pane_id"] for pane in alive],
        },
    )


def case_visible_shell_exec(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
    tab_index = ctx.active_tab_index()
    pane_id, pane_index, pane = ctx.choose_exec_visible_shell(tab_index)
    token = f"CON_BENCH_READY_{int(time.time() * 1000)}"
    exec_result = ctx.panes_exec(tab_index, pane_id, "/bin/echo", token)
    wait_result = ctx.panes_wait(
        tab_index,
        pane_id,
        pattern=token,
        timeout_secs=10,
    )
    if wait_result.get("status") != "matched":
        raise BenchError(
            f"wait did not match benchmark token; status={wait_result.get('status')!r}"
        )

    fresh = ctx.wait_for_pane_capability(tab_index, pane_id, "exec_visible_shell")

    return (
        f"tab {tab_index} pane {pane_index} echoed benchmark token and stayed shell-ready",
        {
            "tab_index": tab_index,
            "pane_id": pane_id,
            "pane_index": pane_index,
            "surface_ready_before": pane.get("surface_ready"),
            "surface_ready_after": fresh.get("surface_ready"),
            "front_state_before": pane.get("front_state"),
            "front_state_after": fresh.get("front_state"),
            "wait_status": wait_result.get("status"),
            "exec_result": exec_result,
        },
    )


def case_agent_ready(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
    enabled = os.environ.get("CON_BENCH_ENABLE_AGENT", "").lower()
    if enabled not in {"1", "true", "yes"}:
        return (
            "skipped; set CON_BENCH_ENABLE_AGENT=1 to run live in-tab agent verification",
            {"skipped": True},
        )

    tab_index = ctx.active_tab_index()
    token = f"CON_BENCH_AGENT_READY_{int(time.time() * 1000)}"
    result = ctx.agent_ask(tab_index, f"Reply with ONLY {token}")
    message = result.get("message", {})
    content = str(message.get("content", "")).strip()
    if token not in content:
        raise BenchError(f"agent did not echo the expected token; got {content!r}")
    return (
        f"tab {tab_index} agent returned benchmark token",
        {
            "tab_index": tab_index,
            "conversation_id": result.get("conversation_id"),
            "content": content,
        },
    )


STRICT_CASES: list[tuple[str, Callable[[BenchmarkContext], tuple[str, dict[str, Any]]]]] = [
    ("socket_identify", case_socket_identify),
    ("tabs_list", case_tabs_list),
    ("panes_list", case_panes_list),
    ("visible_shell_exec", case_visible_shell_exec),
]

AGENT_CASES: list[tuple[str, Callable[[BenchmarkContext], tuple[str, dict[str, Any]]]]] = [
    ("agent_ready", case_agent_ready),
]


def operator_case_for(
    scenario: dict[str, Any],
) -> tuple[str, Callable[[BenchmarkContext], tuple[str, dict[str, Any]]]]:
    scenario_name = scenario.get("name", "operator_scenario")
    case_name = scenario.get("case_name", f"operator_{scenario_name}")
    steps = scenario.get("steps", [])

    def case(ctx: BenchmarkContext) -> tuple[str, dict[str, Any]]:
        if not steps:
            raise BenchError(f"operator scenario `{scenario_name}` has no steps")

        tab_index = ctx.active_tab_index()
        executed_steps: list[dict[str, Any]] = []

        for idx, step in enumerate(steps, start=1):
            prompt = step.get("prompt", "").strip()
            if not prompt:
                raise BenchError(
                    f"operator scenario `{scenario_name}` step {idx} has an empty prompt"
                )

            label = step.get("label") or f"step_{idx}"
            timeout_secs = step.get("timeout_secs")
            result = ctx.agent_ask(
                tab_index,
                prompt,
                request_timeout_secs=float(timeout_secs) if timeout_secs is not None else 90.0,
            )
            message = result.get("message", {})
            content = str(message.get("content", "")).strip()
            if not content:
                raise BenchError(
                    f"operator scenario `{scenario_name}` step {idx} returned no assistant content"
                )

            expect_contains = step.get("expect_contains", [])
            missing = [needle for needle in expect_contains if needle not in content]
            if missing:
                raise BenchError(
                    f"operator scenario `{scenario_name}` step {idx} missing expected content: {missing}"
                )

            executed_steps.append(
                {
                    "index": idx,
                    "label": label,
                    "timeout_secs": timeout_secs,
                    "prompt": prompt,
                    "conversation_id": result.get("conversation_id"),
                    "message_id": message.get("id"),
                    "duration_ms": message.get("duration_ms"),
                    "content": content,
                }
            )

        return (
            f"{scenario_name}: executed {len(executed_steps)} operator step(s) on tab {tab_index}; review transcript for quality",
            {
                "tab_index": tab_index,
                "scenario": scenario_name,
                "review_required": True,
                "steps": executed_steps,
            },
        )

    return case_name, case


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Con terminal-agent benchmark suites against a live con-cli socket.",
    )
    parser.add_argument(
        "--list-profiles",
        action="store_true",
        help="List available benchmark profiles and exit.",
    )
    parser.add_argument(
        "--profile",
        help="Load a benchmark profile with environment checks and recommended playbooks.",
    )
    parser.add_argument(
        "--suite",
        choices=("strict", "agent", "operator", "all"),
        default="strict",
        help="Which benchmark suite to run.",
    )
    parser.add_argument(
        "--socket",
        default=DEFAULT_SOCKET,
        help="Path to the running Con control socket.",
    )
    parser.add_argument(
        "--tab",
        type=int,
        help="Override the default target tab for active-tab scenarios.",
    )
    parser.add_argument(
        "--record",
        type=Path,
        help="Optional JSON record path. Defaults to .context/benchmarks/terminal-agent-<timestamp>.json.",
    )
    return parser.parse_args()


def record_path_for(args: argparse.Namespace) -> Path:
    if args.record is not None:
        return args.record
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return DEFAULT_RECORD_DIR / f"terminal-agent-{stamp}.json"


def selected_cases(suite: str) -> list[tuple[str, Callable[[BenchmarkContext], tuple[str, dict[str, Any]]]]]:
    if suite == "strict":
        return STRICT_CASES
    if suite == "agent":
        return AGENT_CASES
    if suite == "operator":
        return []
    return [*STRICT_CASES, *AGENT_CASES]


def main() -> int:
    args = parse_args()
    if args.list_profiles:
        for profile in list_profiles():
            print(
                f"{profile.name}: {profile.description} "
                f"(recommended suite: {profile.recommended_suite})"
            )
        return 0

    profile: Profile | None = None
    env_cases: list[tuple[str, Callable[[BenchmarkContext], tuple[str, dict[str, Any]]]]] = []
    if args.profile:
        profile = load_profile(args.profile)
        env_cases = [env_case_for(check) for check in profile.env_checks]

    if args.suite == "operator" and (
        profile is None or not profile.operator_scenarios
    ):
        print(
            "[terminal-agent-benchmark] operator suite requires a profile with operator_scenarios",
            file=sys.stderr,
        )
        return 2

    socket_path = args.socket
    if not Path(socket_path).exists():
        print(f"[terminal-agent-benchmark] missing socket: {socket_path}", file=sys.stderr)
        return 2

    ctx = BenchmarkContext(socket_path=socket_path, default_tab=args.tab)
    operator_cases: list[
        tuple[str, Callable[[BenchmarkContext], tuple[str, dict[str, Any]]]]
    ] = []
    if profile and profile.operator_scenarios and args.suite in {"operator", "all"}:
        operator_cases = [operator_case_for(s) for s in profile.operator_scenarios]

    selected = [*env_cases, *selected_cases(args.suite), *operator_cases]
    results = [run_case(ctx, name, case_fn) for name, case_fn in selected]

    passed = sum(1 for result in results if result.status == "pass")
    failed = sum(1 for result in results if result.status == "fail")
    skipped = sum(
        1
        for result in results
        if result.status == "pass" and result.details.get("skipped") is True
    )

    summary = {
        "benchmark": "terminal-agent",
        "suite": args.suite,
        "profile": asdict(profile) if profile else None,
        "socket_path": socket_path,
        "tab_override": args.tab,
        "recorded_at": utc_now(),
        "results": [asdict(result) for result in results],
        "counts": {
            "total": len(results),
            "passed": passed,
            "failed": failed,
            "soft_skipped": skipped,
        },
    }

    for result in results:
        marker = "PASS" if result.status == "pass" else "FAIL"
        print(f"[{marker}] {result.name}: {result.note}")

    if profile and profile.playbooks:
        print("\nPlaybooks:")
        for playbook in profile.playbooks:
            label = playbook.get("name", playbook.get("path", "playbook"))
            path = playbook.get("path", "")
            print(f"- {label}: {path}")

    record_path = record_path_for(args)
    record_path.parent.mkdir(parents=True, exist_ok=True)
    record_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(f"\nRecord: {record_path}")

    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
