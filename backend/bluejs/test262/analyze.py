#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Reconcile a Test262 run and classify targets separately from observed blockers.

Classification is triage, not a claim to infer all root causes from one error.
Every test/mode is retained, including passed negatives and unsupported hosts.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import tempfile

import yaml

from run import ROOT, classify, metadata, modes

# Ordered dependency backlog. These are implementation workstreams, not skips.
WORKSTREAMS = {
    "completion": ("P0.1", "Completion records, iterator lifetime, catch/finally, control transfer", []),
    "environments": ("P0.2", "Persistent realms, bindings, parameter environments, arguments, eval", ["completion"]),
    "references": ("P0.3", "Reference evaluation, coercion and observable evaluation order", ["completion", "environments"]),
    "objects": ("P0.4", "Internal methods, descriptors, receiver and callable/constructor contracts", ["references"]),
    "grammar": ("P1.1", "Source grammar and classified strict/early errors", ["environments", "references"]),
    "classes": ("P1.2", "Classes, private slots, super and derived construction", ["objects", "grammar"]),
    "suspension": ("P1.3", "Resumable frames, generators, async functions and promise jobs", ["completion", "environments", "objects", "grammar"]),
    "modules": ("P1.4", "Module linking, live bindings, evaluation and dynamic import", ["environments", "suspension", "grammar"]),
    "storage": ("P1.5", "BigInt, buffers, typed arrays, shared memory and GC weak slots", ["objects"]),
    "host": ("P1.6", "Test262 realm, agent, GC, buffer and async host hooks", ["environments", "suspension", "modules", "storage"]),
    "library": ("P2.1", "Remaining standard builtin algorithms and descriptors", ["completion", "references", "objects"]),
    "intl": ("P2.2", "ECMA-402 constructors, algorithms and locale data", ["library"]),
    "review": ("P3.1", "Unmapped/staging targets requiring specification and applicability review", []),
}
STATUSES = {"pass", "fail", "unsupported", "timeout", "harness_error"}


def target_for(path, features):
    parts = path.split("/")
    if parts[0] == "annexB":
        parts = parts[1:]
    if parts[:2] == ["staging", "intl402"] or parts[0] == "intl402":
        return "intl"
    if parts[0] == "harness":
        return "host"
    if any(x in parts for x in ("class", "class-elements", "super")):
        return "classes"
    if any(x in parts for x in ("import", "export", "module-code", "import.meta", "dynamic-import")):
        return "modules"
    if any(x in parts for x in ("generators", "generator", "yield", "async-function", "async-generator", "async-generators", "async-arrow-function", "await", "Promise", "AsyncGenerator", "GeneratorFunction", "AsyncFunction", "AsyncGeneratorFunction")):
        return "suspension"
    if any(x in parts for x in ("BigInt", "ArrayBuffer", "SharedArrayBuffer", "TypedArray", "TypedArrayConstructors", "%TypedArray%", "DataView", "Atomics", "WeakRef", "FinalizationRegistry")) or any(x.endswith("Array") and x != "Array" for x in parts):
        return "storage"
    if any(x in parts for x in ("try", "throw", "return", "break", "continue", "for-of", "for-await-of", "switch", "with", "completion", "destructuring", "dstr")):
        return "completion"
    if any(x in parts for x in ("arguments-object", "eval-code", "global-code", "block-scope", "scope", "let", "const", "variable", "function", "arrow-function", "default-parameters", "Function", "eval")):
        return "environments"
    if any(x in parts for x in ("Object", "Reflect", "Proxy")):
        return "objects"
    if parts[0] == "built-ins":
        return "library"
    if parts[:2] == ["language", "expressions"]:
        return "references"
    if parts[0] == "language":
        return "grammar"
    return "review"


def observed_blocker(record):
    if record["status"] == "pass":
        return "none"
    actual = record.get("actual", {})
    kind = actual.get("kind", "unknown")
    reason = actual.get("reason", "")
    message = actual.get("message", "")
    if record["status"] == "harness_error":
        return "harness-infrastructure"
    if kind == "timeout":
        return "instruction-limit" if "instruction budget" in message else "deadline"
    if kind == "resource_error":
        return "resource-limit"
    if reason == "module host":
        return "module-host"
    if "async jobs" in reason:
        return "async-host"
    if "host hook" in reason:
        return "host-hook"
    if "harness" in reason:
        return "harness-prerequisite"
    if kind == "unclassified_parse_error":
        return "unclassified-parse"
    if kind == "unsupported":
        return "compiler-unsupported"
    if kind == "ReferenceError":
        return "unresolved-name"
    if kind == "Test262Error":
        return "assertion"
    if kind == "ok" and record.get("expected"):
        return "missing-expected-error"
    return "exception:" + kind


