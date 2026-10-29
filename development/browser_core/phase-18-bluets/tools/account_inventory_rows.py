#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Generate/check feature and refusal labels; verify every case evidence pointer."""

import argparse
import hashlib
import json
import re
from collections import Counter
from pathlib import Path

import inventory_refusals


DIRECT = "### 3.6 Direct runtime (BlueJS) profile"
REFUSALS = "## 4. Refusals: what BlueTS knows it does not do"
MEASUREMENT = "COMPATIBILITY_MEASUREMENT.json"
GAPS = {
    "G-T1": "narrowing_checker_matrix", "G-T2": "inference_checker_matrix",
    "G-T3": "operators_checker_matrix", "G-T4": "overloads_checker_matrix",
    "G-T5": "more_types_checker_matrix", "G-T6": "compatibility_checker_matrix",
    "G-T7": "gt7_library_manifest", "G-T8": "inferred_return_checker_matrix",
    "G-T9": "gt9_classes", "G-T10": "gt10_import_types",
    "G-E1": "targets_checker_matrix", "G-E2": "module_systems",
    "G-E3": "decorator_targets", "G-E4": "output_options",
    "G-E5": "source_map_tokens_oracle", "G-D1": "gd1_declarations",
    "G-D2": "gd2_declaration_maps", "G-M1": "gm1_resolver_compositions",
    "G-M2": "gm2_build_interop", "G-M3": "typescript_oracle",
    "G-C1": "diagnostic_source_families", "G-C2": "strictness_flags",
    "G-C3": "jsx_typing_extras",
}
CLASSIFICATIONS = {
    "compared": ("closed", "compared_properties"),
    "open": ("open", "documented_difference"),
    "refused": ("refused", "documented_refusal"),
    "reference_only": ("closed", "native_reference_control_only"),
    "boundary_control": ("closed", "bluets_owner_control_only"),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def counts(cases):
    kinds = Counter(row["comparison"] for row in cases)
    return {
        "listed": len(cases), "checked": len(cases), "agreements": kinds["compared"],
        "comparison_cases": kinds["compared"] + kinds["open"] + kinds["refused"],
        "open": kinds["open"], "refused": kinds["refused"],
        "reference_only": kinds["reference_only"], "boundary_controls": kinds["boundary_control"],
    }


def suite_status(row_counts):
    return "open" if row_counts["open"] else "refused" if row_counts["refused"] else "closed"


def validate_cases(data, measurement, measurement_bytes=None):
    require(data.get("schema") == 2 and measurement.get("schema") == 1, "unsupported accounting schema")
    require(data.get("typescript") == measurement.get("typescript") == "5.9.3", "unpinned compiler")
    for key in ["source_revision", "source_graph_sha256"]:
        require(data.get(key) == measurement.get(key) and data.get(key), f"foreign {key}")
    if measurement_bytes is not None:
        require(data["measurement_sha256"] == hashlib.sha256(measurement_bytes).hexdigest(), "foreign measurement bytes")
    require(measurement.get("conclusion") == "success" and measurement.get("finished_at"), "incomplete measurement")
    names = [row["harness"] for row in data["suites"]]
    measured = {row["harness"]: (index, row) for index, row in enumerate(measurement["suites"])}
    require(len(measured) == len(measurement["suites"]) and len(set(names)) == len(names)
            and set(names) == set(measured) and names, "missing or duplicate suite accounting")
    labels, total = Counter(), Counter()
    for suite in data["suites"]:
        index, proof = measured[suite["harness"]]
        evidence = f"{MEASUREMENT}#/suites/{index}"
        require(suite.get("evidence") == evidence, "dangling suite evidence")
        require(proof.get("conclusion") == "success" and all(proof.get(key) == 0 for key in ["failed", "ignored", "filtered"]),
                "failed or incomplete measured suite")
        require(re.fullmatch(r"[a-f0-9]{64}", proof.get("log_sha256", ""))
                and suite.get("log_sha256") == proof["log_sha256"], "foreign log evidence")
        require(proof.get("passed") == len(proof.get("test_names", [])) and proof["passed"] > 0,
                "empty or partial measured test selection")
        cases = suite["cases"]
        require(cases and len({row["id"] for row in cases}) == len(cases), "empty or duplicate case accounting")
        for row in cases:
            expected = CLASSIFICATIONS.get(row.get("comparison"))
            require(expected and (row.get("status"), row.get("scope")) == expected, "unlabelled or mis-scoped case")
            source = row["source"]
            require(source.startswith("backend/") and "\\" not in source and ":" not in source
                    and ".." not in Path(source).parts,
                    "non-canonical assertion source")
            require(row["id"] == f'{source}#{row["selector"]}', "foreign assertion identity")
            require(row.get("evidence") == {"assertion": row["id"], "measurement": evidence}, "dangling case evidence")
            if row["comparison"] != "compared":
                require(row.get("reason"), "unexplained gap, refusal or control")
            labels[row["status"]] += 1
        current = counts(cases)
        require(suite["counts"] == current and suite.get("status") == suite_status(current), "incorrect suite accounting")
        total.update(current)
    require(dict(total) == data["counts"], "incorrect global accounting")
    return labels


def suite_pointer(data, stem):
    found = [index for index, row in enumerate(data["suites"]) if row["harness"].endswith(f"/{stem}.rs")]
    require(len(found) == 1, f"ambiguous or missing suite evidence: {stem}")
    return found[0], f"COMPATIBILITY_CASES.json#/suites/{found[0]}"


def render(document, data, root):
    lines = document.splitlines()
    gap_ids = [re.match(r"\| (G-[A-Z]\d+) \|", line).group(1)
               for line in lines if re.match(r"\| G-[A-Z]\d+ \|", line)]
    require(len(gap_ids) == len(set(gap_ids)) and set(gap_ids) == set(GAPS), "missing, duplicate or unknown gap row")
    for index, line in enumerate(lines):
        match = re.fullmatch(r"\| (G-[A-Z]\d+) \| (.*) \|", line)
        if not match:
            continue
        identifier, body = match.groups()
        body = re.sub(r"^(closed|open|refused) \| ", "", body)
        body = re.sub(r" \| \[evidence\]\([^)]*\)$", "", body)
        status = "closed" if identifier == "G-C2" else "open"
        row_index, evidence = suite_pointer(data, GAPS[identifier])
        if status == "closed":
            require(data["suites"][row_index]["status"] == "closed" and "compared subset" in body,
                    "strictness closure has lost its measured scope")
        lines[index] = f"| {identifier} | {status} | {body} | [evidence]({evidence}) |"
    document = "\n".join(lines) + "\n"
    document = document.replace("| ID | Gap |\n| --- | --- |", "| ID | Status | Gap / measured scope | Evidence |\n| --- | --- | --- | --- |")
    require(document.count(DIRECT) == 1 and document.count(REFUSALS) == 1, "missing or duplicate inventory section")
    before, after = document.split(DIRECT + "\n", 1)
    after = re.sub(r"^\s*\*\*Row status:[^\n]*\n", "", after).lstrip("\n")
    _, evidence = suite_pointer(data, "emitted_target_editions")
    document = f"{before}{DIRECT}\n\n**Row status: open.** [evidence]({evidence})\n\n{after}"
    before, _ = document.split(REFUSALS, 1)
    return (f"{before}{REFUSALS}\n\n"
            "Generated by `tools/inventory_refusals.py` from current production source.\n"
            "Each refused label applies to the condition in its diagnostic, not the entire feature.\n\n"
            + inventory_refusals.render(root).rstrip() + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--write", action="store_true")
    modes.add_argument("--check", action="store_true")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[4])
    options = parser.parse_args()
    root = options.root.resolve()
    directory = root / "development/browser_core/phase-18-bluets"
    data = json.loads((directory / "COMPATIBILITY_CASES.json").read_text(encoding="utf-8"))
    proof = (directory / MEASUREMENT).read_bytes()
    labels = validate_cases(data, json.loads(proof), proof)
    for source in {row["source"] for suite in data["suites"] for row in suite["cases"]}:
        path = (root / source).resolve()
        require(path.is_relative_to(root) and path.is_file(), f"missing or escaping assertion source: {source}")
    path = directory / "COMPATIBILITY_INVENTORY.md"
    original = path.read_text(encoding="utf-8")
    generated = render(original, data, root)
    if options.write:
        with path.open("w", encoding="utf-8", newline="\n") as stream:
            stream.write(generated)
    else:
        require(original == generated, "inventory has unlabelled, stale or missing feature/refusal rows; regenerate")
    print(f'Accounted {sum(labels.values())} cases: {labels["closed"]} closed (scoped), '
          f'{labels["open"]} open, {labels["refused"]} refused; {len(GAPS)} feature rows and direct profile checked')


if __name__ == "__main__":
    main()
