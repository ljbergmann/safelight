#!/usr/bin/env python3
"""Writes app/Acknowledgements.txt: the licences of everything the app ships.

The Rust core's dependencies for the iPad (cargo metadata, aarch64-apple-ios),
ONNX Runtime and the AI Denoise model bundled in app/Models. Each crate's own licence files are copied as they are, since
MIT, BSD and Apache ask that their notices ship with the software; identical
texts are written once, after the crates that share them. A crate that ships
no licence file is listed with its SPDX expression.

Run it after changing the core's dependencies or ONNX Runtime:

    scripts/acknowledgements.py
"""

import json
import os
import subprocess
import sys
from collections import OrderedDict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "app" / "Acknowledgements.txt"
NAMES = ("license", "licence", "copying", "notice", "unlicense")


def licence_files(directory: Path) -> list[Path]:
    """The licence files at the top of a package, by name."""
    found = []
    for entry in sorted(directory.iterdir()):
        stem = entry.name.lower()
        if entry.is_file() and any(stem.startswith(n) for n in NAMES):
            found.append(entry)
    return found


def main() -> int:
    metadata = json.loads(
        subprocess.check_output(
            [
                "cargo",
                "metadata",
                "--format-version",
                "1",
                "--filter-platform",
                "aarch64-apple-ios",
                "--manifest-path",
                str(ROOT / "core" / "Cargo.toml"),
            ]
        )
    )
    # Only what the core builds with: no dev- or build-only crates.
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    packages = {p["id"]: p for p in metadata["packages"]}
    root = metadata["resolve"]["root"]
    shipped, todo = set(), [root]
    while todo:
        current = todo.pop()
        if current in shipped:
            continue
        shipped.add(current)
        for dep in nodes[current]["deps"]:
            if any(k["kind"] in (None, "normal") for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    shipped.discard(root)

    texts: "OrderedDict[str, list[str]]" = OrderedDict()
    bare: list[str] = []
    for package in sorted((packages[i] for i in shipped), key=lambda p: p["name"].lower()):
        label = f'{package["name"]} {package["version"]}'
        directory = Path(package["manifest_path"]).parent
        files = licence_files(directory)
        # A crate of a local workspace (RAWmakase's) shares the repository's.
        while not files and package.get("source") is None and directory != ROOT and directory.parent != directory:
            directory = directory.parent
            files = licence_files(directory)
        if not files:
            bare.append(f'{label} ({package.get("license") or "see its source"})')
            continue
        for path in files:
            text = path.read_text(errors="replace").strip()
            texts.setdefault(text, []).append(label)

    ort = ROOT / "build" / "onnxruntime.xcframework" / "LICENSE"
    if ort.exists():
        version = (ROOT / "build" / "onnxruntime.xcframework" / ".version")
        name = "ONNX Runtime " + (version.read_text().strip() if version.exists() else "")
        texts.setdefault(ort.read_text(errors="replace").strip(), []).append(name.strip())
    else:
        print("note: build/onnxruntime.xcframework is missing; run scripts/build-ort.sh first", file=sys.stderr)

    # The AI Denoise model bundled for the Neural Engine (app/Models): its credit, what
    # was changed, and its licence, as CC BY 4.0 asks.
    models = ROOT / "app" / "Models"
    notice = (models / "RawNIND-ANE.NOTICE").read_text().strip()
    texts.setdefault(notice + "\n\n" + (models / "RawNIND-ANE.LICENSE").read_text().strip(), []).append(
        "RawNIND UtNet2 (Bayer), converted for the Neural Engine")

    lines = [
        "Safelight is built on RAWmakase and on the open-source software below.",
        "Each licence follows the software it covers.",
        "",
    ]
    for text, users in texts.items():
        lines.append("=" * 72)
        lines.extend(sorted(set(users)))
        lines.append("-" * 72)
        lines.append(text)
        lines.append("")
    if bare:
        lines.append("=" * 72)
        lines.append("Without a licence file of their own, under the licence named:")
        lines.extend(bare)
        lines.append("")
    OUT.write_text("\n".join(lines))
    print(f"{OUT.relative_to(ROOT)}: {len(shipped)} crates, {len(texts)} licence texts, {OUT.stat().st_size // 1024} KB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
