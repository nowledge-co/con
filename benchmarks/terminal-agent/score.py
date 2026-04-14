#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
RUBRIC_DIR = REPO_ROOT / "benchmarks" / "terminal-agent" / "rubrics"
DEFAULT_OUTPUT_DIR = REPO_ROOT / ".context" / "benchmarks" / "scored"


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Score a terminal-agent operator benchmark run against a named rubric.",
    )
    parser.add_argument("--profile", required=True, help="Benchmark profile/rubric id.")
    parser.add_argument("--record", required=True, type=Path, help="Benchmark run JSON record.")
    parser.add_argument(
        "--judge-file",
        type=Path,
        help="Optional LLM judge JSON from judge_llm.py. When provided, dimension scores, summary, lessons, next_focus, and scored_by default from the judge output.",
    )
    parser.add_argument(
        "--score",
        action="append",
        default=[],
        help="Dimension score as id=value. Repeat for each rubric dimension.",
    )
    parser.add_argument("--summary", default="", help="Short overall judgment for this run.")
    parser.add_argument(
        "--lesson",
        action="append",
        default=[],
        help="One lesson learned from this run. Repeat as needed.",
    )
    parser.add_argument(
        "--next-focus",
        action="append",
        default=[],
        help="One next improvement focus. Repeat as needed.",
    )
    parser.add_argument(
        "--scored-by",
        default="codex",
        help="Who judged the run (default: codex).",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="Optional output path. Defaults to .context/benchmarks/scored/<timestamp>-<profile>.json.",
    )
    return parser.parse_args()


def load_json(path: Path) -> dict:
    return json.loads(path.read_text())


def parse_scores(items: list[str]) -> dict[str, int]:
    parsed: dict[str, int] = {}
    for item in items:
        if "=" not in item:
            raise SystemExit(f"invalid --score `{item}`; expected id=value")
        key, value = item.split("=", 1)
        try:
            parsed[key] = int(value)
        except ValueError as exc:
            raise SystemExit(f"invalid integer score for `{key}`: {value}") from exc
    return parsed


def score_band(total: int, rubric: dict) -> str:
    if total >= int(rubric["world_class_score"]):
        return "world_class"
    if total >= int(rubric["target_score"]):
        return "target_met"
    if total >= int(rubric["release_floor"]):
        return "release_floor"
    return "below_floor"


def output_path(args: argparse.Namespace) -> Path:
    if args.output is not None:
        return args.output
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return DEFAULT_OUTPUT_DIR / f"{stamp}-{args.profile}.json"


def main() -> int:
    args = parse_args()
    rubric_path = RUBRIC_DIR / f"{args.profile}.json"
    if not rubric_path.exists():
        raise SystemExit(f"missing rubric: {rubric_path}")
    if not args.record.exists():
        raise SystemExit(f"missing benchmark record: {args.record}")

    rubric = load_json(rubric_path)
    record = load_json(args.record)
    judge = load_json(args.judge_file) if args.judge_file else None
    record_profile = None
    if isinstance(record.get("profile"), dict):
        record_profile = record["profile"].get("name")
    if record_profile and record_profile != args.profile:
        raise SystemExit(
            f"record profile `{record_profile}` does not match requested rubric `{args.profile}`"
        )
    provided_scores = parse_scores(args.score)
    if judge is not None:
        judge_scores = judge.get("judgment", {}).get("dimension_scores", {})
        for key, value in judge_scores.items():
            provided_scores.setdefault(key, value)

    dimensions = []
    total = 0
    max_total = 0
    missing = []
    for dim in rubric["dimensions"]:
        dim_id = dim["id"]
        dim_max = int(dim["max"])
        if dim_id not in provided_scores:
            missing.append(dim_id)
            continue
        score = provided_scores[dim_id]
        if score < 0 or score > dim_max:
            raise SystemExit(
                f"score for `{dim_id}` must be between 0 and {dim_max}, got {score}"
            )
        dimensions.append(
            {
                "id": dim_id,
                "label": dim["label"],
                "score": score,
                "max": dim_max,
                "description": dim["description"],
            }
        )
        total += score
        max_total += dim_max

    if missing:
        raise SystemExit(f"missing scores for rubric dimensions: {', '.join(missing)}")

    result = {
        "profile": args.profile,
        "rubric_title": rubric["title"],
        "record_path": str(args.record),
        "recorded_at": record.get("recorded_at"),
        "scored_at": utc_now(),
        "scored_by": args.scored_by
        if (args.scored_by != "codex" or judge is None)
        else "con_agent_judge",
        "summary": args.summary
        or (judge.get("judgment", {}).get("summary", "") if judge else ""),
        "lessons": args.lesson
        or (judge.get("judgment", {}).get("lessons", []) if judge else []),
        "next_focus": args.next_focus
        or (judge.get("judgment", {}).get("next_focus", []) if judge else []),
        "total_score": total,
        "max_score": max_total,
        "percentage": round((total / max_total) * 100, 1) if max_total else 0.0,
        "release_floor": rubric["release_floor"],
        "target_score": rubric["target_score"],
        "world_class_score": rubric["world_class_score"],
        "band": score_band(total, rubric),
        "dimensions": dimensions,
        "benchmark_counts": record.get("counts", {}),
        "benchmark_suite": record.get("suite"),
        "benchmark_profile": record.get("profile", {}).get("name")
        if isinstance(record.get("profile"), dict)
        else None,
    }
    if judge is not None:
        result["judge_file"] = str(args.judge_file)
        result["judge_confidence"] = judge.get("judgment", {}).get("confidence")
        result["judge_evidence_gaps"] = judge.get("judgment", {}).get("evidence_gaps", [])
        result["judge_strengths"] = judge.get("judgment", {}).get("strengths", [])
        result["judge_weaknesses"] = judge.get("judgment", {}).get("weaknesses", [])

    out = output_path(args)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")

    print(f"{result['rubric_title']}: {total}/{max_total} ({result['percentage']}%)")
    print(f"Band: {result['band']}")
    print(f"Target: {rubric['target_score']}  World-class: {rubric['world_class_score']}")
    print(f"Output: {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
