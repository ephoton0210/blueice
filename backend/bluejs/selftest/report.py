# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import json
import os
import re
from datetime import datetime
from pathlib import Path

from .common import METRICS, complete, digest, load, now, save, snapshot
from .coverage import export_profiles


def outcome_contract(row):
    # Per-mode timings describe scheduling and must not change the outcome
    # contract when a newer collector adds them to the same pinned fixture.
    result = {key: value for key, value in row.items() if key not in ("actual", "elapsed_seconds")}
    actual = row.get("actual", {})
    result["actual"] = {key: actual.get(key) for key in ("kind", "phase")}
    if row["status"] in ("excluded", "stale_corpus"):
        result["actual"]["reason"] = actual.get("reason")
    return result


def compare_outcomes(before, after):
    def rows(path):
        values = [json.loads(line) for line in Path(path).read_text().splitlines()]
        indexed = {(r["path"], r["mode"]): outcome_contract(r) for r in values}
        if len(values) != len(indexed):
            raise ValueError("Duplicate Test262 outcome key")
        return indexed
    old, new = rows(before), rows(after)
    if old.keys() != new.keys():
        raise ValueError("Test262 inventory changed; a full comparison requires the same pinned inventory")
    changes = [key for key in old if old[key] != new[key]]
    return {"compared_modes": len(old), "contract_changes": len(changes), "changes": changes, "comparison_available": True}


def coverage_summary(payload, root):
    source = Path(root).resolve() / "backend/bluejs/src"
    import importlib.util
    spec = importlib.util.spec_from_file_location("bluejs_coverage_scope", Path(root) / "backend/bluejs/coverage_file.py")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    test_sources = {name for name, reason in helper.NO_COUNTER_REASONS.items() if reason.startswith("Test source;")}
    files = {}
    for item in payload["data"][0]["files"]:
        path = Path(item["filename"]).resolve()
        if path.is_relative_to(source):
            relative = path.relative_to(source).as_posix()
            if relative not in test_sources:
                files[relative] = item["summary"]
    if not files:
        raise ValueError("The full export contains no BlueJS production counters")
    for value in files.values():
        for key in METRICS:
            if not 0 <= value[key]["covered"] <= value[key]["count"]:
                raise ValueError("Invalid raw LLVM counter summary")
    suspect = []
    for function in payload["data"][0].get("functions", []):
        for region in function["regions"]:
            if region[4] >= (1 << 63) and Path(function["filenames"][region[5]]).is_relative_to(source):
                suspect.append(region[:5])
    if suspect:
        raise ValueError("Unsigned counter underflow invalidates the coverage measurement")
    totals = {key: {"count": sum(v[key]["count"] for v in files.values()),
                    "covered": sum(v[key]["covered"] for v in files.values())} for key in METRICS}
    return files, totals


