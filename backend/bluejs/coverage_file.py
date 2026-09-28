# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Run the complete BlueJS Rust coverage suite and inspect one source file.

The test suite is deliberately never filtered by source path: any integration
test may exercise the requested file. Each invocation clears the previous LLVM
execution profiles while reusing Cargo's instrumented build artifacts. On
Linux, disposable DWARF sections are stripped from reusable test executables.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from datetime import date
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SOURCE_ROOT = REPO_ROOT / "backend/bluejs/src"
MACOS_REPORT = (
    REPO_ROOT
    / "development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md"
)
LINUX_REPORT = (
    REPO_ROOT
    / "development/browser_core/phase-13-bluejs-engine/TEST262_LINUX_REPORT.md"
)
METRICS = ("lines", "functions", "regions")
TEST_GROWTH_LIMIT = 32 * 1024**3
TEST_TARGET_LIMIT = 128 * 1024**3
HOST_GROWTH_LIMIT = 32 * 1024**3
HOST_FREE_FLOOR = 20 * 1024**3
TEST_RECLAIM_THRESHOLD = 4 * 1024**3

# Files without executable coverage targets are audited here. An unexpected
# uninstrumented source file is an error rather than a silently omitted row.
NO_COUNTER_REASONS = {
    "compiler/expressions/tests.rs": "Test source; not a coverage target",
    "compiler/functions/tests.rs": "Test source; not a coverage target",
    "compiler/private_validation/tests.rs": "Test source; not a coverage target",
    "compiler/statements/tests.rs": "Test source; not a coverage target",
    "heap/tests.rs": "Test source; not a coverage target",
    "lib.rs": "Declarations/re-exports only; no executable code",
    "parser/tests.rs": "Test source; not a coverage target",
    "vm/builtins/execution.rs": "Declarations/re-exports only; no executable code",
    "vm/builtins/native_dispatch.rs": "Declarations/re-exports only; no executable code",
    "vm/builtins/numbers/tests.rs": "Test source; not a coverage target",
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


def source_union_export(payload: dict, show_text: str) -> tuple[dict[Path, dict], dict]:
    """Count each source location once across unit and integration binaries."""
    raw_files, raw_totals = checked_export(payload)
    regions: dict[Path, dict[tuple[int, ...], int]] = {
        path: {} for path in raw_files
    }
    functions: dict[Path, dict[tuple[int, ...], int]] = {
        path: {} for path in raw_files
    }
    for function in payload["data"][0]["functions"]:
        for index, filename in enumerate(function["filenames"]):
            path = Path(filename).resolve()
            if path not in raw_files:
                continue
            code_regions = [
                region
                for region in function["regions"]
                if len(region) == 8 and region[5] == index and region[7] == 0
            ]
            if not code_regions:
                continue
            count = function["count"]
            if not isinstance(count, int) or count < 0:
                raise ValueError(f"invalid function counter for {path}")
            function_key = tuple(code_regions[0][:4])
            functions[path][function_key] = max(
                functions[path].get(function_key, 0), count
            )
            for region in code_regions:
                hit = region[4]
                if not isinstance(hit, int) or hit < 0:
                    raise ValueError(f"invalid region counter for {path}")
                key = tuple(region[:4])
                regions[path][key] = max(regions[path].get(key, 0), hit)

    lines: dict[Path, dict[int, bool]] = {path: {} for path in raw_files}
    source_rows: dict[Path, set[int]] = {path: set() for path in raw_files}
    active: Path | None = None
    for row in show_text.splitlines():
        if row.startswith(str(SOURCE_ROOT)) and row.endswith(".rs:"):
            active = Path(row[:-1]).resolve()
            if active not in raw_files:
                raise ValueError(f"unexpected source file in text coverage: {active}")
            continue
        if active is None:
            continue
        match = re.match(r"^\s*(\d+)\|([^|]*)\|", row)
        if match is None:
            continue
        number = int(match.group(1))
        if number in source_rows[active]:
            raise ValueError(f"duplicate source line in text coverage: {active}:{number}")
        source_rows[active].add(number)
        counter = match.group(2).strip()
        if not counter:
            continue
        if not re.fullmatch(r"0|[1-9][0-9.]*(?:[kMGT])?", counter):
            raise ValueError(f"invalid line counter for {active}: {counter}")
        lines[active][number] = counter != "0"

    summaries: dict[Path, dict] = {}
    totals = {metric: {"count": 0, "covered": 0} for metric in METRICS}
    for path, raw in raw_files.items():
        source_lines = len(path.read_text().splitlines())
        if source_rows[path] != set(range(1, source_lines + 1)):
            raise ValueError(f"incomplete source-line coverage view for {path}")
        counts = {
            "lines": (len(lines[path]), sum(lines[path].values())),
            "functions": (
                len(functions[path]),
                sum(hit > 0 for hit in functions[path].values()),
            ),
            "regions": (len(regions[path]), sum(hit > 0 for hit in regions[path].values())),
        }
        summary = {}
        for metric, (count, covered) in counts.items():
            if count == 0 or count > raw[metric]["count"]:
                raise ValueError(f"invalid source-union {metric} denominator for {path}")
            if metric != "lines" and count != raw[metric]["count"]:
                raise ValueError(f"unreconciled source-union {metric} for {path}")
            summary[metric] = {"count": count, "covered": covered}
            totals[metric]["count"] += count
            totals[metric]["covered"] += covered
        summary["_raw"] = raw
        summaries[path] = summary
    totals["_raw"] = raw_totals
    return summaries, totals


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
    if summary is None:
        return False
    authoritative = summary.get("_raw", summary)
    return all(
        authoritative[metric]["count"] > 0
        and authoritative[metric]["covered"] == authoritative[metric]["count"]
        for metric in METRICS
    )


def markdown_row(path: Path, summary: dict | None) -> str:
    relative = path.relative_to(SOURCE_ROOT).as_posix()
    link = f"[`{relative}`](../../../backend/bluejs/src/{relative})"
    if summary is None:
        note = NO_COUNTER_REASONS[relative]
        return f"| {link} | - | - | - | - | {note} |"
    raw = summary.get("_raw", summary)
    cells = " | ".join(metric_cell(raw[metric]) for metric in METRICS)
    completed = "☑" if is_complete(summary) else "☐"
    note = ""
    if summary is not raw and any(summary[metric] != raw[metric] for metric in METRICS):
        union_counts = ", ".join(
            f"{metric} {summary[metric]['covered']:,}/{summary[metric]['count']:,}"
            for metric in METRICS
        )
        note = f"Unique source-location union: {union_counts}"
    return f"| {link} | {cells} | {completed} | {note} |"


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


def report_section(
    files: dict[Path, dict], totals: dict, update_option: str, include_test262: bool = False
) -> str:
    sources = sorted(
        SOURCE_ROOT.rglob("*.rs"),
        key=lambda path: path.relative_to(SOURCE_ROOT).as_posix(),
    )
    unmeasured = len(sources) - len(files)
    rows = [markdown_row(path, files.get(path)) for path in sources]
    complete_files = sum(is_complete(summary) for summary in files.values())
    raw_totals = totals.get("_raw", totals)
    total_cells = " | ".join(f"**{metric_cell(raw_totals[metric])}**" for metric in METRICS)
    total_complete = "☑" if is_complete(totals) else "☐"
    rows.append(
        f"| **Total ({len(files)} instrumented files)** | {total_cells} | {total_complete} |  |"
    )
    test262_option = " --include-test262" if include_test262 else ""
    interpreter = "python" if include_test262 else "python3"
    reproduction = (
        "With Python and PyYAML installed, reproduce it with "
        if include_test262
        else "Reproduce it with "
    )
    test262_scope = (
        "then ran the full pinned Test262 inventory with the instrumented "
        "adapter, "
        if include_test262
        else ""
    )
    excluded_scope = "" if include_test262 else "The full Test262 runner was not included. "
    measurement_note = (
        f"This is a separate BlueJS coverage measurement at {provenance()}. "
        "It is independent of the Test262 status and historical verification above. "
        f"{reproduction}"
        f"`{interpreter} backend/bluejs/coverage_file.py {update_option}{test262_option}`. "
        "This measurement cleared prior LLVM execution profiles, reused instrumented Cargo "
        "build artifacts, ran the complete default BlueJS Rust test suite, "
        f"{test262_scope}"
        "exported fresh per-file JSON and source-line text, and released raw "
        "profiles and incremental compilation caches afterward. On Linux, "
        "test-binary DWARF was removed while retaining the coverage maps. "
        f"{excluded_scope}The opt-in Node oracle was not included. "
        "Workspace coverage was not remeasured at this revision."
    )
    table_note = (
        "Each measured cell shows LLVM JSON's raw covered / instrumented "
        "counts and the coverage "
        f"rate. All {len(sources)} Rust files under `backend/bluejs/src/` are "
        f"listed: {len(files)} have LLVM counters; {unmeasured} use `-` with "
        "an individual reason in `Note`. A `0%` result requires a positive "
        "instrumented denominator and zero covered units. `☑` means "
        "**lines, functions and regions all reach 100%**; `☐` means at least "
        "one is below 100%. The total aggregates only instrumented files. "
        "LLVM can count separate compiled instances of the same source in "
        "different test binaries. `Note` shows the union across those binaries "
        "when its counts differ: each source location is counted once and "
        "considered covered if any binary executes it. The raw LLVM counts "
        "in the main columns determine completion. Region coverage "
        "is separate from branch coverage. "
        "To rerun any one "
        f"file independently, use `python backend/bluejs/coverage_file.py "
        f"ast.rs{test262_option}` (replace `ast.rs` with its source path). "
        "Each invocation reruns the entire measured suite, since tests "
        "outside a file can still exercise it."
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
            f"Raw LLVM lines, functions, and regions are all complete in "
            f"**{complete_files} of {len(files)}** instrumented files; "
            f"**{len(files) - complete_files}** remain incomplete.",
            "",
        ]
    )


