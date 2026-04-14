#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
RUBRIC_DIR = REPO_ROOT / "benchmarks" / "terminal-agent" / "rubrics"
DEFAULT_OUTPUT_DIR = REPO_ROOT / ".context" / "benchmarks" / "judged"
DEFAULT_SOCKET = os.environ.get("CON_SOCKET_PATH", "/tmp/con.sock")
DEFAULT_CON_CLI_BIN = REPO_ROOT / "target" / "debug" / "con-cli"


class JudgeError(RuntimeError):
    pass


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Ask Con's built-in agent to judge a benchmark run from the rubric, raw record, and saved conversation transcript.",
    )
    parser.add_argument("--profile", required=True, help="Benchmark profile / rubric id.")
    parser.add_argument("--record", required=True, type=Path, help="Benchmark run record JSON.")
    parser.add_argument(
        "--socket",
        default=DEFAULT_SOCKET,
        help="Path to the running Con control socket.",
    )
    parser.add_argument(
        "--conversations-dir",
        type=Path,
        help="Optional explicit conversations directory. Use this for isolated bench-runtime runs.",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=180,
        help="Judge request timeout in seconds.",
    )
    parser.add_argument(
        "--keep-tab",
        action="store_true",
        help="Keep the temporary judge tab open instead of closing it.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="Optional output path. Defaults to .context/benchmarks/judged/<timestamp>-<profile>.json.",
    )
    return parser.parse_args()


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def cli_base() -> list[str]:
    override = os.environ.get("CON_BENCH_CON_CLI")
    if override:
        return override.split()
    if DEFAULT_CON_CLI_BIN.exists():
        return [str(DEFAULT_CON_CLI_BIN)]
    return ["cargo", "run", "-q", "-p", "con-cli", "--"]


def run_json(socket_path: str, *args: str) -> dict[str, Any]:
    cmd = [*cli_base(), "--json", "--socket", socket_path, *args]
    proc = subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if proc.returncode != 0:
        raise JudgeError(
            f"`{' '.join(cmd)}` failed with exit {proc.returncode}: {proc.stderr.strip() or proc.stdout.strip()}"
        )
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise JudgeError(f"invalid JSON from `{' '.join(cmd)}`: {exc}") from exc


def output_path(args: argparse.Namespace) -> Path:
    if args.output is not None:
        return args.output
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return DEFAULT_OUTPUT_DIR / f"{stamp}-{args.profile}.json"


def find_conversation_id(record: dict[str, Any]) -> str:
    ids: list[str] = []
    for result in record.get("results", []):
        details = result.get("details") or {}
        for action in details.get("setup_actions", []):
            conversation_id = action.get("conversation_id")
            if conversation_id:
                ids.append(str(conversation_id))
        for step in details.get("steps", []):
            conversation_id = step.get("conversation_id")
            if conversation_id:
                ids.append(str(conversation_id))
    unique = []
    seen = set()
    for item in ids:
        if item not in seen:
            seen.add(item)
            unique.append(item)
    if not unique:
        raise JudgeError("benchmark record does not contain a conversation_id")
    return unique[-1]


def candidate_conversation_dirs(record_path: Path, explicit_dir: Path | None) -> list[Path]:
    candidates: list[Path] = []
    if explicit_dir is not None:
        candidates.append(explicit_dir)
    env_dir = os.environ.get("CON_CONVERSATIONS_DIR")
    if env_dir:
        candidates.append(Path(env_dir))

    runtime_dir = REPO_ROOT / ".context" / "bench-runtime" / record_path.stem / "conversations"
    candidates.append(runtime_dir)
    runtime_matches = sorted((REPO_ROOT / ".context" / "bench-runtime").glob(f"{record_path.stem}/conversations"))
    candidates.extend(runtime_matches)

    home = Path.home()
    candidates.append(home / "Library" / "Application Support" / "con" / "conversations")
    candidates.append(home / ".local" / "share" / "con" / "conversations")

    deduped: list[Path] = []
    seen = set()
    for path in candidates:
        key = str(path)
        if key in seen:
            continue
        seen.add(key)
        deduped.append(path)
    return deduped


def locate_conversation_path(
    conversation_id: str,
    record_path: Path,
    explicit_dir: Path | None,
) -> Path:
    for directory in candidate_conversation_dirs(record_path, explicit_dir):
        path = directory / f"{conversation_id}.json"
        if path.exists():
            return path
    raise JudgeError(
        f"could not locate saved conversation {conversation_id} for record {record_path}"
    )


def truncate_text(value: str, limit: int = 500) -> str:
    value = value.strip()
    if len(value) <= limit:
        return value
    return value[: limit - 3] + "..."