def render_report(data, root, report_path):
    root, report_path = Path(root), Path(report_path)
    summary, raw, totals = data["test262"], data["files"], data["totals"]
    def link(path):
        return os.path.relpath(root / path, report_path.parent)
    def source(name):
        return f"[`{name}`]({link('backend/bluejs/src/' + name)})"
    def metric(value):
        n, c = value["count"], value["covered"]
        return f"{c:,} / {n:,} ({100 * c / n:.6f}%)" if n else "No counters"
    def table(headers, rows):
        out.extend(["| " + " | ".join(headers) + " |", "| " + " | ".join("---" for _ in headers) + " |"])
        out.extend("| " + " | ".join(map(str, row)) + " |" for row in rows)
        out.append("")
    statuses = ("pass", "fail", "unsupported", "excluded", "stale_corpus", "timeout", "harness_error")
    rows = data["outcomes"]
    def counts(prefixes):
        values = {key: 0 for key in statuses}
        for row in rows:
            if any(row["path"] == p or (not p.endswith(".js") and row["path"].startswith(p)) for p in prefixes):
                values[row["status"]] += 1
        return values
    def results(selections, label):
        records = []
        for name, prefixes in selections:
            values = counts(prefixes)
            total = sum(values.values())
            records.append([name, f"{total:,}", *[f"{values[k]:,}" for k in statuses], f"{100 * values['pass'] / total:.3f}%" if total else "No scheduled modes"])
        table([label, "Scheduled", "Pass", "Fail", "Unsupported", "Excluded", "Stale corpus", "Timeout", "Harness error", "Raw pass rate"], records)
    selected, modified = data["selected"], data["modified"]
    complete_count = sum(complete(v) for v in raw.values())
    out = ["# macOS Test262 Report" if os.uname().sysname == "Darwin" else "# BlueJS Test Report", "",
           f"**Measurement: {data['measured_at']}. All numbers use this one verified source snapshot.**", "",
           "## Current complete inventory", "",
           f"Source fingerprint: `{data['snapshot']}`. Adapter SHA-256: `{summary['adapter_sha256']}`. "
           f"Pinned Test262 revision: `{summary['snapshot']['revision']}`.", "",
           f"**{summary['results'].get('pass', 0):,} applicable modes passed**, across {summary['scheduled_modes']:,} scheduled modes and "
           f"{summary['test_files']:,} test files, in {summary['elapsed_seconds']:.3f} seconds. "
           f"The scheduled inventory retains {summary['results'].get('excluded', 0)} host exclusions and "
           f"{summary['results'].get('stale_corpus', 0)} stale corpus modes. These modes are not counted as passes.", "",
           "The runner returns 1 when any scheduled status is not `pass`. The command record retains that exit; semantic outcomes determine the gate.", ""]
    out.extend(["Test262 has no official Core classification. This report defines ECMA-262 Core as `language/` plus `built-ins/`, "
                "the complete ECMA-262 scope as Core plus `annexB/` and `staging/`, and ECMA-402 as `intl402/`. "
                "All pass-rate denominators include the scheduled dispositions in their row.", ""])
    results([("ECMA-262 Core (`language/` + `built-ins/`)", ["language/", "built-ins/"]),
             ("Complete ECMA-262 scope (Core + `annexB/` + `staging/`)", ["language/", "built-ins/", "annexB/", "staging/"]),
             ("ECMA-402 (`intl402/`)", ["intl402/"]), ("Test262 harness support (`harness/`)", ["harness/"]),
             ("**All Test262 runner modes**", [""])], "Scope")
    results([(f"`{name}/`", [name + "/"]) for name in ("language", "built-ins", "annexB", "staging", "intl402", "harness")], "Top-level Test262 group")
    out.extend(["### Non-pass inventory dispositions", ""])
    dispositions = {}
    for row in rows:
        if row["status"] != "pass":
            key = (row["path"], row["status"], row.get("actual", {}).get("reason", ""))
            dispositions[key] = dispositions.get(key, 0) + 1
    table(["Path", "Modes", "Status", "Reason"], [[f"`{key[0]}`", count, key[1], key[2].replace("|", "\\|")] for key, count in sorted(dispositions.items())])
    out.extend(["### Corrected crash regressions", ""])
    results([(f"`{path}`", [path]) for path in ("built-ins/Array/prototype/push/S15.4.4.7_A3.js", "built-ins/Proxy/has/null-handler.js", "built-ins/Proxy/has/null-handler-using-with.js")], "Crash regression")
    out.extend(["## Selected results", ""])
    prefixes = ["built-ins/Array/", "built-ins/ArrayBuffer/", "built-ins/SharedArrayBuffer/", "built-ins/DataView/", "built-ins/Atomics/", "built-ins/Iterator/", "built-ins/Promise/", "built-ins/ShadowRealm/", "built-ins/AsyncDisposableStack/", "built-ins/DisposableStack/", "built-ins/Temporal/", "language/import/", "language/expressions/dynamic-import/", "language/module-code/", "language/statements/using/", "language/statements/await-using/", "language/statements/for-await-of/", "language/eval-code/"]
    selections = [(f"`{p}`", [p]) for p in prefixes]
    selections.insert(1, ("`built-ins/TypedArray/` + `built-ins/TypedArrayConstructors/`", ["built-ins/TypedArray/", "built-ins/TypedArrayConstructors/"]))
    results(selections, "Selection")
    out.extend(["## ECMA-402 and Temporal breakdown", ""])
    intl = ("Temporal", "NumberFormat", "DateTimeFormat", "Locale", "DurationFormat", "ListFormat", "RelativeTimeFormat", "Segmenter", "Intl", "Collator", "DisplayNames", "PluralRules")
    intl_prefixes = [f"intl402/{name}/" for name in intl]
    others = sorted({row["path"] for row in rows if row["path"].startswith("intl402/") and not any(row["path"].startswith(p) for p in intl_prefixes)})
    results([(f"`intl402/{name}/`", [f"intl402/{name}/"]) for name in intl]
            + [("Top-level Intl fixtures and locale methods on other built-ins", others), ("**Total ECMA-402**", ["intl402/"])], "Intl group")
    temporal = sorted({row["path"].split("/")[2] for row in rows if row["path"].startswith(("built-ins/Temporal/", "intl402/Temporal/")) and len(row["path"].split("/")) > 3})
    temporal_roots = sorted({row["path"] for row in rows if row["path"].startswith(("built-ins/Temporal/", "intl402/Temporal/")) and len(row["path"].split("/")) == 3})
    results([(name, [f"built-ins/Temporal/{name}/", f"intl402/Temporal/{name}/"]) for name in temporal]
            + [("Temporal root files", temporal_roots), ("**Total Temporal**", ["built-ins/Temporal/", "intl402/Temporal/"])], "Temporal type")
    out.extend(["## BlueJS per-file coverage", "", "All counters come from fresh atomic LLVM profiles for the current full run. Completion requires 100% raw lines, functions and regions.", ""])
    table(["Metric", "Covered / instrumented"], [[key.capitalize(), metric(totals[key])] for key in METRICS])
    out.extend([f"**{complete_count} / {len(raw)} instrumented files are complete; {len(raw) - complete_count} remain incomplete.**", ""])
    source_paths = sorted(p.relative_to(root / "backend/bluejs/src").as_posix() for p in (root / "backend/bluejs/src").rglob("*.rs"))
    table(["Source file", "Raw lines", "Raw functions", "Raw regions", "Status", "Note"],
          [[source(name), *[metric(raw[name][key]) if name in raw else "—" for key in METRICS],
            "Complete" if name in raw and complete(raw[name]) else ("Incomplete" if name in raw else "No executable counters"),
            "" if name in raw else data.get("no_counters", {}).get(name, "Audited source without executable counters")] for name in source_paths])
    preserved = data["preserved"]
    out.extend(["### Preservation of previously complete files", "",
                f"{preserved['passed']} / {preserved['total']} original complete files remain complete. "
                f"The raw comparison records {len(data['coverage_regressions'])} per-file coverage percentage regressions.", "",
                "## Nine most difficult and thirteen difficult files", ""])
    table(["Difficulty", "Selected source", "Raw lines", "Raw functions", "Raw regions", "Status"],
          [[f"D{level}", source(name), *[metric(raw[name][key]) if name in raw else "Missing counters" for key in METRICS],
            "Complete" if name in raw and complete(raw[name]) else "Incomplete"] for name, level in sorted(selected.items(), key=lambda p: (-p[1], p[0]))])
    out.extend([f"Selected files complete: **{sum(name in raw and complete(raw[name]) for name in selected)} / {len(selected)}**. "
                f"Modified production files complete: **{sum(name in raw and complete(raw[name]) for name in modified)} / {len(modified)}**.", "",
                "### Verification evidence", ""])
    rust = data["rust"]
    table(["Gate", "Current result"], [["BlueJS Rust", f"{rust['targets']} targets; {rust['counts'][0]:,} passed; {rust['counts'][1]} failed; {rust['counts'][2]} ignored"],
                                       ["Full Test262", f"{summary['results'].get('pass', 0):,} passed"],
                                       ["Semantic comparison", f"{data['contracts']['compared_modes']:,} mode contracts; {data['contracts']['contract_changes']} changes" if data['contracts'].get('comparison_available') else "No prior complete inventory available"],
                                       ["Workspace runtime and Rustdoc", "Passed" if data["workspace"] else "Not verified for this snapshot"]])
    if data.get("gates"):
        table(["Current gate", "Pass", "Fail", "Ignored", "Seconds"],
              [[gate["label"], *[f"{c:,}" for c in gate["counts"]], f"{gate['elapsed_seconds']:.3f}"] for gate in data["gates"]])
    static = data.get("static_gates", {})
    static_current = static.get("snapshot") == data["snapshot"]
    table(["Static gate", "Current result"],
          [[name, "Passed" if static_current and static.get(key, {}).get("status") == "passed" else "Not verified for this snapshot"]
           for name, key in (("Task Rust formatting", "rustfmt"), ("Diff whitespace", "diff_check"), ("Workspace Clippy, warnings denied", "workspace_clippy"))])
    incomplete_modified = sorted(name for name in modified if name not in raw or not complete(raw[name]))
    out.extend(["### Remaining modified production files", ""])
    table(["Source file", "Missing lines", "Missing functions", "Missing regions"],
          [[source(name), *[raw[name][key]["count"] - raw[name][key]["covered"] if name in raw else "Missing counters" for key in METRICS]] for name in incomplete_modified])
    out.extend(["### Coverage percentage regressions", ""])
    if data["coverage_regressions"]:
        table(["Source file", "Current raw lines", "Current raw functions", "Current raw regions"],
              [[source(name), *[metric(raw[name][key]) for key in METRICS]] for name in data["coverage_regressions"]])
    else:
        out.extend(["Zero per-file percentage regressions against the retained verified comparison thresholds.", ""])
    achieved = (not incomplete_modified and all(name in raw and complete(raw[name]) for name in selected)
                and preserved["passed"] == preserved["total"] and not data["coverage_regressions"] and data["workspace"]
                and rust["counts"][1] == 0 and data["contracts"].get("comparison_available")
                and data["contracts"]["contract_changes"] == 0 and static_current
                and all(static.get(key, {}).get("status") == "passed" for key in ("rustfmt", "diff_check", "workspace_clippy")))
    out.extend(["**The requested completion gates are satisfied.**" if achieved else "**The requested completion gates are not yet satisfied.**", ""])
    if data.get("evidence"):
        out.extend([f"[Frozen source hashes]({link(data['evidence'] + '/source-state.json')}), "
                    f"[complete run record]({link(data['evidence'] + '/run.json')}), "
                    f"[raw coverage data]({link(data['evidence'] + '/report-data.json')}) and "
                    f"[semantic contract audit]({link(data['evidence'] + '/outcome-contract-audit.json')}) retain this measurement.", ""])
    out.extend(["## Difficulty ranking of the remaining incomplete files", ""])
    ranking = data.get("ranking", {})
    missing = sorted((name for name in raw if not complete(raw[name])), key=lambda n: (-ranking.get(n, -1), -(raw[n]["regions"]["count"] - raw[n]["regions"]["covered"]), n))
    table(["Rank", "Difficulty", "Source file", "Missing lines", "Missing functions", "Missing regions"],
          [[i, f"D{ranking[name]}" if name in ranking else "Unranked", source(name), *[raw[name][key]["count"] - raw[name][key]["covered"] for key in METRICS]] for i, name in enumerate(missing, 1)])
    out.extend(["## Reproduce the current inventory", "", "```sh", "python3 -m backend.bluejs.selftest run --mode pipeline --workspace", "```", "",
                "The pipeline runs affected tests first, then the full current inventory and fresh coverage. "
                "A partial or stale run cannot publish this report.", ""])
    return "\n".join(out)