def update_report(
    report: Path,
    files: dict[Path, dict],
    totals: dict,
    update_option: str,
    include_test262: bool = False,
) -> None:
    text = report.read_text()
    start = text.find("## Later BlueJS per-file coverage (")
    end_markers = (
        "## Historical differences from the other platforms",
        "## Differences from the other platforms",
    )
    after_start = max(start, 0)
    end = min(
        (
            position
            for marker in end_markers
            if (position := text.find(marker, after_start)) >= 0
        ),
        default=-1,
    )
    if end < 0:
        raise ValueError("cannot find the section after BlueJS per-file coverage")
    if start < 0:
        start = end
    new_text = (
        text[:start]
        + report_section(files, totals, update_option, include_test262)
        + "\n"
        + text[end:]
    )
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=report.parent, delete=False
    ) as output:
        output.write(new_text)
        temporary = Path(output.name)
    temporary.chmod(report.stat().st_mode & 0o777)
    os.replace(temporary, report)


def update_macos_report(
    files: dict[Path, dict], totals: dict, include_test262: bool = False
) -> None:
    if platform.system() != "Darwin":
        raise ValueError("the macOS report can only be regenerated on macOS")
    update_report(MACOS_REPORT, files, totals, "--update-macos-report", include_test262)


def update_linux_report(
    files: dict[Path, dict], totals: dict, include_test262: bool = False
) -> None:
    if platform.system() != "Linux":
        raise ValueError("the Linux report can only be regenerated on Linux")
    update_report(LINUX_REPORT, files, totals, "--update-linux-report", include_test262)


