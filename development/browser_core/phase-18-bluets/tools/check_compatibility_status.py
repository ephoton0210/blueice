#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Check the four current BlueTS summaries against the versioned evidence.

Dated historical measurements outside the unique current-status block remain
unchanged. This checks documentation consistency, not a new runtime test run.
"""

import argparse
import json
import re
from collections import Counter
from decimal import Decimal
from pathlib import Path

import account_inventory_rows as rows
import inventory_refusals


PHASE = "development/browser_core/phase-18-bluets"
DOCUMENTS = ("CLAUDE.md", *(f"{PHASE}/{name}.md" for name in
                          ("PLAN", "TODO", "COMPATIBILITY_INVENTORY")))
BEGIN = "<!-- BEGIN CURRENT BLUETS STATUS -->"
END = "<!-- END CURRENT BLUETS STATUS -->"


def load_inputs(root):
    directory = root / PHASE
    proof = (directory / rows.MEASUREMENT).read_bytes()
    data = json.loads((directory / "COMPATIBILITY_CASES.json").read_text(encoding="utf-8"))
    catalog = json.loads((directory / "COMPATIBILITY_SUITES.json").read_text(encoding="utf-8"))
    inventory = (directory / "COMPATIBILITY_INVENTORY.md").read_text(encoding="utf-8")
    return data, catalog, json.loads(proof), inventory, inventory_refusals.render(root), proof


def render_status(data, catalog, measurement, inventory, refusals, proof):
    labels = rows.validate_cases(data, measurement, proof)
    rows.require(catalog.get("schema") == 1, "unsupported suite catalog")
    for key in ("typescript", "source_revision", "source_graph_sha256"):
        rows.require(catalog.get(key) == data[key], f"foreign suite catalog {key}")
    rows.require(re.fullmatch(r"(?:backend-tree:)?[a-f0-9]{40}", data["source_revision"])
                 and re.fullmatch(r"[a-f0-9]{64}", data["source_graph_sha256"]), "invalid source binding")
    suites = catalog["suites"]
    names = [suite["harness"] for suite in suites]
    rows.require(len(names) == len(set(names)) and set(names) == {suite["harness"] for suite in data["suites"]},
                 "missing or duplicate catalog suite")
    measured = {suite["harness"]: suite for suite in measurement["suites"]}
    for suite in suites:
        tests = suite["ignored_tests"]
        rows.require(tests and len(tests) == len(set(tests))
                     and sorted(tests) == sorted(measured[suite["harness"]]["pinned_tests"])
                     and set(tests) <= set(measured[suite["harness"]]["test_names"]),
                     "incomplete source-bound oracle selection")
    ignored = sum(len(suite["ignored_tests"]) for suite in suites)
    features = re.findall(r"^\| (G-[A-Z]\d+) \| (closed|open|refused) \|", inventory, re.M)
    expected = {identifier: "closed" if identifier == "G-C2" else "open" for identifier in rows.GAPS}
    rows.require(len(features) == len(expected) and dict(features) == expected, "unaccounted feature status")
    rows.require(inventory.count(rows.DIRECT + "\n") == 1, "missing or duplicate direct runtime profile")
    direct = inventory.split(rows.DIRECT + "\n", 1)[1].split("\n## ", 1)[0]
    rows.require("**Row status: open.**" in direct,
                 "unaccounted direct runtime profile")
    rows.require(inventory.endswith(refusals.rstrip() + "\n"), "stale source refusal accounting")
    refusal_counts = re.match(r"(\d+) refusal sites in (\d+) areas\n", refusals)
    rows.require(refusal_counts, "missing source refusal counts")
    sites, areas = map(int, refusal_counts.groups())
    features = Counter(dict(features).values())
    statuses = Counter(suite["status"] for suite in data["suites"])
    counts = data["counts"]
    controls = counts["reference_only"] + counts["boundary_controls"]
    ratio = Decimal(counts["agreements"]) * 100 / Decimal(counts["comparison_cases"])
    rows.require(data.get("measured_on") == measurement.get("platform"), "foreign measurement platform")
    return f"""{BEGIN}

**Current BlueTS / BlueTSC compatibility status (2026-10-19).**
**Full TypeScript parity: OFF; do not advertise full parity.**

Pinned compiler: **TypeScript {data['typescript']}**. **N = {counts['listed']:,} suite-qualified
case assertions**, not unique programs or a percentage of the TypeScript language.

| Current measurement | Count / scope |
| --- | --- |
| Direct agreement | {counts['agreements']:,}/{counts['comparison_cases']:,} ({ratio:.4f}%); {counts['open']} open, {counts['refused']} refused observations |
| Separate controls | {counts['reference_only']:,} native-reference + {counts['boundary_controls']} BlueTS owner = {controls:,}; excluded from direct agreement |
| Case labels | {labels['closed']:,} scoped closed (including controls), {labels['open']} open, {labels['refused']} refused |
| Suite inventory | {len(suites)} suites / {ignored} ignored Rust functions; not a case denominator |
| Suite labels | {statuses['closed']} scoped closed, {statuses['open']} open, {statuses['refused']} refused |
| Broader G-* feature rows | {len(expected)} total: {features['closed']} scoped closed, {features['open']} open |
| Direct BlueJS runtime profile | Open; separate from the TypeScript comparison claim |
| Source refusal sites | {sites} in {areas} areas; condition-specific locations, not extra failing cases |

All {counts['checked']:,} recorded case expectations pass; expected differences/refusals are not
agreements. The complete case measurement is **{data['measured_on']}**, not all-platform proof.
Accepted baseline runtime evidence (AC-K14.3; its recorded source): Windows
x86_64 VM **82/82** core oracles plus the complete local test gate. This is not
a new Windows run for the compiler/oracle source below. Hosted CI and coverage
are supplementary, not prerequisites. This consistency check does not rerun
those runtime gates.

Compiler/oracle source: `{data['source_revision']}`.
Input graph SHA-256: `{data['source_graph_sha256']}`.
Versioned authorities: `COMPATIBILITY_CASES.json`, `COMPATIBILITY_SUITES.json`
and `COMPATIBILITY_MEASUREMENT.json` in `development/browser_core/phase-18-bluets/`.
Older dated measurements below retain their original scope; they are not this current summary.

{END}"""


def check_document(document, expected, name):
    rows.require(document.count(BEGIN) == document.count(END) == 1, f"{name}: missing or duplicate current-status block")
    start, end = document.index(BEGIN), document.index(END) + len(END)
    rows.require(document[start:end] == expected, f"{name}: current BlueTS status is stale or inconsistent")


def replace_status(document, expected, name):
    rows.require(document.count(BEGIN) == document.count(END) == 1,
                 f"{name}: missing or duplicate current-status block")
    start, end = document.index(BEGIN), document.index(END) + len(END)
    rows.require(start < end - len(END), f"{name}: reversed current-status block")
    updated = document[:start] + expected + document[end:]
    check_document(updated, expected, name)
    return updated


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--check", action="store_true")
    modes.add_argument("--write", action="store_true")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[4])
    options = parser.parse_args()
    root = options.root.resolve()
    block = render_status(*load_inputs(root))
    for relative in DOCUMENTS:
        target = root / relative
        document = target.read_text(encoding="utf-8")
        if options.write:
            with target.open("w", encoding="utf-8", newline="\n") as stream:
                stream.write(replace_status(document, block, relative))
        else:
            check_document(document, block, relative)
    print(f"Current BlueTS status matches validated case/suite/measurement evidence in all {len(DOCUMENTS)} documents")


if __name__ == "__main__":
    main()
