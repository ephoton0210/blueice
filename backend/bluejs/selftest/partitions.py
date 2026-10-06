# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Rust boundary graph and native libtest partitions for correction work."""
import hashlib
import json
import re
from pathlib import Path

from .common import digest, load, snapshot
from .graph import inventory, plan


REFERENCE_FILTERS = ("with_", "with_scope", "eval", "capture", "destructur", "reference",
                     "global", "staged_interpreter_name")
REFERENCE_TARGET_PREFIXES = ("with_", "eval_", "destructuring_")
REFERENCE_TEST262 = tuple("staging/sm/expressions/destructuring-array-default-" + suffix + ".js"
                         for suffix in ("call", "class", "function-nested", "function", "simple"))
BUFFER_FILTERS = ("typed_array", "typed_arrays", "binary_data", "foreign", "cross_realm", "buffer", "species", "test262")
BUFFER_SOURCES = re.compile(r"\b(?:ArrayBuffer|SharedArrayBuffer|DataView|TypedArray|(?:Uint|Int|Float|BigInt|BigUint)\d+(?:Clamped)?Array)\b")
BUFFER_TEST262 = ("built-ins/TypedArray/", "built-ins/TypedArrayConstructors/",
                  "built-ins/ArrayBuffer/", "built-ins/SharedArrayBuffer/", "built-ins/DataView/")
DELETION_FILTERS = ("delete", "eval", "global", "with", "object", "capture")
DELETION_SOURCES = re.compile(r"\bdelete\b|Reflect\.deleteProperty")
DELETION_TEST262 = ("language/expressions/delete/", "language/eval-code/", "language/global-code/")


def mask_rust(text):
    """Keep structural braces while hiding comments and Rust string literals."""
    pattern = re.compile(r'''//[^\n]*|/\*|r(\#*)"|"|'(?:\\(?:x[\da-fA-F]{2}|u\{[\da-fA-F]+\}|.)|[^'\\\n])'|\bfn\b''', re.M)
    result = list(text)
    position = 0
    while match := pattern.search(text, position):
        start, token = match.start(), match.group()
        if token == "fn":
            position = match.end()
            continue
        if token == "/*":
            end, depth = match.end(), 1
            while depth and end < len(text):
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        elif token.startswith("r"):
            delimiter = '"' + match.group(1)
            finish = text.find(delimiter, match.end())
            end = len(text) if finish < 0 else finish + len(delimiter)
        elif token == '"':
            end = match.end()
            while end < len(text):
                if text[end] == "\\":
                    end += 2
                elif text[end] == '"':
                    end += 1
                    break
                else:
                    end += 1
        else:
            end = match.end()
        result[start:end] = ["\n" if c == "\n" else " " for c in text[start:end]]
        position = end
    return "".join(result)


def bodies(text, pattern):
    masked, result = mask_rust(text), {}
    for match in re.finditer(pattern, masked):
        opening = masked.find("{", match.end())
        if opening < 0:
            continue
        depth, end = 1, opening + 1
        while depth and end < len(masked):
            depth += (masked[end] == "{") - (masked[end] == "}")
            end += 1
        result.setdefault(match.group(1), []).append(text[match.start():end])
    return result


def changed_symbols(before, after):
    old, new = bodies(before, r"\bfn\s+(\w+)\s*"), bodies(after, r"\bfn\s+(\w+)\s*")
    return sorted(name for name in old.keys() | new.keys() if old.get(name) != new.get(name))


def changed_opcodes(before, after):
    pattern = r"Opcode::(\w+)\s*=>\s*"
    old, new = bodies(before, pattern), bodies(after, pattern)
    return sorted(name for name in old.keys() | new.keys() if old.get(name) != new.get(name))