def prune_superseded_test_executables(target: Path) -> None:
    """Release old integration-test binaries while retaining current builds.

    Cargo's dep-info identifies the test source. Grouping by executable name
    alone is unsafe: a normal binary and its test harness can share that name.
    Missing old feature variants are rebuilt on demand.
    """
    for deps in (target / "debug/deps", target / "llvm-cov-target/debug/deps"):
        if not deps.is_dir():
            continue
        groups: dict[Path, list[Path]] = {}
        for path in deps.iterdir():
            name, separator, digest = path.name.rpartition("-")
            if (
                not separator
                or not re.fullmatch(r"[0-9a-f]{16}", digest)
                or not path.is_file()
                or not stat.S_IMODE(path.stat().st_mode) & 0o111
            ):
                continue
            dep_info = deps / f"{name}-{digest}.d"
            if not dep_info.is_file():
                continue
            with dep_info.open() as source:
                first_line = source.readline()
            dependencies = first_line.partition(": ")[2].split()
            if not dependencies:
                continue
            test_source = Path(dependencies[0])
            if test_source.parent.name == "tests" and test_source.suffix == ".rs":
                groups.setdefault(test_source, []).append(path)
        for paths in groups.values():
            newest = max(paths, key=lambda path: path.stat().st_mtime_ns)
            for path in paths:
                if path != newest:
                    path.unlink()


