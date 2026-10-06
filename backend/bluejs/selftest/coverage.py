# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import concurrent.futures
import hashlib
import json
import re
import shutil
import os
import uuid
from datetime import datetime, timezone
from pathlib import Path

from .common import ROOT, checked, complete, digest, load, now, retain_coverage_reference, save
from .graph import empty_graph


def llvm_tool(name):
    candidates = [os.environ.get(name.upper().replace("-", "_")),
                  f"/opt/homebrew/opt/llvm@22/bin/{name}", shutil.which(name)]
    path = next((Path(p) for p in candidates if p and Path(p).is_file()), None)
    if path is None:
        raise RuntimeError(f"Install the LLVM tools matching rustc, or add {name} to PATH")
    return str(path)


def artifact_id(artifact):
    target = artifact["target"]
    kind = target["kind"][0]
    return "rust:lib" if kind == "lib" else f"rust:{kind}:{target['name']}"


def profile_owners(text, names):
    """Read length-prefixed crate names from Rust v0 instrumentation symbols."""
    owners = set()
    for match in re.finditer(r"Cs[A-Za-z0-9]+_(\d+)(\w+)", text):
        length = int(match.group(1))
        name = match.group(2)[:length]
        if name in names:
            owners.add(names[name])
    return owners


def covered_sources(payload, root=ROOT):
    root = Path(root).resolve()
    data = payload.get("data", [])
    if len(data) != 1:
        raise ValueError("Expected exactly one LLVM coverage data set")
    result = []
    for item in data[0]["files"]:
        path = Path(item["filename"]).resolve()
        if not path.is_relative_to(root / "backend/bluejs"):
            continue
        summary = item["summary"]
        for metric in ("lines", "functions", "regions"):
            value = summary[metric]
            if not 0 <= value["covered"] <= value["count"]:
                raise ValueError("Invalid LLVM coverage counters")
        if any(summary[key]["covered"] > 0 for key in ("lines", "functions", "regions")):
            result.append(path.relative_to(root).as_posix())
    return sorted(set(result))


def export_profiles(binary, profiles, destination, root=ROOT, objects=(), summary_only=True):
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    files = sorted(map(str, profiles))
    if not files:
        raise ValueError("A measured execution must produce fresh profiles")
    profile_list = destination / "profiles.txt"
    profile_list.write_text("\n".join(files) + "\n")
    merged = destination / "measured.profdata"
    checked([llvm_tool("llvm-profdata"), "merge", "-sparse", "-f", str(profile_list), "-o", str(merged)], root)
    command = [llvm_tool("llvm-cov"), "export", str(binary), "--instr-profile=" + str(merged)]
    command.extend("--object=" + str(p) for p in objects if str(p) != str(binary))
    if summary_only:
        command.append("--summary-only")
    scope = Path(root) / ("backend/bluejs" if summary_only else "backend/bluejs/src")
    command.extend(["--sources", *[str(p) for p in sorted(scope.rglob("*.rs"))]])
    payload = json.loads(checked(command, root))
    save(destination / "coverage.json", payload)
    return payload


