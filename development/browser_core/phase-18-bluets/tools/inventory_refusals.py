#!/usr/bin/env python3
"""Lists every place BlueTS refuses or defers a TypeScript form, from the source.

A refusal is a diagnostic whose text says a form is not supported, not lowered,
not in the initial matrix, or not yet implemented. The list is the compiler's own
account of what it knows it does not do; `COMPATIBILITY_INVENTORY.md` embeds the
output and adds the gaps the compiler cannot know about.

    python3 tools/inventory_refusals.py [repo-root]
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

PHRASES = re.compile(
    r"not supported|not in the initial|not lowered|is not emitted yet|not yet|"
    r"unsupported|cannot be lowered|are refused|is refused|not valid with|"
    r"is not implemented|require the module bridge|needs? .{0,40}to be",
    re.I,
)
STRING = re.compile(r'"((?:[^"\\]|\\.)*)"')


SITE = re.compile(r"(?:unsupported\(|UnsupportedSyntax|UnsupportedRuntimeTarget)")
MESSAGE = re.compile(r'(?:format!\()?\s*"((?:[^"\\]|\\.)*)"', re.S)


def messages(text: str):
    """Yields (offset, message) for each refusal site: the first string literal
    that follows the site within the same expression."""
    for site in SITE.finditer(text):
        window = text[site.end() : site.end() + 700]
        # The message is the first string literal that is not just a module id.
        for literal in MESSAGE.finditer(window):
            body = literal.group(1)
            if len(body) >= 12 and " " in body:
                yield site.start(), body
                break


def main() -> None:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[4]
    sources = []
    for crate in ("backend/bluets/src", "backend/bluets-bluejs/src"):
        sources += sorted((root / crate).rglob("*.rs"))
    groups = defaultdict(set)
    for path in sources:
        relative = path.relative_to(root).as_posix()
        if "/tests" in relative or relative.endswith("tests.rs"):
            continue
        text = path.read_text()
        for offset, body in messages(text):
            number = text.count("\n", 0, offset) + 1
            body = " ".join(body.replace('\\"', '"').replace("\\\\", "\\").split())
            # Malformed-input diagnostics are not feature gaps.
            if re.match(r"(expected|unterminated|invalid|empty template|unexpected)", body, re.I):
                continue
            area = relative.split("/src/")[1].split("/")[0].removesuffix(".rs")
            groups[area].add((body, f"{relative}:{number}"))
    total = sum(len(v) for v in groups.values())
    print(f"{total} refusal sites in {len(groups)} areas\n")
    for area in sorted(groups):
        print(f"### {area} ({len(groups[area])})\n")
        for body, where in sorted(groups[area]):
            print(f"- `{where}` — {body}")
        print()


if __name__ == "__main__":
    main()