def strip_test_binary_debug(target: Path, *, min_age_seconds: int = 0) -> None:
    """Release Linux DWARF while keeping test binaries and LLVM coverage maps."""
    if platform.system() != "Linux":
        return
    saved = 0
    stripped = 0
    for deps in (target / "llvm-cov-target/debug/deps", target / "debug/deps"):
        if not deps.is_dir():
            continue
        for path in deps.iterdir():
            try:
                snapshot = path.stat()
            except FileNotFoundError:
                continue
            if (
                not re.fullmatch(r"[^/]+-[0-9a-f]{16}", path.name)
                or not stat.S_ISREG(snapshot.st_mode)
                or not stat.S_IMODE(snapshot.st_mode) & 0o111
                or time.time_ns() - snapshot.st_mtime_ns < min_age_seconds * 1_000_000_000
            ):
                continue
            sections = subprocess.run(
                ["readelf", "--section-headers", "--wide", str(path)],
                capture_output=True,
                text=True,
            )
            if sections.returncode:
                continue
            if ".debug_info" not in sections.stdout:
                continue
            try:
                current = path.stat()
            except FileNotFoundError:
                continue
            if (current.st_size, current.st_mtime_ns) != (
                snapshot.st_size,
                snapshot.st_mtime_ns,
            ):
                continue
            before = snapshot.st_size
            result = subprocess.run(
                ["strip", "--strip-debug", str(path)], capture_output=True, text=True
            )
            if result.returncode:
                if any(
                    transient in result.stderr
                    for transient in (
                        "Text file busy",
                        "file truncated",
                        "file format not recognized",
                    )
                ):
                    continue
                raise RuntimeError(
                    f"could not strip test binary {path}: {result.stderr.strip()}"
                )
            saved += before - path.stat().st_size
            stripped += 1
    if stripped:
        print(f"Released {saved / 1024**3:.1f} GiB of test DWARF from {stripped} binaries")


def cargo_target() -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO_ROOT / "target"))
    return (target if target.is_absolute() else REPO_ROOT / target).resolve()


def target_size_bytes(target: Path) -> int:
    if not target.exists():
        return 0
    result = subprocess.run(
        ["du", "-sk", str(target)], capture_output=True, text=True, check=False
    )
    # Cargo can replace an executable while du walks the directory. In that
    # case du still prints the total for files that remain on disk.
    transient_removals = result.stderr.splitlines() and all(
        "No such file or directory" in line for line in result.stderr.splitlines()
    )
    if result.returncode not in (0, 1) or (result.returncode == 1 and not transient_removals):
        raise RuntimeError(f"could not measure Cargo target size: {result.stderr.strip()}")
    try:
        return int(result.stdout.split()[0]) * 1024
    except (IndexError, ValueError) as error:
        raise RuntimeError("du did not report a Cargo target size") from error


