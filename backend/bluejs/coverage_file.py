# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Run the complete BlueJS Rust coverage suite and inspect one source file.

The test suite is deliberately never filtered by source path: any integration
test may exercise the requested file. Each invocation starts with fresh LLVM
coverage artifacts, so the result cannot come from a previous checkout.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
from datetime import date
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SOURCE_ROOT = REPO_ROOT / "backend/bluejs/src"
MACOS_REPORT = (
    REPO_ROOT
    / "development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md"
)
METRICS = ("lines", "functions", "regions")

# Files without executable coverage targets are audited here. An unexpected
# uninstrumented source file is an error rather than a silently omitted row.
NO_COUNTER_REASONS = {
    "compiler/expressions/tests.rs": "Test source; not a coverage target",
    "heap/tests.rs": "Test source; not a coverage target",
    "lib.rs": "Declarations/re-exports only; no executable code",
    "parser/tests.rs": "Test source; not a coverage target",
    "vm/builtins/execution.rs": "Declarations/re-exports only; no executable code",
    "vm/builtins/native_dispatch.rs": "Declarations/re-exports only; no executable code",
    "vm/builtins/tests.rs": "Test source; not a coverage target",
    "vm/temporal/conversion.rs": "Declarations/re-exports only; no executable code",
    "vm/temporal/dates.rs": "Declarations/re-exports only; no executable code",
    "vm/temporal/iso/tests.rs": "Test source; not a coverage target",
    "vm/temporal/plain_date.rs": "Declarations/re-exports only; no executable code",
    "vm/temporal/plain_date/tests.rs": "Test source; not a coverage target",
    "vm/temporal/zoned.rs": "Declarations/re-exports only; no executable code",
    "vm/tests.rs": "Test source; not a coverage target",
}


def source_path(raw: str) -> Path:
    """Accept an absolute, repository-relative, or src-relative Rust path."""
    supplied = Path(raw)
    candidates = (
        [supplied]
        if supplied.is_absolute()
        else [Path.cwd() / supplied, REPO_ROOT / supplied, SOURCE_ROOT / supplied]
    )
    for candidate in candidates:
        path = candidate.resolve()
        if path.is_file() and path.suffix == ".rs" and path.is_relative_to(SOURCE_ROOT):
            return path
    raise ValueError(f"not a BlueJS Rust source file: {raw}")


def checked_export(payload: dict) -> tuple[dict[Path, dict], dict]:
    """Reject partial, foreign, or unreconciled LLVM coverage exports."""
    data = payload.get("data")
    if not isinstance(data, list) or len(data) != 1:
        raise ValueError("expected exactly one LLVM coverage data set")
    entry = data[0]
    files: dict[Path, dict] = {}
    for item in entry["files"]:
        path = Path(item["filename"]).resolve()
        if not path.is_relative_to(SOURCE_ROOT) or path in files:
            raise ValueError(f"foreign or duplicate source file in coverage export: {path}")
        summary = item["summary"]
        for metric in METRICS:
            count = summary[metric]["count"]
            covered = summary[metric]["covered"]
            if (
                not isinstance(count, int)
                or not isinstance(covered, int)
                or not 0 <= covered <= count
                or count == 0
            ):
                raise ValueError(f"invalid {metric} counters for {path}")
        files[path] = summary

    source_files = set(SOURCE_ROOT.rglob("*.rs"))
    if not files.keys() <= source_files:
        raise ValueError("coverage export contains a missing source file")
    missing = {
        path.relative_to(SOURCE_ROOT).as_posix()
        for path in source_files - files.keys()
    }
    unexpected = missing - NO_COUNTER_REASONS.keys()
    if unexpected:
        raise ValueError(f"unreviewed source files without coverage counters: {sorted(unexpected)}")

    totals = entry["totals"]
    for metric in METRICS:
        count = sum(summary[metric]["count"] for summary in files.values())
        covered = sum(summary[metric]["covered"] for summary in files.values())
        if (count, covered) != (totals[metric]["count"], totals[metric]["covered"]):
            raise ValueError(f"{metric} totals do not reconcile with per-file data")
        if not 0 <= covered <= count:
            raise ValueError(f"invalid {metric} coverage totals")
    return files, totals


def metric_cell(metric: dict) -> str:
    count, covered = metric["count"], metric["covered"]
    if (
        not isinstance(count, int)
        or not isinstance(covered, int)
        or not 0 <= covered <= count
        or count == 0
    ):
        raise ValueError(f"invalid coverage metric: {metric}")
    return f"{covered:,} / {count:,} ({covered / count * 100:.2f}%)"


def is_complete(summary: dict | None) -> bool:
    return summary is not None and all(
        summary[metric]["count"] > 0
        and summary[metric]["covered"] == summary[metric]["count"]
        for metric in METRICS
    )


def markdown_row(path: Path, summary: dict | None) -> str:
    relative = path.relative_to(SOURCE_ROOT).as_posix()
    link = f"[`{relative}`](../../../backend/bluejs/src/{relative})"
    if summary is None:
        note = NO_COUNTER_REASONS[relative]
        return f"| {link} | - | - | - | - | {note} |"
    cells = " | ".join(metric_cell(summary[metric]) for metric in METRICS)
    completed = "☑" if is_complete(summary) else "☐"
    return f"| {link} | {cells} | {completed} |  |"


