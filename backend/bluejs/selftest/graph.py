# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import collections
import re
import json
import subprocess
from pathlib import Path

from .common import ROOT, changed_files, digest, load, now, relative, snapshot

SOURCE = "backend/bluejs/src/"
TESTS = "backend/bluejs/tests/"
# Changes here can alter dispatch, storage, compilation or the test contract.
BROAD = ("compiler", "parser", "heap", "vm/interpreter", "vm/execution",
         "vm/builtins/native_dispatch", "vm/realm_reentrancy", "vm/lifecycle")
GLOBAL_FILES = {"lib.rs", "vm.rs", "value.rs", "bytecode.rs", "property.rs", "vm/builtins.rs"}
_INVENTORY_CACHE = {}


def inventory(root=ROOT):
    root = Path(root)
    manifest = root / "backend/bluejs/Cargo.toml"
    if manifest.is_file():
        automatic = [str(p) for directory in (root / TESTS, root / SOURCE / "bin", root / "backend/bluejs/examples")
                     for p in sorted(directory.glob("*.rs"))]
        stamp = (str(root.resolve()), digest(manifest), tuple(automatic))
        if stamp not in _INVENTORY_CACHE:
            command = ["cargo", "metadata", "--no-deps", "--offline", "--locked", "--format-version", "1",
                       "--manifest-path", str(manifest)]
            completed = subprocess.run(command, cwd=root, capture_output=True, text=True)
            if completed.returncode:
                raise ValueError("Cargo target discovery failed: " + completed.stderr[-2000:])
            package = next(p for p in json.loads(completed.stdout)["packages"] if p["name"] == "blueice-bluejs")
            targets = {}
            for target in package["targets"]:
                kind = target["kind"][0]
                # Explicit target selection includes examples whose default
                # `test` setting is false; the complete Cargo gate uses all targets.
                if kind not in ("lib", "test", "bin", "example", "bench"):
                    continue
                key = "rust:lib" if kind == "lib" else f"rust:{kind}:{target['name']}"
                targets[key] = {"id": key, "label": "BlueJS unit tests" if kind == "lib" else target["name"],
                                "kind": "rust", "cargo_args": ["--lib"] if kind == "lib" else ["--" + kind, target["name"]],
                                "source": relative(root, target["src_path"])}
            _INVENTORY_CACHE[stamp] = targets
        result = {key: dict(value) for key, value in _INVENTORY_CACHE[stamp].items()}
        result["test262:all"] = {"id": "test262:all", "label": "Complete Test262", "kind": "test262", "filter": ""}
        result["python:selftest"] = {"id": "python:selftest", "label": "Self-test tooling contracts", "kind": "python"}
        return result
    result = {"rust:lib": {"id": "rust:lib", "label": "BlueJS unit tests", "kind": "rust",
                           "cargo_args": ["--lib"], "source": SOURCE + "lib.rs"}}
    for path in sorted((root / TESTS).glob("*.rs")):
        name = path.stem
        key = "rust:test:" + name
        result[key] = {"id": key, "label": name, "kind": "rust", "cargo_args": ["--test", name],
                       "source": path.relative_to(root).as_posix()}
    for path in sorted((root / SOURCE / "bin").glob("*.rs")):
        key = "rust:bin:" + path.stem
        result[key] = {"id": key, "label": path.stem + " binary harness", "kind": "rust",
                       "cargo_args": ["--bin", path.stem], "source": path.relative_to(root).as_posix()}
    for path in sorted((root / "backend/bluejs/examples").glob("*.rs")):
        key = "rust:example:" + path.stem
        result[key] = {"id": key, "label": path.stem + " example harness", "kind": "rust",
                       "cargo_args": ["--example", path.stem], "source": path.relative_to(root).as_posix()}
    result["test262:all"] = {"id": "test262:all", "label": "Complete Test262", "kind": "test262", "filter": ""}
    result["python:selftest"] = {"id": "python:selftest", "label": "Self-test tooling contracts", "kind": "python"}
    return result


def rust_structure(text):
    return sorted(set(re.findall(r"\b(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*(?:<[^;{}]*>)?\s*\(", text)))


def static_edges(root=ROOT):
    """Resolve explicit Rust path/include and qualified crate dependencies.

    Observed coverage handles runtime dispatch. This parser supplies extra
    relationships; it does not assert a complete Rust call graph.
    """
    root = Path(root)
    paths = sorted((root / "backend/bluejs").rglob("*.rs"))
    known = {p.resolve(): p.relative_to(root).as_posix() for p in paths}
    edges = set()
    structures = {}
    for path in paths:
        name = path.relative_to(root).as_posix()
        text = path.read_text()
        structures[name] = rust_structure(text)
        for match in re.finditer(r'(?:#\[path\s*=\s*|include!\s*\(\s*)"([^"\n]+)"', text):
            dependency = (path.parent / match.group(1)).resolve()
            if dependency in known:
                edges.add((known[dependency], name, "include"))
        for match in re.finditer(r"\bcrate::([A-Za-z_][\w]*(?:::[A-Za-z_][\w]*)*)", text):
            parts = match.group(1).split("::")
            for end in range(len(parts), 0, -1):
                stem = root / SOURCE / Path(*parts[:end])
                candidates = [stem.with_suffix(".rs"), stem / "mod.rs"]
                dependency = next((p.resolve() for p in candidates if p.resolve() in known), None)
                if dependency is not None:
                    if known[dependency] != name:
                        edges.add((known[dependency], name, "static"))
                    break
    from .shared_functions import shared_function_edges, shared_function_graph
    for edge in shared_function_edges(shared_function_graph(root)):
        edges.add((edge["source"], edge["target"], edge["kind"]))
    return [{"source": a, "target": b, "kind": kind} for a, b, kind in sorted(edges)], structures