def stop_process_group(process: subprocess.Popen) -> None:
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()


def run_bounded_command(
    argv: list[str],
    *,
    cwd: Path,
    target: Path,
    growth_limit: int = TEST_GROWTH_LIMIT,
    target_limit: int = TEST_TARGET_LIMIT,
    host_growth_limit: int = HOST_GROWTH_LIMIT,
    free_floor: int = HOST_FREE_FLOOR,
    poll_interval: float = 5,
    env: dict[str, str] | None = None,
    accepted_returncodes: tuple[int, ...] = (0,),
) -> None:
    """Stop a test and its children before target or host growth exceeds budget."""
    if target.is_relative_to(cwd):
        strip_test_binary_debug(target)
    starting_size = target_size_bytes(target)
    if starting_size > target_limit:
        raise RuntimeError("Cargo target already exceeds the test disk limit")
    starting_free = shutil.disk_usage(cwd).free
    if starting_free < free_floor:
        raise RuntimeError("insufficient free disk space to start tests")
    print(
        f"Test disk budget: {growth_limit // 1024**3} GiB target growth, "
        f"{target_limit // 1024**3} GiB total target, "
        f"{host_growth_limit // 1024**3} GiB host growth, "
        f"{free_floor // 1024**3} GiB host reserve",
        flush=True,
    )
    process = subprocess.Popen(argv, cwd=cwd, env=env, start_new_session=True)
    try:
        while True:
            try:
                returncode = process.wait(timeout=poll_interval)
            except subprocess.TimeoutExpired:
                returncode = None
            growth = target_size_bytes(target) - starting_size
            free = shutil.disk_usage(cwd).free
            if (
                returncode is None
                and target.is_relative_to(cwd)
                and TEST_RECLAIM_THRESHOLD <= growth <= growth_limit
                and starting_free - free <= host_growth_limit
                and free >= free_floor
            ):
                strip_test_binary_debug(target, min_age_seconds=5)
                growth = target_size_bytes(target) - starting_size
                free = shutil.disk_usage(cwd).free
            if (
                growth > growth_limit
                or starting_size + growth > target_limit
                or starting_free - free > host_growth_limit
                or free < free_floor
            ):
                stop_process_group(process)
                raise RuntimeError(
                    f"test disk budget exceeded: target grew {growth / 1024**3:.1f} GiB, "
                    f"host grew {(starting_free - free) / 1024**3:.1f} GiB, "
                    f"host free {free / 1024**3:.1f} GiB"
                )
            if returncode is not None:
                if returncode not in accepted_returncodes:
                    raise subprocess.CalledProcessError(returncode, argv)
                return
    finally:
        stop_process_group(process)


def run_test262_coverage(python: Path, output: Path, target: Path) -> None:
    adapter = target / "llvm-cov-target/debug/bluejs-test262"
    corpus = REPO_ROOT / "development/browser_core/reference/test262"
    if not adapter.is_file():
        raise ValueError(f"instrumented Test262 adapter is missing: {adapter}")
    environment = os.environ.copy()
    environment["LLVM_PROFILE_FILE"] = str(
        target / "llvm-cov-target/blueice-%p-%m.profraw"
    )
    run_bounded_command(
        [
            str(python),
            str(REPO_ROOT / "backend/bluejs/test262/run.py"),
            "--corpus",
            str(corpus),
            "--adapter",
            str(adapter),
            "--output",
            str(output),
            "--jobs",
            "8",
            "--progress-interval",
            "60",
            "--flush-profiles",
        ],
        cwd=REPO_ROOT,
        target=target,
        env=environment,
        accepted_returncodes=(0, 1),
    )
    summary = json.loads((output / "summary.json").read_text())
    expected = {"pass": 102_921, "excluded": 4, "stale_corpus": 1}
    if (
        not summary.get("complete_inventory")
        or summary.get("scheduled_modes") != 102_926
        or summary.get("results") != expected
    ):
        raise ValueError(
            f"instrumented Test262 inventory changed: {summary.get('results')}"
        )