def outside_functions(text):
    """Do not apply a narrow body contract when imports or shared data changed."""
    masked, parts, position = mask_rust(text), [], 0
    for match in re.finditer(r"\bfn\s+(\w+)\s*", masked):
        if match.start() < position:
            continue
        opening = masked.find("{", match.end())
        if opening < 0:
            return text
        depth, end = 1, opening + 1
        while depth and end < len(masked):
            depth += (masked[end] == "{") - (masked[end] == "}")
            end += 1
        parts.append(text[position:match.start()])
        position = end
    parts.append(text[position:])
    return "".join(parts)


def public_reference_cases(text):
    """Find public test contracts independently of their Cargo target names."""
    tests = bodies(text, r"#\[test\]\s*(?:#\[[^\n]*\]\s*)*fn\s+(\w+)\s*")
    return sorted(name for name, implementations in tests.items()
                  if any(re.search(r"(?<![.\w$])with\s*\(|\b(?:ResolveWithReference|load_with_reference|store_with_reference)\b", body)
                         for body in implementations))


def public_wrapper_cases(text, wrappers):
    tests = bodies(text, r"#\[test\]\s*(?:#\[[^\n]*\]\s*)*fn\s+(\w+)\s*")
    return sorted(name for name, implementations in tests.items()
                  if any(re.search(r"\b" + re.escape(wrapper) + r"\s*\(", mask_rust(body))
                         for body in implementations for wrapper in wrappers))


def latest_rust_anchor(manager):
    required = {key for key, t in inventory(manager.root).items() if t["kind"] == "rust"}
    for row in manager.history(limit=None):
        passed = {t["id"].split(":", 1)[-1] for t in row.get("tasks", [])
                  if t.get("status") == "passed" and t.get("counts") and not t.get("case")}
        if required <= passed and row.get("status") not in ("stale", "running", "interrupted"):
            directory = manager.state / "runs" / row["id"]
            state = load(directory / "source-state.json", {})
            frozen = load(directory / "frozen-rust-sources.json", {})
            rust_sources = {name for name in state.get("files", {}) if name.endswith(".rs")}
            if state.get("identity") == row.get("snapshot") and rust_sources and frozen.keys() == rust_sources:
                for name, text in frozen.items():
                    if hashlib.sha256(text.encode()).hexdigest() != state.get("files", {}).get(name):
                        raise ValueError("Rust anchor source text does not match its recorded hash")
                return row["id"], state, frozen
    return None, {}, {}


def selection_identity(selection):
    """A passing target with different case filters is a different prerequisite."""
    keys = ("id", "kind", "target_id", "cargo_args", "case_filters", "filter", "required_modes")
    contract = [{key: test[key] for key in keys if key in test}
                for test in sorted(selection["tests"], key=lambda test: test["id"])]
    return hashlib.sha256(json.dumps(contract, sort_keys=True).encode()).hexdigest()


def passed_partition(manager, selection):
    expected = selection_identity(selection)
    for row in manager.history():
        if row.get("mode") != "partition" or row.get("status") != "passed" or row.get("snapshot") != selection["snapshot"]:
            continue
        retained = row.get("selection_identity")
        if retained is None:
            previous = load(manager.state / "runs" / row["id"] / "plan.json")
            retained = selection_identity(previous) if previous else None
        if retained == expected:
            return row
    return None