def classify_item(record, data):
    path = record["path"]
    features = set(data.get("features", record.get("features", [])))
    flags = set(data.get("flags", record.get("flags", [])))
    target = target_for(path, features)
    dependencies = set(WORKSTREAMS[target][2])
    if any("iterator" in f.lower() or f in {"destructuring-binding", "destructuring-assignment"} for f in features):
        dependencies.add("completion")
    if "module" in flags or any("import" in f for f in features):
        dependencies.add("modules")
    if "async" in flags or any(f in {"generators", "async-functions", "async-iteration", "Promise"} for f in features):
        dependencies.add("suspension")
    if any(f.startswith("class") or f == "super" for f in features):
        dependencies.add("classes")
    if features & {"Proxy", "Symbol", "Reflect"}:
        dependencies.add("objects")
    if features & {"BigInt", "ArrayBuffer", "SharedArrayBuffer", "Atomics", "WeakRef", "FinalizationRegistry"}:
        dependencies.add("storage")
    if features & {"IsHTMLDDA", "cross-realm"} or flags & {"CanBlockIsTrue", "CanBlockIsFalse"}:
        dependencies.add("host")
    blocker = observed_blocker(record)
    for observed, dependency in {"module-host": "modules", "async-host": "suspension", "host-hook": "host", "harness-prerequisite": "host", "harness-infrastructure": "host"}.items():
        if blocker == observed:
            dependencies.add(dependency)
    dependencies.discard(target)
    scope = path.split("/")[0]
    return {**record, "scope": scope, "target": target, "priority": WORKSTREAMS[target][0],
            "dependencies": sorted(dependencies, key=lambda name: WORKSTREAMS[name][0]),
            "blocker": blocker, "confidence": "observed-symptom" if blocker != "none" else "runner-pass",
            "esid": data.get("esid"), "description": data.get("description", ""),
            "includes": data.get("includes", [])}


def signature(record):
    actual = record.get("actual", {})
    message = actual.get("reason") or actual.get("message") or actual.get("kind", "metadata error")
    # Group offset/token-number variations; keep raw details in items.jsonl.
    return re.sub(r"\b\d+\b", "#", message)[:200]


def markdown(report):
    rows = ["# Test262 architecture triage", "", f"Snapshot: `{report['snapshot']['revision']}`.", "",
            f"Reconciled {report['test_files']:,} files / {report['scheduled_modes']:,} modes; complete inventory: {report['complete_inventory']}.", "",
            "Targets are inferred from paths/metadata; blockers are first observed symptoms, not proven root causes. "
            "Feature/dependency counts overlap. Target counts are exclusive and reconcile to all modes. "
            "Priorities are dependency order, not failure-count order. Passed negatives remain passes; no outcomes are excluded.", "",
            "| Order | Target | Pass | Fail | Unsupported | Timeout | Harness error | Prerequisites |",
            "| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |"]
    for name, (priority, title, dependencies) in WORKSTREAMS.items():
        counts = report["targets"].get(name, {})
        numbers = " | ".join(str(counts.get(status, 0)) for status in ("pass", "fail", "unsupported", "timeout", "harness_error"))
        rows.append(f"| {priority} | {name}: {title} | {numbers} | {', '.join(dependencies) or '—'} |")
    rows += ["", "## Observed blockers", "", "| Symptom | Modes | Representative test / mode |", "| --- | ---: | --- |"]
    for blocker, count in sorted(report["blockers"].items(), key=lambda item: (-item[1], item[0])):
        examples = report["examples"].get(blocker, [])
        sample = examples[0] if examples else "—"
        rows.append(f"| {blocker} | {count} | `{sample}` |")
    rows += ["", "## Frequent diagnostics", "", "Exact per-mode evidence, source hashes, esid, includes and dependency labels are in `items.jsonl`.", "",
             "| Diagnostic (numeric details normalized) | Modes |", "| --- | ---: |"]
    for diagnostic, count in sorted(report["diagnostics"].items(), key=lambda item: (-item[1], item[0]))[:30]:
        rows.append(f"| {diagnostic.replace('|', '&#124;').replace(chr(10), ' ')} | {count} |")
    rows += ["", "## Measurement limits", "", "A passing result is the runner's observation, not proof of complete feature conformance. "
             "Unclassified parser rejections cannot establish required negative early errors. Existing adapter records may not distinguish "
             "a runtime exception in an include from one in the test body. Missing APIs, assertion mismatches and timeouts require reproduction "
             "before assigning a confirmed engine root cause. Staging, Annex B, Intl and host-dependent cases remain visible pending applicability audit.", ""]
    return "\n".join(rows)