def run_coverage(*, include_test262: bool = False) -> tuple[dict[Path, dict], dict]:
    with tempfile.TemporaryDirectory(prefix="bluejs-coverage-") as tmp:
        output = Path(tmp) / "coverage.json"
        text_output = Path(tmp) / "coverage.txt"
        try:
            subprocess.run(
                ["cargo", "llvm-cov", "clean", "--profraw-only"],
                cwd=REPO_ROOT,
                check=True,
            )
            run_bounded_command(
                [
                    "cargo",
                    "llvm-cov",
                    "-p",
                    "blueice-bluejs",
                    "--no-clean",
                    "--json",
                    "--output-path",
                    str(output),
                ],
                cwd=REPO_ROOT,
                target=cargo_target(),
            )
            if include_test262:
                run_test262_coverage(sys.executable, Path(tmp) / "test262", cargo_target())
                subprocess.run(
                    [
                        "cargo",
                        "llvm-cov",
                        "report",
                        "-p",
                        "blueice-bluejs",
                        "--json",
                        "--output-path",
                        str(output),
                    ],
                    cwd=REPO_ROOT,
                    check=True,
                )
            subprocess.run(
                [
                    "cargo",
                    "llvm-cov",
                    "report",
                    "-p",
                    "blueice-bluejs",
                    "--text",
                    "--output-path",
                    str(text_output),
                ],
                cwd=REPO_ROOT,
                check=True,
            )
            result = source_union_export(
                json.loads(output.read_text()), text_output.read_text()
            )
            return result
        finally:
            try:
                subprocess.run(
                    ["cargo", "llvm-cov", "clean", "--profraw-only"],
                    cwd=REPO_ROOT,
                    check=True,
                )
            finally:
                # Keep compiled dependencies for the next run; these two
                # incremental directories are disposable and grow per test.
                target = cargo_target()
                if target.is_relative_to(REPO_ROOT):
                    for path in (
                        target / "debug/incremental",
                        target / "llvm-cov-target/debug/incremental",
                    ):
                        if path.exists():
                            shutil.rmtree(path)
                    prune_superseded_test_executables(target)
                    strip_test_binary_debug(target)


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
    parser.add_argument(
        "--update-linux-report",
        action="store_true",
        help="regenerate every row in the Linux report",
    )
    parser.add_argument(
        "--include-test262",
        action="store_true",
        help="combine the full pinned Test262 inventory with the Rust test suite",
    )
    args = parser.parse_args(argv)
    if args.update_macos_report and args.update_linux_report:
        parser.error("update only one platform report")
    if not args.source_file and not (args.update_macos_report or args.update_linux_report):
        parser.error("provide a source file or a platform report option")
    try:
        path = source_path(args.source_file) if args.source_file else None
        files, totals = run_coverage(include_test262=args.include_test262)
        if args.update_macos_report:
            update_macos_report(files, totals, include_test262=args.include_test262)
            print(f"Updated {MACOS_REPORT.relative_to(REPO_ROOT)}")
        if args.update_linux_report:
            update_linux_report(files, totals, include_test262=args.include_test262)
            print(f"Updated {LINUX_REPORT.relative_to(REPO_ROOT)}")
        if path is not None:
            print(markdown_row(path, files.get(path)))
            print(f"Complete: {'yes' if is_complete(files.get(path)) else 'no'}")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"coverage_file.py: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