def empty_graph(root=ROOT):
    edges, structures = static_edges(root)
    scope = load(Path(root) / "backend/bluejs/selftest/acceptance-scope.json", {})
    return {"schema": 1, "created_at": now(), "root": str(Path(root).resolve()), "tests": inventory(root),
            "edges": edges, "structures": structures, "source_hashes": snapshot(root)["files"],
            "observed_targets": [], "baseline": None, "acceptance_scope": scope,
            "originally_complete": scope.get("originally_complete_files", []),
            "warnings": ["No observed execution profiles have been indexed."]}


def plan(graph, files=None, root=ROOT, base=None):
    root = Path(root)
    files = changed_files(root, base) if files is None else sorted({relative(root, p) for p in files})
    tests = inventory(root)
    # Keep timings, commands and measured metadata, while including new targets.
    tests = {key: {**graph.get("tests", {}).get(key, {}), **value} for key, value in tests.items()}
    edges, structures = static_edges(root)
    old_edges = [e for e in graph.get("edges", []) if e["kind"] == "observed"]
    adjacency = collections.defaultdict(list)
    for edge in old_edges + edges:
        adjacency[edge["source"]].append(edge)
    for key, test in tests.items():
        if test.get("source"):
            adjacency[test["source"]].append({"source": test["source"], "target": key, "kind": "test-source"})
    reasons = collections.defaultdict(list)
    affected = set(files)
    fallbacks = []
    visited_edges = []
    from .shared_functions import body_contract
    body_sources = set()
    for name in files:
        contract = graph.get("shared_body_contracts", {}).get(name)
        path = root / name
        if contract and path.is_file() and body_contract(path.read_text()) == contract["structural_sha256"]:
            if any(e["source"] == name and e["kind"] == "observed"
                   and e.get("source_sha256") == contract["source_sha256"]
                   and e.get("snapshot") == contract["snapshot"] for e in old_edges):
                body_sources.add(name)

    def all_engine(reason):
        fallbacks.append(reason)
        for key, test in tests.items():
            if test["kind"] in ("rust", "test262"):
                reasons[key].append(reason)

    for name in files:
        if name.startswith("backend/bluejs/selftest/") or name == TESTS + "test_selftest.py":
            reasons["python:selftest"].append("Self-test tool changed: " + name)
            continue
        if name.startswith("development/") or name.endswith((".md", ".png", ".svg")):
            continue
        if name.startswith(SOURCE):
            short = name[len(SOURCE):]
            if name in graph.get("shared_body_contracts", {}) and name not in body_sources:
                all_engine("Shared boundary interface changed or current evidence is unavailable: " + name)
            if name not in body_sources and (short in GLOBAL_FILES or short.startswith("bin/") or any(short == part + ".rs" or short.startswith(part + "/") for part in BROAD)):
                all_engine("Shared engine contract changed: " + name)
            if not (root / name).is_file() or name not in graph.get("source_hashes", {}):
                all_engine("New or removed production source: " + name)
            elif structures.get(name) != graph.get("structures", {}).get(name):
                all_engine("Function structure changed; recorded coverage may omit new paths: " + name)
            elif not any(e["kind"] == "observed" for e in adjacency[name]):
                all_engine("No measured test relationship for: " + name)
        elif name.startswith(TESTS) and name.endswith(".rs"):
            if name not in graph.get("source_hashes", {}):
                # A new integration target has a direct relationship; fixtures need a fallback.
                if not any(t.get("source") == name for t in tests.values()):
                    all_engine("Unmapped new regression fixture: " + name)
        elif name.startswith(("backend/bluejs/test262/", ".cargo/")) or name in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain", "backend/bluejs/Cargo.toml"}:
            all_engine("Build or conformance configuration changed: " + name)
        elif name.startswith("backend/") or name.startswith("scripts/"):
            all_engine("Dependency change outside the recorded BlueJS graph: " + name)
    for origin in files:
        queue = collections.deque([(origin, [])])
        seen = {origin}
        while queue:
            node, path = queue.popleft()
            for edge in adjacency[node]:
                if origin in body_sources and edge["kind"] == "shared-function":
                    # A complete gate measured actual entry to this module.
                    # Its interface is identical; lexical caller-file edges
                    # would otherwise merge unrelated branches in dispatch.
                    continue
                target = edge["target"]
                if target in seen:
                    continue
                seen.add(target)
                chain = path + [edge]
                visited_edges.append(edge)
                if target in tests:
                    reasons[target].append(" → ".join([origin] + [e["target"] for e in chain]))
                elif (root / target).is_file():
                    affected.add(target)
                    queue.append((target, chain))
    # An incomplete import must not silently omit unmeasured targets.
    if any(p.startswith(SOURCE) for p in files):
        for key, test in tests.items():
            if test["kind"] in ("rust", "test262") and key not in graph.get("observed_targets", []):
                reasons[key].append("Target has no complete observed profile; include conservatively")
    selected = [{**tests[key], "reasons": sorted(set(value))} for key, value in sorted(reasons.items())]
    current = snapshot(root)
    return {"schema": 1, "created_at": now(), "snapshot": current["identity"], "changed_files": files,
            "affected_files": sorted(affected), "tests": selected, "total_tests": len(tests),
            "selected_tests": len(selected), "fallbacks": sorted(set(fallbacks)),
            "estimated_serial_seconds": round(sum(t.get("elapsed_seconds", 0) for t in selected), 3),
            "edges": list({(e["source"], e["target"], e["kind"]): e for e in visited_edges}.values()),
            "graph_created_at": graph.get("created_at"),
            "shared_body_contract_sources": sorted(body_sources),
            "note": "Observed relationships describe executed paths. The final full run checks behavior beyond those observations."}