def import_baseline(baseline, state, root=ROOT, progress=None):
    """Build observed target relationships from an already completed measurement."""
    root, baseline, state = Path(root), Path(baseline).resolve(), Path(state)
    if (baseline / "run.json").is_file():
        return import_verified_run(baseline, state, root, progress)
    rust = load(baseline / "rust-results.json")
    summary = load(baseline / "test262/summary.json")
    source_state = load(baseline / "source-state.json")
    audit = load(baseline / "coverage-audit.json")
    if not all((rust, summary, source_state, audit)):
        raise ValueError("Baseline needs full Rust results, Test262 results, source hashes and a coverage audit")
    if rust.get("failed_targets") or any(r["exit_code"] for r in rust["results"]):
        raise ValueError("A failed Rust epoch cannot seed the measured graph")
    if not summary.get("complete_inventory") or any(summary["results"].get(k, 0) for k in ("fail", "unsupported", "timeout", "harness_error")):
        raise ValueError("Baseline must contain a passing complete Test262 inventory")
    artifacts = {}
    for line in (baseline / "build.jsonl").read_text().splitlines():
        row = json.loads(line)
        if row.get("reason") == "compiler-artifact" and row.get("executable") and row["profile"]["test"]:
            artifacts[artifact_id(row)] = row
    if len(artifacts) != len(rust["results"]):
        raise ValueError("Cargo artifact inventory and completed Rust targets disagree")
    results = {r["executable"]: r for r in rust["results"]}
    graph = empty_graph(root)
    graph["source_hashes"] = source_state
    measured_at = datetime.fromtimestamp((baseline / "test262/summary.json").stat().st_mtime, timezone.utc).isoformat()
    graph["baseline"] = {"path": str(baseline), "measured_at": measured_at, "test262": summary,
                         "rust": {"targets": len(rust["results"]), "elapsed_seconds": rust["elapsed_seconds"],
                                  "counts": [sum(c[i] for r in rust["results"] for c in r["counts"]) for i in range(3)]},
                         "coverage": {name: value for name, value in audit.get("modified", {}).items()},
                         "audit": {k: v for k, v in audit.items() if k not in ("changed_counters", "modified", "selected")}}
    # Preserve exact baseline structures, rather than deriving them from changed files.
    frozen = load(baseline / "frozen-rust-sources.json", {})
    from .graph import rust_structure
    for name, text in frozen.items():
        if name.startswith("backend/bluejs/"):
            graph["structures"][name] = rust_structure(text)
    names = {a["target"]["name"].replace("-", "_"): key for key, a in artifacts.items()}
    profiles = sorted((baseline / "profiles").glob("*.profraw"))
    groups = {}
    if progress:
        progress("Identifying retained profiles", 0, len(profiles))

    def identify(path):
        text = checked([llvm_tool("llvm-profdata"), "show", "--all-functions", "--counts", str(path)], root)
        owners = profile_owners(text, names)
        specific = owners - {"rust:lib"}
        # Executed standalone adapter/worker main functions belong to the corpus;
        # zero-count binary harness profiles remain their own Cargo targets.
        binaries = {key for key in specific if key.startswith("rust:bin:")}
        main_executed = any(int(count) > 0 for count in re.findall(
            r"(?:bluejs_test262|bluejs_regexp_worker)[^\n]*4main[^\n]*:\n(?:[^\n]*\n){0,4}?\s+Function count: (\d+)", text))
        if binaries and main_executed:
            return path, "test262:all"
        if len(specific) == 1:
            return path, next(iter(specific))
        if not specific and "rust:lib" in owners:
            return path, "rust:lib"
        return path, None

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        for index, (path, key) in enumerate(pool.map(identify, profiles), 1):
            if key:
                groups.setdefault(key, []).append(path)
            if progress and (index % 20 == 0 or index == len(profiles)):
                progress("Identifying retained profiles", index, len(profiles))
    bins = load(baseline / "binaries.json")
    cache = state / "index-cache"
    warnings = []
    if "test262:all" not in groups:
        warnings.append("Retained adapter profiles could not be identified; Test262 stays conservative until the next full run.")

    def measure(item):
        key, paths = item
        if key == "test262:all":
            binary = bins["bluejs-test262"]
            expected = summary["adapter_sha256"]
            objects = [bins["bluejs-regexp-worker"]]
        else:
            binary = artifacts[key]["executable"]
            expected = results[binary]["binary_sha256"]
            objects = []
        if not Path(binary).is_file() or digest(binary) != expected:
            return key, None, "Retained binary changed or is missing: " + key
        signature = hashlib.sha256((expected + "".join(digest(p) for p in paths)).encode()).hexdigest()
        destination = cache / signature
        payload = load(destination / "coverage.json")
        if payload is None:
            payload = export_profiles(binary, paths, destination, root, objects)
        return key, covered_sources(payload, root), None

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        for index, (key, sources, warning) in enumerate(pool.map(measure, sorted(groups.items())), 1):
            if warning:
                warnings.append(warning)
                continue
            if key not in graph["tests"]:
                continue
            graph["observed_targets"].append(key)
            graph["edges"].extend({"source": name, "target": key, "kind": "observed",
                                    "evidence": str(baseline), "source_sha256": source_state.get(name)} for name in sources)
            if key in artifacts:
                result = results[artifacts[key]["executable"]]
                graph["tests"][key]["elapsed_seconds"] = result["elapsed_seconds"]
                graph["tests"][key]["counts"] = result["counts"]
            else:
                graph["tests"][key]["elapsed_seconds"] = summary["elapsed_seconds"]
            if progress:
                progress("Reading measured target coverage", index, len(groups))
    graph["observed_targets"] = sorted(set(graph["observed_targets"]))
    graph["warnings"] = warnings
    graph["created_at"] = now()
    graph["baseline"]["full_coverage"] = load(baseline / "source-union.json", {}).get("files", {})
    imported_scope = load(baseline / "acceptance-scope.json", {})
    existing_scope = load(state / "graph.json", {}).get("acceptance_scope", {})
    retained_scope = graph.get("acceptance_scope", {})
    graph["acceptance_scope"] = {**retained_scope, **imported_scope}
    graph["acceptance_scope"]["modified_production_files"] = sorted(set(imported_scope.get("modified_production_files", []))
        | set(retained_scope.get("modified_production_files", [])) | set(existing_scope.get("modified_production_files", [])))
    before = {row["path"]: row["before"] for row in audit.get("changed_counters", [])}
    graph["originally_complete"] = sorted(name for name, value in graph["baseline"]["full_coverage"].items()
                                          if complete(before.get(name, value.get("_raw", value))))
    if len(graph["originally_complete"]) != audit.get("formerly_complete_files", len(graph["originally_complete"])):
        raise ValueError("Originally complete file identities do not reconcile with the retained audit")
    existing = load(state / "graph.json", {})
    graph["coverage_reference"] = existing.get("coverage_reference", {})
    graph["coverage_reference_provenance"] = existing.get("coverage_reference_provenance", [])
    retain_coverage_reference(graph, graph["baseline"]["full_coverage"], baseline)
    save(state / "graph.json", graph)
    return graph