def provenance() -> str:
    revision = subprocess.check_output(
        ["git", "rev-parse", "--short=8", "HEAD"], cwd=REPO_ROOT, text=True
    ).strip()
    dirty = subprocess.check_output(
        ["git", "status", "--porcelain"], cwd=REPO_ROOT, text=True
    )
    state = " with uncommitted changes" if dirty else ""
    system = platform.system()
    release = platform.mac_ver()[0] if system == "Darwin" else platform.release()
    rustc = subprocess.check_output(["rustc", "--version"], text=True).strip()
    llvm_cov = subprocess.check_output(["cargo", "llvm-cov", "--version"], text=True).strip()
    return (
        f"commit `{revision}`{state} on {system} {release} (`{platform.machine()}`), "
        f"{rustc} and `{llvm_cov}`"
    )


def report_section(files: dict[Path, dict], totals: dict) -> str:
    sources = sorted(
        SOURCE_ROOT.rglob("*.rs"),
        key=lambda path: path.relative_to(SOURCE_ROOT).as_posix(),
    )
    unmeasured = len(sources) - len(files)
    rows = [markdown_row(path, files.get(path)) for path in sources]
    total_cells = " | ".join(f"**{metric_cell(totals[metric])}**" for metric in METRICS)
    total_complete = "☑" if is_complete(totals) else "☐"
    rows.append(
        f"| **Total ({len(files)} instrumented files)** | {total_cells} | {total_complete} |  |"
    )
    measurement_note = (
        f"This is a separate BlueJS coverage measurement at {provenance()}. "
        "It measures the Rust test suite independently of the Test262 "
        "inventory and historical verification above. "
        "`python3 backend/bluejs/coverage_file.py --update-macos-report` "
        "cleaned prior LLVM artifacts, ran the complete default BlueJS Rust "
        "test suite, and exported fresh per-file JSON. The opt-in Node "
        "oracle and external full Test262 runner were not included. "
        "Workspace coverage was not remeasured at this revision."
    )
    table_note = (
        "Each measured cell shows covered / instrumented and the coverage "
        f"rate. All {len(sources)} Rust files under `backend/bluejs/src/` are "
        f"listed: {len(files)} have LLVM counters; {unmeasured} use `-` with "
        "an individual reason in `Note`. A `0%` result requires a positive "
        "instrumented denominator and zero covered units. `☑` means "
        "**lines, functions and regions all reach 100%**; `☐` means at least "
        "one is below 100%. The total aggregates only instrumented files. "
        "Region coverage is separate from branch coverage. To rerun any one "
        "file independently, use `python3 backend/bluejs/coverage_file.py ast.rs` "
        "(replace `ast.rs` with its source path). Each invocation reruns the "
        "entire test suite, since tests outside a file can still exercise it."
    )
    return "\n".join(
        [
            f"## Later BlueJS per-file coverage ({date.today().isoformat()})",
            "",
            measurement_note,
            "",
            table_note,
            "",
            "| Source file (relative to `backend/bluejs/src/`) | Lines | "
            "Functions | Regions | Complete | Note |",
            "| --- | ---: | ---: | ---: | :---: | --- |",
            *rows,
            "",
        ]
    )


def update_macos_report(files: dict[Path, dict], totals: dict) -> None:
    if platform.system() != "Darwin":
        raise ValueError("the macOS report can only be regenerated on macOS")
    text = MACOS_REPORT.read_text()
    start = text.index("## Later BlueJS per-file coverage (")
    end_markers = (
        "## Historical differences from the other platforms",
        "## Differences from the other platforms",
    )
    end = min(
        (position for marker in end_markers if (position := text.find(marker, start)) >= 0),
        default=-1,
    )
    if end < 0:
        raise ValueError("cannot find the section after BlueJS per-file coverage")
    new_text = text[:start] + report_section(files, totals) + "\n" + text[end:]
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=MACOS_REPORT.parent, delete=False
    ) as output:
        output.write(new_text)
        temporary = Path(output.name)
    temporary.chmod(MACOS_REPORT.stat().st_mode & 0o777)
    os.replace(temporary, MACOS_REPORT)


def run_coverage() -> tuple[dict[Path, dict], dict]:
    with tempfile.TemporaryDirectory(prefix="bluejs-coverage-") as tmp:
        output = Path(tmp) / "coverage.json"
        subprocess.run(
            ["cargo", "llvm-cov", "clean", "--workspace"], cwd=REPO_ROOT, check=True
        )
        subprocess.run(
            [
                "cargo",
                "llvm-cov",
                "-p",
                "blueice-bluejs",
                "--json",
                "--summary-only",
                "--output-path",
                str(output),
            ],
            cwd=REPO_ROOT,
            check=True,
        )
        return checked_export(json.loads(output.read_text()))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "source_file",
        nargs="?",
        help="BlueJS src-relative, repo-relative, or absolute .rs path",
    )
    parser.add_argument(
        "--update-macos-report",
        action="store_true",
        help="regenerate every row in the macOS report",
    )
    args = parser.parse_args(argv)
    if not args.source_file and not args.update_macos_report:
        parser.error("provide a source file or --update-macos-report")
    try:
        path = source_path(args.source_file) if args.source_file else None
        files, totals = run_coverage()
        if args.update_macos_report:
            update_macos_report(files, totals)
            print(f"Updated {MACOS_REPORT.relative_to(REPO_ROOT)}")
        if path is not None:
            print(markdown_row(path, files.get(path)))
            print(f"Complete: {'yes' if is_complete(files.get(path)) else 'no'}")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"coverage_file.py: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