def full_report(manager, artifacts, bins):
    directory = manager.state / "runs" / manager.active["id"]
    full = directory / "full"
    # The current inventory, not the previous graph, determines completeness.
    from .graph import inventory
    expected = {key for key, node in inventory(manager.root).items() if node["kind"] == "rust"}
    tasks = [t for t in manager.active["tasks"] if t["stage"] == "full" and t["id"].startswith("full:rust:")]
    if {t["id"][5:] for t in tasks if t["status"] == "passed"} != expected:
        raise ValueError("A partial Rust run cannot generate the full report")
    if snapshot(manager.root)["identity"] != manager.active["snapshot"]:
        raise ValueError("Source changes invalidate the full measurement")
    for task in tasks:
        if digest(task["binary"]) != task["binary_sha256"]:
            raise ValueError("A measured executable changed before coverage export")
    profiles = list((full / "profiles").rglob("*.profraw"))
    objects = list(artifacts.values()) + list(bins.values())
    payload = export_profiles(objects[0], profiles, full / "coverage", manager.root, objects, summary_only=False)
    raw, totals = coverage_summary(payload, manager.root)
    graph = manager.graph()
    baseline = graph.get("baseline")
    contracts = {"compared_modes": 0, "contract_changes": 0, "comparison_available": False}
    if baseline:
        contracts = compare_outcomes(Path(baseline["path"]) / "test262/results.jsonl", full / "test262/results.jsonl")
        if contracts["contract_changes"]:
            raise ValueError("Test262 semantic outcome contracts changed")
    previous = graph.get("coverage_reference") or (baseline.get("full_coverage", {}) if baseline else {})
    regressions = []
    for name, current in raw.items():
        if name not in previous:
            continue
        old = previous[name].get("_raw", previous[name])
        if any(current[key]["covered"] * old[key]["count"] < old[key]["covered"] * current[key]["count"] for key in METRICS):
            regressions.append(name)
    scope = graph.get("acceptance_scope", {})
    original = graph.get("originally_complete", [])
    canonical = manager.root / "development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md"
    ranking = {}
    if canonical.is_file():
        ranking = {m.group(2): int(m.group(1)) for m in re.finditer(r"^\| \d+ \| D(\d) \| \[`([^`]+)`\]", canonical.read_text(), re.M)}
    ranking.update(scope.get("selected_files", {}))
    source_root = manager.root / "backend/bluejs/src"
    unknown = [p.relative_to(source_root).as_posix() for p in source_root.rglob("*.rs") if p.relative_to(source_root).as_posix() not in raw]
    # Preserve the existing audited set of files without executable counters.
    import importlib.util
    spec = importlib.util.spec_from_file_location("bluejs_coverage", manager.root / "backend/bluejs/coverage_file.py")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    if set(unknown) != set(helper.NO_COUNTER_REASONS):
        raise ValueError("Executable source scope differs from the reviewed no-counter manifest")
    from .runner import CRITICAL_RUST_TARGETS
    critical = [t for t in tasks if t["id"][5:].startswith(CRITICAL_RUST_TARGETS)]
    gates = []
    for label, records in (("Complete BlueJS Rust", tasks), ("Critical Rust targets", critical)):
        first = min(datetime.fromisoformat(t["started_at"]) for t in records)
        last = max(datetime.fromisoformat(t["finished_at"]) for t in records)
        gates.append({"label": label, "counts": [sum(c[i] for t in records for c in t["counts"]) for i in range(3)],
                      "elapsed_seconds": (last - first).total_seconds()})
    for task in manager.active["tasks"]:
        if task["id"] not in ("workspace:tests", "workspace:docs", "full:python:selftest"):
            continue
        log = (directory / task["log"]).read_text()
        counts = [list(map(int, row)) for row in re.findall(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored", log)]
        if task["id"] == "full:python:selftest":
            match = re.search(r"Ran (\d+) tests", log)
            counts = [[int(match[1]), 0, 0]] if match and task["status"] == "passed" else []
        gates.append({"label": task["label"], "counts": [sum(c[i] for c in counts) for i in range(3)], "elapsed_seconds": task["elapsed_seconds"]})
    data = {"measured_at": now(), "snapshot": manager.active["snapshot"], "files": raw, "totals": totals,
            "static_gates": load(directory / "static-gates.json", {}),
            "evidence": str(directory.relative_to(manager.root)), "gates": gates,
            "test262": load(full / "test262/summary.json"), "selected": scope.get("selected_files", {}),
            "modified": scope.get("modified_production_files", []), "contracts": contracts, "no_counters": helper.NO_COUNTER_REASONS,
            "coverage_regressions": regressions, "ranking": ranking,
            "coverage_reference": previous,
            "preserved": {"total": len(original), "passed": sum(name in raw and complete(raw[name]) for name in original)},
            "workspace": manager.workspace, "rust": {"targets": len(tasks),
            "counts": [sum(c[i] for t in tasks for c in t["counts"]) for i in range(3)]},
            "outcomes": [json.loads(line) for line in (full / "test262/results.jsonl").read_text().splitlines()]}
    save(directory / "report-data.json", {k: v for k, v in data.items() if k != "outcomes"})
    save(directory / "outcome-contract-audit.json", contracts)
    report = directory / "report.md"
    report.write_text(render_report(data, manager.root, report))
    return report


def publish_report(manager, run_id):
    run = load(manager.state / "runs" / run_id / "run.json")
    if not run or run["status"] != "passed" or run["mode"] not in ("full", "pipeline") or not run.get("report"):
        raise ValueError("Only a successful complete run can publish the report")
    if snapshot(manager.root)["identity"] != run["snapshot"]:
        raise ValueError("Sources changed after the full run; report publication is stale")
    directory = manager.state / "runs" / run_id
    data = load(directory / "report-data.json")
    data["outcomes"] = [json.loads(line) for line in (directory / "full/test262/results.jsonl").read_text().splitlines()]
    target = manager.root / "development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md"
    target.write_text(render_report(data, manager.root, target))
    save(target.with_name("test262-summary.json"), data["test262"])
    return target