def import_verified_run(directory, state, root=ROOT, progress=None):
    """Rebuild the graph from the per-target profiles of a completed UI run."""
    directory, state, root = Path(directory), Path(state), Path(root)
    run, data = load(directory / "run.json", {}), load(directory / "report-data.json", {})
    source_state = load(directory / "source-state.json", {})
    from .runner import test262_passed
    graph = empty_graph(root)
    expected = {key for key, node in graph["tests"].items() if node["kind"] == "rust"}
    tasks = {task["id"][5:]: task for task in run.get("tasks", []) if task["id"].startswith("full:rust:")}
    corpus_task = next((t for t in run.get("tasks", []) if t["id"] == "full:test262:all"), None)
    if (run.get("status") != "passed" or run.get("mode") not in ("full", "pipeline") or not data
            or data.get("snapshot") != run.get("snapshot") or source_state.get("identity") != run.get("snapshot")
            or set(tasks) != expected or any(t["status"] != "passed" for t in tasks.values())
            or not corpus_task or corpus_task["status"] != "passed" or not test262_passed(data["test262"], True)):
        raise ValueError("Only a verified complete run can seed the measured graph")
    existing = load(state / "graph.json", {})
    graph["source_hashes"] = source_state["files"]
    from .graph import rust_structure
    graph["structures"] = load(directory / "source-structures.json", {})
    for name, text in load(directory / "frozen-rust-sources.json", {}).items():
        graph["structures"][name] = rust_structure(text)
    bins = load(directory / "full/bins.json", {})
    tasks["test262:all"] = corpus_task
    warnings = []
    for index, (key, task) in enumerate(sorted(tasks.items()), 1):
        binary = bins.get("bluejs-test262") if key == "test262:all" else task.get("binary")
        expected_hash = data["test262"]["adapter_sha256"] if key == "test262:all" else task.get("binary_sha256")
        if not binary or not Path(binary).is_file() or digest(binary) != expected_hash:
            warnings.append("Retained binary changed or is missing: " + key)
            continue
        profiles_path = Path(task["profile_directory"])
        if not profiles_path.resolve().is_relative_to((directory / "full/profiles").resolve()):
            raise ValueError("Target profiles must belong to the verified run")
        profiles = sorted(profiles_path.glob("*.profraw"))
        if not profiles:
            warnings.append("Retained profiles are missing: " + key)
            continue
        destination = directory / "full/coverage" / uuid.uuid5(uuid.NAMESPACE_URL, key).hex
        signature = {"binary": expected_hash, "profiles": {str(p): digest(p) for p in profiles}}
        cached = load(destination / "evidence.json", {})
        payload = load(destination / "coverage.json")
        if cached.get("inputs") != signature or not payload or cached.get("coverage_sha256") != digest(destination / "coverage.json"):
            objects = list(bins.values()) if key == "test262:all" else []
            payload = export_profiles(binary, profiles, destination, root, objects)
            save(destination / "evidence.json", {"inputs": signature, "coverage_sha256": digest(destination / "coverage.json")})
        graph["observed_targets"].append(key)
        graph["edges"].extend({"source": name, "target": key, "kind": "observed", "evidence": str(directory),
                                "source_sha256": source_state["files"].get(name)} for name in covered_sources(payload, root))
        graph["tests"][key]["elapsed_seconds"] = task["elapsed_seconds"]
        if progress and (index % 20 == 0 or index == len(tasks)):
            progress("Reading measured target coverage", index, len(tasks))
    graph["baseline"] = {"path": str(directory / "full"), "measured_at": data["measured_at"], "rust": data["rust"],
                         "test262": data["test262"], "full_coverage": data["files"], "audit": {
                             "source_files": len(data["files"]) + len(data["no_counters"]), "instrumented_files": len(data["files"]),
                             "complete_files": sum(complete(v) for v in data["files"].values()), "totals": data["totals"],
                             "raw_percentage_regressions": data["coverage_regressions"]}}
    scope = graph["acceptance_scope"]
    scope["selected_files"] = {**scope.get("selected_files", {}), **data["selected"]}
    scope["modified_production_files"] = sorted(set(scope.get("modified_production_files", []))
        | set(existing.get("acceptance_scope", {}).get("modified_production_files", [])) | set(data["modified"]))
    graph["coverage_reference"] = existing.get("coverage_reference", {})
    graph["coverage_reference_provenance"] = existing.get("coverage_reference_provenance", [])
    retain_coverage_reference(graph, data.get("coverage_reference", {}), directory / "report-data.json")
    retain_coverage_reference(graph, data["files"], directory)
    graph.update(warnings=warnings, created_at=now())
    save(state / "graph.json", graph)
    return graph


def latest_baseline(root=ROOT, state=None):
    paths = list((Path(root) / "target").glob("test262-macos-*/coverage-audit.json"))
    paths.extend(((Path(state) if state else Path(root) / "target/bluejs-selftest") / "runs").glob("*/report-data.json"))
    paths.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    for path in paths:
        if path.name == "report-data.json":
            run = load(path.parent / "run.json", {})
            if run.get("status") == "passed" and run.get("mode") in ("full", "pipeline"):
                return path.parent
            continue
        if (path.parent / "test262/summary.json").is_file() and (path.parent / "rust-results.json").is_file():
            return path.parent
    raise ValueError("No retained complete measurement found; specify --baseline")