def analyze(results_path, summary_path, corpus, output):
    summary = json.loads(summary_path.read_text())
    if json.loads((corpus / ".bluejs-snapshot.json").read_text()) != summary["snapshot"]:
        raise ValueError("run and corpus snapshot markers disagree")
    if summary["complete_inventory"] != (not summary["filter"]):
        raise ValueError("inconsistent full/partial inventory label")
    records = {}
    counts, groups, features = Counter(), defaultdict(Counter), defaultdict(Counter)
    with results_path.open() as source:
        for line in source:
            record = json.loads(line)
            key = (record["path"], record["mode"])
            if key in records or record["status"] not in STATUSES:
                raise ValueError(f"duplicate or invalid outcome: {key}")
            if "actual" in record and classify(record["actual"], record.get("expected")) != record["status"]:
                raise ValueError(f"outcome disagrees with runner classification: {key}")
            records[key] = record
            counts[record["status"]] += 1
            groups[record["path"].split("/")[0]][record["status"]] += 1
            for feature in record.get("features", []):
                features[feature][record["status"]] += 1
    if len(records) != summary["scheduled_modes"] or counts != Counter(summary["results"]) or dict(groups) != summary["groups"] or dict(features) != summary["features"]:
        raise ValueError("results do not reconcile with summary")
    selected = sorted(path for path in (corpus / "test").rglob("*.js") if "_FIXTURE" not in path.name and summary["filter"] in path.relative_to(corpus / "test").as_posix())
    if len(selected) != summary["test_files"]:
        raise ValueError("source inventory does not reconcile with summary")
    targets, dependencies, scopes = defaultdict(Counter), defaultdict(Counter), defaultdict(Counter)
    blockers, diagnostics, examples = Counter(), Counter(), defaultdict(list)
    seen = set()
    # Publish reports only after the entire input has been reconciled.
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="test262-analysis-", dir=output.parent) as temporary:
        pending = Path(temporary)
        with (pending / "items.jsonl").open("w") as items:
            for path in selected:
                relative = path.relative_to(corpus / "test").as_posix()
                contents = path.read_bytes()
                digest = hashlib.sha256(contents).hexdigest()
                try:
                    data = metadata(contents.decode("utf-8"))
                    expected_modes = modes(data)
                except (ValueError, UnicodeError, yaml.YAMLError):
                    data, expected_modes = {}, ["metadata"]
                for mode in expected_modes:
                    key = (relative, mode)
                    record = records.get(key)
                    if record is None or record["sha256"] != digest:
                        raise ValueError(f"missing mode or changed source: {key}")
                    if mode != "metadata" and any(record.get(field, []) != data.get(field, []) for field in ("features", "flags")):
                        raise ValueError(f"metadata mismatch: {key}")
                    if record.get("expected") != data.get("negative"):
                        raise ValueError(f"negative metadata mismatch: {key}")
                    seen.add(key)
                    item = classify_item(record, data)
                    items.write(json.dumps(item, ensure_ascii=True, sort_keys=True) + "\n")
                    targets[item["target"]][item["status"]] += 1
                    scopes[item["scope"]][item["status"]] += 1
                    for dependency in item["dependencies"]:
                        dependencies[dependency][item["status"]] += 1
                    blockers[item["blocker"]] += 1
                    if item["status"] != "pass":
                        diagnostics[signature(record)] += 1
                        if len(examples[item["blocker"]]) < 5:
                            examples[item["blocker"]].append(f"{relative} [{mode}]")
        if seen != records.keys():
            raise ValueError("unexpected path/mode in results")
        report = {key: value for key, value in summary.items() if key not in {"features", "groups"}}
        report.update(schema_version=1, classifier_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                      results_sha256=hashlib.sha256(results_path.read_bytes()).hexdigest(),
                      targets=dict(targets), dependencies=dict(dependencies), scopes=dict(scopes),
                      blockers=dict(blockers), diagnostics=dict(diagnostics), examples=dict(examples),
                      workstreams={name: {"priority": priority, "description": title, "depends_on": deps} for name, (priority, title, deps) in WORKSTREAMS.items()})
        (pending / "analysis.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
        (pending / "REPORT.md").write_text(markdown(report))
        output.mkdir(parents=True, exist_ok=True)
        for path in pending.iterdir():
            path.replace(output / path.name)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True, help="directory containing results.jsonl and summary.json")
    parser.add_argument("--corpus", type=Path, default=ROOT / "development/browser_core/reference/test262")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = analyze(args.run / "results.jsonl", args.run / "summary.json", args.corpus, args.output)
    except (ValueError, OSError, KeyError) as error:
        parser.error(str(error))
    print(json.dumps({key: report[key] for key in ("test_files", "scheduled_modes", "results", "blockers")}, indent=2))


if __name__ == "__main__":
    main()
