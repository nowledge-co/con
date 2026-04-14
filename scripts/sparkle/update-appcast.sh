#!/usr/bin/env bash
#
# Add or update an entry in a Sparkle appcast XML file.
#
# Usage:
#   ./update-appcast.sh
#
# Environment (all required unless noted):
#   APPCAST_FILE          — path to appcast XML (created if absent)
#   APPCAST_TITLE         — feed title (e.g. "con")
#   APPCAST_LINK          — feed link (e.g. "https://con-releases.nowledge.co")
#   ITEM_TITLE            — item title (e.g. "Version 0.2.0")
#   ITEM_VERSION          — CFBundleVersion / sparkle:version (build number)
#   ITEM_SHORT_VERSION    — CFBundleShortVersionString (e.g. "0.2.0")
#   ITEM_URL              — direct download URL for the artifact
#   ITEM_LENGTH           — file size in bytes
#   ITEM_SIGNATURE        — Ed25519 signature (base64) from sign_update
#   ITEM_MIN_OS           — sparkle:minimumSystemVersion (default: 10.15.7)
#   ITEM_PUB_DATE         — RFC 2822 date (default: now)
#   MAX_ITEMS             — max items to keep in feed (default: 20)

set -euo pipefail

: "${APPCAST_FILE:?required}"
: "${APPCAST_TITLE:?required}"
: "${APPCAST_LINK:?required}"
: "${ITEM_TITLE:?required}"
: "${ITEM_VERSION:?required}"
: "${ITEM_SHORT_VERSION:?required}"
: "${ITEM_URL:?required}"
: "${ITEM_LENGTH:?required}"
: "${ITEM_SIGNATURE:?required}"

ITEM_MIN_OS="${ITEM_MIN_OS:-10.15.7}"
ITEM_PUB_DATE="${ITEM_PUB_DATE:-$(date -u '+%a, %d %b %Y %H:%M:%S +0000')}"
MAX_ITEMS="${MAX_ITEMS:-20}"

exec python3 - <<'PYTHON'
import os, sys
import xml.etree.ElementTree as ET
from xml.dom import minidom

SPARKLE_NS = "http://www.andymatuschak.org/xml-namespaces/sparkle"

ET.register_namespace("sparkle", SPARKLE_NS)

appcast_file = os.environ["APPCAST_FILE"]
title = os.environ["APPCAST_TITLE"]
link = os.environ["APPCAST_LINK"]
item_title = os.environ["ITEM_TITLE"]
version = os.environ["ITEM_VERSION"]
short_version = os.environ["ITEM_SHORT_VERSION"]
url = os.environ["ITEM_URL"]
length = os.environ["ITEM_LENGTH"]
signature = os.environ["ITEM_SIGNATURE"]
min_os = os.environ.get("ITEM_MIN_OS", "10.15.7")
pub_date = os.environ.get("ITEM_PUB_DATE", "")
max_items = int(os.environ.get("MAX_ITEMS", "20"))


def make_item():
    item = ET.Element("item")
    ET.SubElement(item, "title").text = item_title
    ET.SubElement(item, "pubDate").text = pub_date
    ET.SubElement(item, f"{{{SPARKLE_NS}}}version").text = version
    ET.SubElement(item, f"{{{SPARKLE_NS}}}shortVersionString").text = short_version
    ET.SubElement(item, f"{{{SPARKLE_NS}}}minimumSystemVersion").text = min_os
    enc = ET.SubElement(item, "enclosure")
    enc.set("url", url)
    enc.set("length", length)
    enc.set("type", "application/octet-stream")
    enc.set(f"{{{SPARKLE_NS}}}edSignature", signature)
    return item


def prettify(elem):
    """Return pretty-printed XML with proper indentation."""
    rough = ET.tostring(elem, encoding="unicode", xml_declaration=False)
    dom = minidom.parseString(rough)
    lines = dom.toprettyxml(indent="  ", encoding=None).split("\n")
    # Remove blank lines and the XML declaration minidom adds
    return "\n".join(
        line for line in lines
        if line.strip() and not line.startswith("<?xml")
    )


if os.path.isfile(appcast_file):
    tree = ET.parse(appcast_file)
    root = tree.getroot()
    channel = root.find("channel")
else:
    root = ET.Element("rss", version="2.0")
    # Namespace declarations are handled by ET.register_namespace above
    channel = ET.SubElement(root, "channel")
    ET.SubElement(channel, "title").text = title
    ET.SubElement(channel, "link").text = link
    ET.SubElement(channel, "description").text = "Most recent changes"
    ET.SubElement(channel, "language").text = "en"

# Remove any existing item with the same build version
for existing in channel.findall("item"):
    v = existing.find(f"{{{SPARKLE_NS}}}version")
    if v is not None and v.text == version:
        channel.remove(existing)

# Insert new item after the last non-item element (title, link, etc.)
insert_pos = 0
for i, child in enumerate(channel):
    if child.tag != "item":
        insert_pos = i + 1
    else:
        break

channel.insert(insert_pos, make_item())

# Enforce max items
items = channel.findall("item")
while len(items) > max_items:
    channel.remove(items.pop())

# Write with XML declaration
with open(appcast_file, "w", encoding="utf-8") as f:
    f.write('<?xml version="1.0" encoding="utf-8"?>\n')
    f.write(prettify(root))
    f.write("\n")

print(f"[appcast] Updated {appcast_file} — {short_version} (build {version})")
PYTHON
