#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from datetime import datetime
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_LOG = REPO_ROOT / "docs" / "impl" / "terminal-agent-improvement-log.md"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Append a scored terminal-agent benchmark result to the tracked improvement log.",
    )
    parser.add_argument("--scorecard", required=True, type=Path, help="Scored benchmark JSON file.")
    parser.add_argument(
        "--change",
        action="append",
        default=[],
        help="One product change included in this iteration. Repeat as needed.",
    )
    parser.add_argument(
        "--note",
        action="append",
        default=[],
        help="One free-form note for this iteration. Repeat as needed.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_LOG,
        help="Tracked markdown log path.",
    )
    return parser.parse_args()


def load_scorecard(path: Path) -> dict:
    if not path.exists():
        raise SystemExit(f"missing scorecard: {path}")
    return json.loads(path.read_text())


def format_timestamp(raw: str) -> str:
    if not raw:
        return "unknown"
    try:
        return datetime.fromisoformat(raw.replace("Z", "+00:00")).strftime("%Y-%m-%d %H:%M UTC")
    except Exception:
        return raw


def dimension_lines(scorecard: dict) -> list[str]:
    rows = []
    for dim in scorecard.get("dimensions", []):
        rows.append(f"- {dim['label']}: {dim['score']}/{dim['max']}")
    return rows


def build_entry(scorecard: dict, changes: list[str], notes: list[str]) -> str:
    heading = "## {timestamp} · {profile} · {score}/{max_score} · {band}".format(
        timestamp=format_timestamp(scorecard.get("scored_at", "")),
        profile=scorecard.get("profile", "unknown-profile"),
        score=scorecard.get("total_score", 0),
        max_score=scorecard.get("max_score", 0),
        band=scorecard.get("band", "unknown"),
    )

    lines = [heading, ""]
    if scorecard.get("summary"):
        lines.append(scorecard["summary"])
        lines.append("")

    lines.append("Score breakdown:")
    lines.extend(dimension_lines(scorecard))
    lines.append("")

    if changes:
        lines.append("Product changes:")
        lines.extend(f"- {item}" for item in changes)
        lines.append("")

    if scorecard.get("lessons"):
        lines.append("Lessons:")
        lines.extend(f"- {item}" for item in scorecard["lessons"])
        lines.append("")

    if scorecard.get("next_focus"):
        lines.append("Next focus:")
        lines.extend(f"- {item}" for item in scorecard["next_focus"])
        lines.append("")

    if notes:
        lines.append("Notes:")
        lines.extend(f"- {item}" for item in notes)
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def ensure_header(path: Path) -> str:
    if path.exists():
        return path.read_text()
    return (
        "# Terminal Agent Improvement Log\n\n"
        "Tracked benchmark-backed iteration notes for Con's terminal agent.\n\n"
    )


def main() -> int:
    args = parse_args()
    scorecard = load_scorecard(args.scorecard)
    current = ensure_header(args.output)
    entry = build_entry(scorecard, args.change, args.note)

    if not current.endswith("\n"):
        current += "\n"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(current + entry + "\n")
    print(f"Updated: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