def truncate_tool_output(value: str, limit: int = 1800) -> str:
    value = value.strip()
    if len(value) <= limit:
        return value
    head = value[: limit // 2 - 32].rstrip()
    tail = value[-(limit // 2 - 32) :].lstrip()
    return f"{head}\n...\n{tail}"


def compact_step(step: Any) -> dict[str, Any] | str:
    if not isinstance(step, dict) or len(step) != 1:
        return str(step)
    kind, payload = next(iter(step.items()))
    if kind == "Thinking":
        return {"thinking": truncate_text(str(payload), 220)}
    if kind == "ToolCall":
        return {
            "tool_call": {
                "tool": payload.get("tool"),
                "input": payload.get("input"),
            }
        }
    if kind == "ToolResult":
        output = payload.get("output")
        if isinstance(output, str):
            output = truncate_tool_output(output, 1800)
        return {
            "tool_result": {
                "tool": payload.get("tool"),
                "success": payload.get("success"),
                "output": output,
            }
        }
    return {kind: payload}


def compact_conversation(conversation: dict[str, Any]) -> list[dict[str, Any]]:
    compacted: list[dict[str, Any]] = []
    for message in conversation.get("messages", []):
        compacted.append(
            {
                "role": message.get("role"),
                "content": truncate_text(str(message.get("content", "")), 3000),
                "model": message.get("model"),
                "duration_ms": message.get("duration_ms"),
                "steps": [compact_step(step) for step in message.get("steps", [])],
            }
        )
    return compacted


def judge_prompt(
    profile: str,
    rubric: dict[str, Any],
    record: dict[str, Any],
    compacted_conversation: list[dict[str, Any]],
) -> str:
    judge_input = {
        "profile": profile,
        "rubric": rubric,
        "benchmark_record": record,
        "conversation": compacted_conversation,
    }
    return (
        "You are judging a terminal-agent benchmark run.\n"
        "Use only the provided rubric and evidence.\n"
        "Do not call any tools. The evidence is self-contained.\n"
        "Be strict, concrete, and evidence-based.\n\n"
        "Return exactly one JSON object with this shape:\n"
        "{\n"
        '  "summary": "short overall judgment",\n'
        '  "dimension_scores": {"dimension_id": integer, "...": integer},\n'
        '  "strengths": ["..."],\n'
        '  "weaknesses": ["..."],\n'
        '  "lessons": ["..."],\n'
        '  "next_focus": ["..."],\n'
        '  "confidence": "high|medium|low",\n'
        '  "evidence_gaps": ["..."]\n'
        "}\n\n"
        "Rules:\n"
        "- score every rubric dimension from 0 to its max\n"
        "- use the exact rubric dimension ids as keys in dimension_scores\n"
        "- cite concrete benchmark evidence in strengths/weaknesses/lessons\n"
        "- if evidence is weak or missing, lower confidence and say so in evidence_gaps\n"
        "- do not include markdown fences or explanatory prose outside the JSON object\n\n"
        f"Evidence:\n{json.dumps(judge_input, indent=2, sort_keys=True)}\n"
    )


def extract_json_object(text: str) -> dict[str, Any]:
    text = text.strip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass

    match = re.search(r"\{.*\}", text, re.S)
    if not match:
        raise JudgeError("judge response did not contain a JSON object")
    try:
        return json.loads(match.group(0))
    except json.JSONDecodeError as exc:
        raise JudgeError(f"judge response JSON could not be parsed: {exc}") from exc


def validate_judgment(judgment: dict[str, Any], rubric: dict[str, Any]) -> None:
    scores = judgment.get("dimension_scores")
    if not isinstance(scores, dict):
        raise JudgeError("judge response is missing dimension_scores")
    expected = {dim["id"]: int(dim["max"]) for dim in rubric["dimensions"]}
    missing = [dim_id for dim_id in expected if dim_id not in scores]
    if missing:
        raise JudgeError(f"judge response is missing dimension scores: {', '.join(missing)}")
    for dim_id, dim_max in expected.items():
        value = scores[dim_id]
        if not isinstance(value, int):
            raise JudgeError(f"judge score for {dim_id} is not an integer")
        if value < 0 or value > dim_max:
            raise JudgeError(
                f"judge score for {dim_id} must be between 0 and {dim_max}, got {value}"
            )


def main() -> int:
    args = parse_args()
    rubric_path = RUBRIC_DIR / f"{args.profile}.json"
    if not rubric_path.exists():
        raise SystemExit(f"missing rubric: {rubric_path}")
    if not args.record.exists():
        raise SystemExit(f"missing benchmark record: {args.record}")

    rubric = load_json(rubric_path)
    record = load_json(args.record)
    conversation_id = find_conversation_id(record)
    conversation_path = locate_conversation_path(
        conversation_id,
        args.record,
        args.conversations_dir,
    )
    conversation = load_json(conversation_path)

    prompt = judge_prompt(
        args.profile,
        rubric,
        record,
        compact_conversation(conversation),
    )

    created_tab = run_json(args.socket, "tabs", "new")
    tab_index = int(created_tab["active_tab_index"])
    try:
        run_json(args.socket, "agent", "new-conversation", "--tab", str(tab_index))
        result = run_json(
            args.socket,
            "agent",
            "ask",
            "--tab",
            str(tab_index),
            "--timeout",
            str(args.timeout),
            prompt,
        )
        message = result.get("message", {})
        raw_content = str(message.get("content", "")).strip()
        judgment = extract_json_object(raw_content)
        validate_judgment(judgment, rubric)

        payload = {
            "profile": args.profile,
            "record_path": str(args.record),
            "conversation_id": conversation_id,
            "conversation_path": str(conversation_path),
            "judged_at": utc_now(),
            "socket_path": args.socket,
            "judge_tab_index": tab_index,
            "judge_conversation_id": result.get("conversation_id"),
            "judge_message_id": message.get("id"),
            "judge_model": message.get("model"),
            "judge_duration_ms": message.get("duration_ms"),
            "raw_judgment_text": raw_content,
            "judgment": judgment,
        }
        out = output_path(args)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
        print(f"Output: {out}")
        return 0
    finally:
        if not args.keep_tab:
            try:
                run_json(args.socket, "tabs", "close", "--tab", str(tab_index))
            except Exception:
                pass


if __name__ == "__main__":
    raise SystemExit(main())
