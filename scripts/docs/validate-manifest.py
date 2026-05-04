#!/usr/bin/env python3
"""Validate the docs manifest that powers con.nowledge.co/docs."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "docs" / "manifest.json"
ROUTE_RE = re.compile(r"^/[-a-z0-9_/]+/$")


def iter_items(manifest: dict):
    for item in manifest.get("extra", []):
        yield "extra", item
    for group in manifest.get("groups", []):
        label = group.get("label", "<missing group label>")
        for item in group.get("items", []):
            yield label, item


def default_route(repo_path: str) -> str:
    explicit = {
        "README.md": "/docs/",
        "docs/README.md": "/docs/index/",
        "CHANGELOG.md": "/changelog/",
        "HACKING.md": "/docs/hacking/",
        "DESIGN.md": "/docs/design/",
        "LICENSE": "/docs/license/",
    }
    if repo_path in explicit:
        return explicit[repo_path]
    clean = re.sub(r"^docs/", "", repo_path)
    clean = re.sub(r"\.(md|markdown)$", "", clean)
    return "/" + re.sub(r"/+", "/", f"docs/{clean}/")


def main() -> int:
    errors: list[str] = []

    try:
        manifest = json.loads(MANIFEST.read_text())
    except Exception as exc:  # noqa: BLE001
        print(f"docs manifest is not valid JSON: {exc}", file=sys.stderr)
        return 1

    if manifest.get("version") != 1:
        errors.append("manifest.version must be 1")
    if manifest.get("repository") != "nowledge-co/con-terminal":
        errors.append("manifest.repository must be nowledge-co/con-terminal")
    if not manifest.get("groups"):
        errors.append("manifest.groups must not be empty")

    seen_routes: dict[str, str] = {}
    seen_pages: set[str] = set()

    for group_label, item in iter_items(manifest):
        label = item.get("label")
        repo_path = item.get("path")
        if not label:
            errors.append(f"{group_label}: item is missing label")
        if not repo_path:
            errors.append(f"{group_label}: {label or '<missing label>'} is missing path")
            continue

        target = ROOT / repo_path
        if not target.is_file():
            errors.append(f"{group_label}: {label or repo_path} points to missing file {repo_path}")

        route = item.get("route") or default_route(repo_path)
        if not ROUTE_RE.match(route):
            errors.append(f"{group_label}: {label or repo_path} has invalid route {route}")
        if item.get("hash") and item.get("route"):
            errors.append(f"{group_label}: {label or repo_path} should not set both hash and route")

        if not item.get("hash"):
            if route in seen_routes and seen_routes[route] != repo_path:
                errors.append(f"route {route} is used by both {seen_routes[route]} and {repo_path}")
            seen_routes[route] = repo_path
            seen_pages.add(repo_path)

    if "README.md" not in seen_pages:
        errors.append("README.md must be included as a renderable page")
    if "CHANGELOG.md" not in seen_pages:
        errors.append("CHANGELOG.md must be included as a renderable page")

    if errors:
        for error in errors:
            print(f"docs manifest error: {error}", file=sys.stderr)
        return 1

    print(f"docs manifest ok: {len(seen_pages)} pages, {len(seen_routes)} routes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
