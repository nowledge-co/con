#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from datetime import datetime
from pathlib import Path
from statistics import mean


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_INPUT_DIR = REPO_ROOT / ".context" / "benchmarks" / "scored"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a trend report from scored terminal-agent benchmark runs.",
    )
    parser.add_argument(
        "--input-dir",
        type=Path,
        default=DEFAULT_INPUT_DIR,
        help="Directory of scored benchmark JSON files.",
    )
    parser.add_argument("--profile", help="Optional profile filter.")
    parser.add_argument("--limit", type=int, default=50, help="Max runs to include.")
    parser.add_argument("--output", type=Path, help="Optional markdown output path.")
    return parser.parse_args()


def load_scorecards(input_dir: Path) -> list[dict]:
    if not input_dir.exists():
        return []
    cards = []
    for path in sorted(input_dir.glob("*.json")):
        try:
            cards.append(json.loads(path.read_text()))
        except json.JSONDecodeError:
            continue
    cards.sort(key=lambda card: card.get("scored_at", ""))
    return cards


def history_tail(profile_cards: list[dict], limit: int = 6) -> str:
    tail = profile_cards[-limit:]
    return " -> ".join(str(card["total_score"]) for card in tail)


def sparkline(profile_cards: list[dict]) -> str:
    if not profile_cards:
        return ""
    glyphs = "▁▂▃▄▅▆▇█"
    max_score = max(int(card["max_score"]) for card in profile_cards) or 1
    out = []
    for card in profile_cards:
        score = int(card["total_score"])
        idx = round((score / max_score) * (len(glyphs) - 1))
        idx = max(0, min(idx, len(glyphs) - 1))
        out.append(glyphs[idx])
    return "".join(out)


def average_dimension_scores(profile_cards: list[dict]) -> list[tuple[str, float, int]]:
    totals: dict[str, list[int]] = defaultdict(list)
    maximums: dict[str, int] = {}
    for card in profile_cards:
        for dim in card.get("dimensions", []):
            totals[dim["label"]].append(int(dim["score"]))
            maximums[dim["label"]] = int(dim["max"])
    rows = []
    for label, scores in sorted(totals.items()):
        rows.append((label, round(mean(scores), 2), maximums[label]))
    return rows


def unique_recent(items: list[str], limit: int = 5) -> list[str]:
    out = []
    seen = set()
    for item in items:
        if item in seen:
            continue
        seen.add(item)
        out.append(item)
        if len(out) >= limit:
            break
    return out


def main() -> int:
    args = parse_args()
    cards = load_scorecards(args.input_dir)
    if args.profile:
        cards = [card for card in cards if card.get("profile") == args.profile]
    cards = cards[-args.limit :]

    if not cards:
        text = "# Terminal Agent Improvement Report\n\nNo scored runs found.\n"
        if args.output:
            args.output.write_text(text)
        print(text, end="")
        return 0

    by_profile: dict[str, list[dict]] = defaultdict(list)
    for card in cards:
        by_profile[card["profile"]].append(card)

    lines = ["# Terminal Agent Improvement Report", ""]
    lines.append("| Profile | Runs | Latest | Best | Target | Band |")
    lines.append("|---|---:|---:|---:|---:|---|")
    for profile, profile_cards in sorted(by_profile.items()):
        latest = profile_cards[-1]
        best = max(profile_cards, key=lambda card: card["total_score"])
        lines.append(
            "| {profile} | {runs} | {latest_score}/{latest_max} | {best_score}/{best_max} | {target} | {band} |".format(
                profile=profile,
                runs=len(profile_cards),
                latest_score=latest["total_score"],
                latest_max=latest["max_score"],
                best_score=best["total_score"],
                best_max=best["max_score"],
                target=latest["target_score"],
                band=latest["band"],
            )
        )

    lines.append("")
    lines.append("## Profile Trends")
    lines.append("")
    for profile, profile_cards in sorted(by_profile.items()):
        latest = profile_cards[-1]
        best = max(profile_cards, key=lambda card: card["total_score"])
        lines.append(f"### {profile}")
        lines.append("")
        lines.append(
            "- Score history: `{}`".format(history_tail(profile_cards))
        )
        lines.append(f"- Trend chart: `{sparkline(profile_cards)}`")
        lines.append(
            "- Latest vs target: `{}/{}` vs `{}`".format(
                latest["total_score"],
                latest["max_score"],
                latest["target_score"],
            )
        )
        lines.append(
            "- Best run: `{}/{}` ({})".format(
                best["total_score"],
                best["max_score"],
                best["band"],
            )
        )
        dim_rows = average_dimension_scores(profile_cards)
        if dim_rows:
            lines.append("- Average dimensions:")
            for label, avg_score, max_score in dim_rows:
                lines.append(f"  - {label}: {avg_score}/{max_score}")
        recent_lessons = unique_recent(
            [
                lesson
                for card in reversed(profile_cards)
                for lesson in card.get("lessons", [])
            ]
        )
        if recent_lessons:
            lines.append("- Recurring lessons:")
            for lesson in recent_lessons:
                lines.append(f"  - {lesson}")
        recent_focus = unique_recent(
            [
                focus
                for card in reversed(profile_cards)
                for focus in card.get("next_focus", [])
            ]
        )
        if recent_focus:
            lines.append("- Current focus:")
            for focus in recent_focus:
                lines.append(f"  - {focus}")
        lines.append("")

    lines.append("## Recent Runs")
    lines.append("")
    for card in reversed(cards):
        timestamp = card.get("scored_at", "")
        try:
            timestamp = datetime.fromisoformat(timestamp.replace("Z", "+00:00")).strftime(
                "%Y-%m-%d %H:%M"
            )
        except Exception:
            pass
        lines.append(
            "- `{}` · `{}` · {}/{} · {}{}".format(
                timestamp,
                card["profile"],
                card["total_score"],
                card["max_score"],
                card["band"],
                f" · {card['summary']}" if card.get("summary") else "",
            )
        )
        for lesson in card.get("lessons", [])[:3]:
            lines.append(f"  - Lesson: {lesson}")
        for focus in card.get("next_focus", [])[:3]:
            lines.append(f"  - Next: {focus}")

    text = "\n".join(lines) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text)
    print(text, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