def partition_plan(manager, files=None, base=None):
    current = snapshot(manager.root)
    anchor, previous, frozen = latest_rust_anchor(manager)
    targets = inventory(manager.root)
    changed = sorted(name for name in current["files"].keys() | previous.get("files", {}).keys()
                     if current["files"].get(name) != previous.get("files", {}).get(name))
    if files is not None:
        # Reuse repository path validation from the ordinary planner.
        changed = plan(manager.graph(), files, manager.root, base)["changed_files"]
    selected, boundaries, unknown, non_rust = {}, [], [], []

    def add(target, filters, reason):
        item = selected.setdefault(target, {**targets[target], "case_filters": [], "reasons": []})
        item["case_filters"] = sorted(set(item["case_filters"] + list(filters)))
        item["reasons"].append(reason)

    for name in changed:
        if not name.endswith(".rs"):
            non_rust.append(name)
            continue
        path = manager.root / name
        if not path.is_file() or not anchor:
            unknown.append(name)
            continue
        after = path.read_text()
        symbols = changed_symbols(frozen.get(name, ""), after)
        body_only = outside_functions(frozen.get(name, "")) == outside_functions(after)
        opcodes = changed_opcodes(frozen.get(name, ""), after) if name.endswith("/interpreter.rs") else []
        reference = body_only and ((name.endswith("/vm/operations.rs") and bool(symbols) and set(symbols) <= {
            "load_with_reference", "store_with_reference", "classify", "describe",
            "a_with_reference_is_an_object_property_a_slot_or_an_unresolvable_name"}) or (
            name.endswith("/vm/interpreter.rs") and symbols == ["interpret_frame"] and
            bool(opcodes) and set(opcodes) <= {"ResolveWithReference", "LoadWithReference", "StoreWithReference",
                                              "StoreResolvedWithReference", "UpdateWithReference"}))
        buffer = body_only and bool(symbols) and (
            (name.endswith("/vm/builtins/typed_arrays.rs") and set(symbols) <= {
                "typed_array_slice", "typed_array_create_foreign_target"}) or
            (name.endswith("/vm/test262/foreign.rs") and symbols == ["test262_refresh_foreign_buffer_mirrors"]))
        deletion = body_only and bool(symbols) and (
            (name.endswith("/vm/builtins/object.rs") and symbols == ["object_delete"]) or
            (name.endswith("/vm/execution.rs") and symbols == ["delete_unbound_name"]))
        if reference:
            reason = "Reference resolution contract: " + name
            add("rust:lib", REFERENCE_FILTERS, reason)
            for key, target in targets.items():
                if target["kind"] == "rust" and target["label"].startswith(REFERENCE_TARGET_PREFIXES):
                    add(key, [""], reason)
                elif target["kind"] == "rust" and target.get("source", "").startswith("backend/bluejs/tests/"):
                    source = manager.root / target["source"]
                    filters = public_reference_cases(source.read_text()) if source.is_file() else []
                    if filters:
                        add(key, filters, reason + "; public case source contains the related contract")
            if "rust:test:try_completion" in targets:
                add("rust:test:try_completion", ["generators_suspend_resume_and_close_as_iterators",
                    "with_scopes", "direct_eval"], reason)
            boundaries.append({"source": name, "symbols": symbols, "opcodes": opcodes, "contract": "Reference resolution"})
        elif deletion:
            reason = "Global property deletion and captured eval bindings: " + name
            add("rust:lib", DELETION_FILTERS, reason)
            for key, target in targets.items():
                if target["kind"] == "rust" and target.get("source", "").startswith("backend/bluejs/tests/"):
                    source = manager.root / target["source"]
                    if source.is_file() and DELETION_SOURCES.search(source.read_text()):
                        add(key, [""], reason + "; public source exercises deletion")
            boundaries.append({"source": name, "symbols": symbols, "contract": "Global property deletion"})
        elif buffer:
            reason = "TypedArray species and buffer mirror contract: " + name
            add("rust:lib", BUFFER_FILTERS, reason)
            for key, target in targets.items():
                if target["kind"] == "rust" and target.get("source", "").startswith("backend/bluejs/tests/"):
                    source = manager.root / target["source"]
                    if source.is_file() and BUFFER_SOURCES.search(source.read_text()):
                        # Constants and reusable script matrices can sit outside
                        # individual test bodies; keep every case in that owner.
                        add(key, [""], reason + "; public source contains the related buffer contract")
            boundaries.append({"source": name, "symbols": symbols, "contract": "TypedArray and buffers"})
        elif "/tests/fixtures/" in name and symbols:
            test_names = [symbol for symbol in symbols if re.search(
                r"#\[cfg_attr\(test,\s*test\)\]\s*fn\s+" + re.escape(symbol) + r"\b", after)]
            if test_names and set(symbols) <= set(test_names) | {"verify_execution_boundary_contracts"}:
                add("rust:lib", test_names, "Changed regression fixture: " + name)
                wrappers = re.findall(r"\bpub\s+fn\s+(verify_\w+)\s*\(", mask_rust(after))
                for key, target in targets.items():
                    if target["kind"] == "rust" and target.get("source", "").startswith("backend/bluejs/tests/"):
                        source = manager.root / target["source"]
                        filters = public_wrapper_cases(source.read_text(), wrappers) if source.is_file() else []
                        if filters:
                            add(key, filters, "Ordinary-library fixture entry point: " + name)
                boundaries.append({"source": name, "symbols": symbols, "contract": "Regression fixture"})
            else:
                unknown.append(name)
        elif "/tests/" in name and any(t.get("source") == name for t in targets.values()):
            for key, target in targets.items():
                if target.get("source") == name:
                    add(key, [""], "Changed public Rust contract: " + name)
        else:
            unknown.append(name)
    if unknown or non_rust:
        fallback = plan(manager.graph(), unknown + non_rust, manager.root, base)
        conformance_inputs = (plan(manager.graph(), non_rust, manager.root, base)["tests"]
                              if unknown and non_rust else fallback["tests"] if non_rust else [])
        full_conformance = any(test["kind"] == "test262" for test in conformance_inputs)
        for target in fallback["tests"]:
            if target["kind"] == "rust":
                add(target["id"], [""], "; ".join(target["reasons"]))
            elif target["kind"] == "python" or (target["kind"] == "test262" and full_conformance):
                selected.setdefault(target["id"], target)
    reference = any(boundary["contract"] == "Reference resolution" for boundary in boundaries)
    buffer = any(boundary["contract"] == "TypedArray and buffers" for boundary in boundaries)
    deletion = any(boundary["contract"] == "Global property deletion" for boundary in boundaries)
    if "test262:all" not in selected and (reference or buffer or deletion):
        key = ("test262:partition:buffer-contracts" if buffer else
               "test262:partition:global-deletion" if deletion else "test262:partition:reference-resolution")
        filters = ((REFERENCE_TEST262 if reference else ()) + (BUFFER_TEST262 if buffer else ())
                   + (DELETION_TEST262 if deletion else ()))
        label = ("Related buffer, Reference and deletion contracts" if deletion else
                 "Related TypedArray and buffer contracts" if buffer else "Related Test262 Reference contracts (5 files, 10 modes)")
        selected[key] = {**targets["test262:all"], "id": key, "target_id": "test262:all",
                         "label": label, "case": "buffer-contracts" if buffer else "global-deletion" if deletion else "reference-resolution",
                         "case_filters": list(filters), "filter": ",".join(filters),
                         "required_modes": [(path, mode) for path in (REFERENCE_TEST262 if reference else ()) for mode in ("sloppy", "strict")],
                         "reasons": [boundary["contract"] for boundary in boundaries if boundary["contract"] != "Regression fixture"]}
    return {"snapshot": current["identity"], "anchor": anchor, "changed_files": changed,
            "boundaries": boundaries, "unknown_sources": unknown, "tests": list(selected.values()),
            "selected_targets": sum(t["kind"] == "rust" for t in selected.values()), "total_rust_targets": sum(t["kind"] == "rust" for t in targets.values()),
            "full_verification_required": True}


def case_partitions(target, listing):
    names = sorted(set(re.findall(r"^(.+): test$", listing, re.M)))
    chosen = [name for name in names if any(part in name for part in target["case_filters"])]
    if not chosen and names:
        raise ValueError("Rust boundary filters matched no cases in " + target["id"])
    groups = {}
    for name in chosen:
        prefix = name.rpartition("::")[0] or "public"
        groups.setdefault(prefix, []).append(name)
    return [{**target, "id": target["id"] + ":partition:" + prefix, "target_id": target["id"],
             "case": prefix, "case_names": cases, "label": target["label"] + " / " + prefix,
             "partition": True} for prefix, cases in sorted(groups.items())]
