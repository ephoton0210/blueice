# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Public contracts for selecting and executing tests without a full engine run."""
import json
import sys
import tempfile
import threading
import time
import unittest
import urllib.request
import urllib.error
from unittest.mock import patch
from types import SimpleNamespace
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from backend.bluejs.selftest.common import digest, relative, retain_coverage_reference, snapshot, validate_corpus
from backend.bluejs.selftest.graph import empty_graph, inventory, plan, static_edges
from backend.bluejs.selftest.coverage import covered_sources, import_baseline, latest_baseline, profile_owners
from backend.bluejs.selftest.report import compare_outcomes, coverage_summary, publish_report, render_report
from backend.bluejs.selftest.runner import Manager, refresh_observations, test262_passed, validate_impact
from backend.bluejs.selftest.shared_functions import body_contract, shared_function_edges, shared_function_graph, verified_body_contracts
from backend.bluejs.selftest.partitions import (case_partitions, changed_opcodes, changed_symbols, latest_rust_anchor,
                                              partition_plan, passed_partition, public_reference_cases, selection_identity)
from backend.bluejs.selftest.server import Application, make_server


class ImpactContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.write("backend/bluejs/src/lib.rs", "pub struct Vm;\n")
        self.write("backend/bluejs/src/vm/builtins/feature.rs", "fn feature() {}\n")
        self.write("backend/bluejs/tests/feature.rs", "#[test] fn feature_contract() {}\n")
        self.write("backend/bluejs/tests/other.rs", "#[test] fn other_contract() {}\n")
        self.graph = empty_graph(self.root)
        self.graph["observed_targets"] = [key for key, value in self.graph["tests"].items() if value["kind"] in ("rust", "test262")]
        self.graph["edges"].append({"source": "backend/bluejs/src/vm/builtins/feature.rs", "target": "rust:test:feature", "kind": "observed"})

    def tearDown(self):
        self.temp.cleanup()

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def selected(self, result):
        return {test["id"] for test in result["tests"]}

    def test_behavior_edit_selects_recorded_target(self):
        self.write("backend/bluejs/src/vm/builtins/feature.rs", "fn feature() { let changed = 1; }\n")
        result = plan(self.graph, ["backend/bluejs/src/vm/builtins/feature.rs"], self.root)
        self.assertEqual(self.selected(result), {"rust:test:feature"})
        self.assertTrue(result["tests"][0]["reasons"])

    def test_new_function_expands_to_full_engine(self):
        self.write("backend/bluejs/src/vm/builtins/feature.rs", "fn feature() {} fn added() {}\n")
        result = plan(self.graph, ["backend/bluejs/src/vm/builtins/feature.rs"], self.root)
        self.assertIn("test262:all", self.selected(result))
        self.assertIn("rust:test:other", self.selected(result))
        self.assertTrue(result["fallbacks"])

    def test_unmapped_source_expands_instead_of_skipping(self):
        self.write("backend/bluejs/src/vm/builtins/new.rs", "fn new() {}")
        self.assertIn("rust:test:other", self.selected(plan(self.graph, ["backend/bluejs/src/vm/builtins/new.rs"], self.root)))

    def test_removed_source_expands_to_full_engine(self):
        name = "backend/bluejs/src/vm/builtins/feature.rs"
        (self.root / name).unlink()
        self.assertIn("test262:all", self.selected(plan(self.graph, [name], self.root)))

    def test_static_include_transitively_selects_fixture_owner(self):
        self.write("backend/bluejs/tests/fixtures/input.rs", "fn fixture() {}")
        self.write("backend/bluejs/tests/feature.rs", '#[path = "fixtures/input.rs"] mod input;')
        graph = empty_graph(self.root)
        result = plan(graph, ["backend/bluejs/tests/fixtures/input.rs"], self.root)
        self.assertEqual(self.selected(result), {"rust:test:feature"})

    def test_new_integration_test_runs_itself(self):
        self.write("backend/bluejs/tests/new_case.rs", "#[test] fn new_case() {}")
        result = plan(self.graph, ["backend/bluejs/tests/new_case.rs"], self.root)
        self.assertEqual(self.selected(result), {"rust:test:new_case"})

    def test_unmeasured_target_is_included(self):
        self.graph["observed_targets"].remove("rust:test:other")
        result = plan(self.graph, ["backend/bluejs/src/vm/builtins/feature.rs"], self.root)
        self.assertIn("rust:test:other", self.selected(result))

    def test_core_change_selects_all_engine_targets(self):
        result = plan(self.graph, ["backend/bluejs/src/lib.rs"], self.root)
        self.assertIn("test262:all", self.selected(result))
        self.assertIn("rust:test:other", self.selected(result))

    def test_tool_change_selects_tool_contracts(self):
        result = plan(self.graph, ["backend/bluejs/selftest/server.py"], self.root)
        self.assertEqual(self.selected(result), {"python:selftest"})

    def test_documentation_does_not_schedule_engine(self):
        result = plan(self.graph, ["development/results.md"], self.root)
        self.assertEqual(self.selected(result), set())

    def test_dependency_change_expands_engine(self):
        result = plan(self.graph, ["backend/ecma402/src/lib.rs"], self.root)
        self.assertIn("test262:all", self.selected(result))

    def test_snapshot_invalidates_results_when_fixture_changes(self):
        before = snapshot(self.root)["identity"]
        self.write("backend/bluejs/tests/feature.rs", "#[test] fn changed() {}")
        self.assertNotEqual(before, snapshot(self.root)["identity"])

    def test_repository_path_cannot_escape(self):
        with self.assertRaises(ValueError):
            relative(self.root, "../outside")

    def test_grouped_crate_import_retains_dependency(self):
        self.write("backend/bluejs/src/feature.rs", "pub struct Feature;")
        self.write("backend/bluejs/src/consumer.rs", "use crate::feature::{Feature};")
        edges, _ = static_edges(self.root)
        self.assertIn({"source": "backend/bluejs/src/feature.rs", "target": "backend/bluejs/src/consumer.rs", "kind": "static"}, edges)

    def test_shared_function_references_connect_isolated_owners_and_skip_literals(self):
        owner = "backend/bluejs/src/vm/classification.rs"
        caller = "backend/bluejs/src/vm/consumer.rs"
        self.write(owner, "impl Vm { fn is_callable(&self, value: &Value) -> bool { true } }")
        self.write(caller, '''impl Vm {
            fn related(&self) {
                // self.is_callable(comment);
                let text = "self.is_callable(string)";
                let raw = r#"self.is_callable(raw)"#;
                self.is_callable(actual);
                fn nested() { unrelated(); }
                self.is_callable(after_nested);
            }
        }''')
        entry = next(item for item in shared_function_graph(self.root) if item["symbol"] == "is_callable")
        self.assertEqual(len(entry["definitions"]), 1)
        self.assertEqual([ref["function"] for ref in entry["references"]], ["related", "related"])
        self.assertEqual({ref["source"] for ref in entry["references"]}, {caller})
        self.assertEqual(entry["definitions"][0]["source_sha256"], digest(self.root / owner))
        edges, _ = static_edges(self.root)
        self.assertIn({"source": owner, "target": caller, "kind": "shared-function"}, edges)

    def test_shared_function_ambiguity_retains_both_definitions(self):
        self.write("backend/bluejs/src/first.rs", "impl First { fn is_callable(&self) {} }")
        self.write("backend/bluejs/src/second.rs", "impl Second { fn is_callable(&self) {} }")
        self.write("backend/bluejs/src/caller.rs", "fn test() { receiver.is_callable(); }")
        edges, _ = static_edges(self.root)
        for owner in ("first", "second"):
            self.assertIn({"source": f"backend/bluejs/src/{owner}.rs", "target": "backend/bluejs/src/caller.rs", "kind": "shared-function"}, edges)

    def test_array_classification_keeps_heap_and_vm_definitions_conservative(self):
        heap = "backend/bluejs/src/heap/array.rs"
        owner = "backend/bluejs/src/vm/builtins/arrays.rs"
        caller = "backend/bluejs/src/vm/json.rs"
        self.write(heap, "impl Heap { fn is_array(&self, id: u64) -> bool { true } }")
        self.write(owner, "impl Vm { fn is_array(&self, value: &Value) -> bool { self.heap.is_array(1) } }")
        self.write(caller, "fn json() { vm.is_array(value); }")
        entry = next(item for item in shared_function_graph(self.root) if item["symbol"] == "is_array")
        self.assertEqual(entry["boundary"], "array-classification")
        self.assertEqual({item["source"] for item in entry["definitions"]}, {heap, owner})
        edges = shared_function_edges([entry])
        for source in (heap, owner):
            self.assertIn({"source": source, "target": caller, "kind": "shared-function"}, edges)

    def test_settlement_and_options_registry_tracks_real_owner_references(self):
        options = "backend/bluejs/src/vm/temporal/conversion/options.rs"
        promise = "backend/bluejs/src/vm/builtins/promise_core.rs"
        caller = "backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs"
        self.write(options, "fn temporal_fractional_second_digits(options: &Value) { old(); }")
        self.write(promise, "fn promise_async_from_sync_fulfill(target: u64) { old(); }")
        self.write(caller, "fn native_call() { self.promise_async_from_sync_fulfill(1); self.temporal_fractional_second_digits(value); }")
        entries = shared_function_graph(self.root)
        for symbol, source, family in [
            ("temporal_fractional_second_digits", options, "temporal-options"),
            ("promise_async_from_sync_fulfill", promise, "promise-settlement"),
        ]:
            entry = next(item for item in entries if item["symbol"] == symbol)
            self.assertEqual(entry["boundary"], family)
            self.assertEqual(entry["definitions"][0]["source"], source)
            self.assertEqual(entry["references"][0]["function"], "native_call")
        sources = {source: (self.root / source).read_text() for source in (options, promise)}
        contracts = verified_body_contracts(sources, "complete")
        self.assertEqual(set(contracts), {options, promise})
        self.assertEqual(body_contract(sources[options]), body_contract(sources[options].replace("old()", "new()")))
        self.assertNotEqual(body_contract(sources[options]), body_contract(sources[options].replace("&Value", "&String")))

    def test_shared_body_contract_uses_current_complete_evidence_and_checks_signatures(self):
        owner = "backend/bluejs/src/heap/capabilities.rs"
        caller = "backend/bluejs/src/vm/builtins/native_dispatch/dispatch.rs"
        before = "fn object_capabilities(&self, id: u64) -> bool { false }"
        self.write(owner, before)
        self.write(caller, "fn native_call() { self.object_capabilities(1); }")
        graph = empty_graph(self.root)
        graph["observed_targets"] = [key for key, value in graph["tests"].items() if value["kind"] in ("rust", "test262")]
        graph["shared_body_contracts"] = verified_body_contracts({owner: before}, "complete")
        graph["edges"].extend([
            {"source": owner, "target": "rust:test:feature", "kind": "observed", "source_sha256": digest(self.root / owner), "snapshot": "complete"},
            {"source": caller, "target": "rust:test:other", "kind": "observed"},
        ])
        self.write(owner, before.replace("false", "true"))
        result = plan(graph, [owner], self.root)
        self.assertEqual(self.selected(result), {"rust:test:feature"})
        self.assertEqual(result["shared_body_contract_sources"], [owner])
        self.write(owner, before.replace("id: u64", "id: u32"))
        self.assertIn("rust:test:other", self.selected(plan(graph, [owner], self.root)))
        self.assertIn("test262:all", self.selected(plan(graph, [owner], self.root)))
        self.write(owner, before.replace("false", "true"))
        graph["edges"][-2]["snapshot"] = "partial"
        self.assertIn("rust:test:other", self.selected(plan(graph, [owner], self.root)))

    def test_structural_contract_keeps_types_literals_attributes_and_new_helpers(self):
        before = 'const MODE: &str = "before"; struct State { index: u32 } #[inline] fn helper(x: u32) -> bool { false }'
        same = before.replace("false", 'new_body("literal"); true')
        self.assertEqual(body_contract(before), body_contract(same))
        for changed in (before.replace('"before"', '"after"'), before.replace("index: u32", "index: u64"),
                        before.replace("#[inline]", "#[cold]"), before.replace("x: u32", "x: u64"),
                        before + " fn added() {}"):
            self.assertNotEqual(body_contract(before), body_contract(changed))

    def test_cargo_metadata_includes_explicit_targets_and_example_harness(self):
        self.write("backend/bluejs/Cargo.toml", '[package]\nname = "blueice-bluejs"\nversion = "0.1.0"')
        metadata = {"packages": [{"name": "blueice-bluejs", "targets": [
            {"name": "named_target", "kind": ["test"], "src_path": str(self.root / "backend/bluejs/tests/fixtures/custom.rs"), "test": True},
            {"name": "sample", "kind": ["example"], "src_path": str(self.root / "backend/bluejs/examples/sample.rs"), "test": False}]}]}
        with patch("backend.bluejs.selftest.graph.subprocess.run", return_value=SimpleNamespace(returncode=0, stdout=json.dumps(metadata))):
            nodes = inventory(self.root)
        self.assertIn("rust:test:named_target", nodes)
        self.assertIn("rust:example:sample", nodes)

    def test_acceptance_scope_survives_without_local_state_cache(self):
        scope = {"selected_files": {"vm/builtins/feature.rs": 4}, "modified_production_files": ["vm/builtins/feature.rs"],
                 "originally_complete_files": ["lib.rs"]}
        self.write("backend/bluejs/selftest/acceptance-scope.json", json.dumps(scope))
        graph = empty_graph(self.root)
        self.assertEqual(graph["acceptance_scope"]["selected_files"], scope["selected_files"])
        self.assertEqual(graph["originally_complete"], ["lib.rs"])

    def test_cargo_configuration_changes_expand_engine(self):
        result = plan(self.graph, [".cargo/config.toml"], self.root)
        self.assertIn("test262:all", self.selected(result))
        self.assertIn("rust:test:other", self.selected(result))

    def test_snapshot_records_cargo_and_plain_toolchain_configuration(self):
        first = snapshot(self.root)["identity"]
        self.write(".cargo/config.toml", '[build]\nrustflags = ["--cfg=changed"]')
        second = snapshot(self.root)["identity"]
        self.assertNotEqual(first, second)
        self.write("rust-toolchain", "stable")
        self.assertNotEqual(second, snapshot(self.root)["identity"])


class MeasurementContracts(unittest.TestCase):
    def test_observed_relationship_refreshes_evidence_and_retains_history(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary, coverage = root / "binary", root / "coverage.json"
            binary.write_bytes(b"current executable")
            coverage.write_text('{"fresh": true}')
            graph = {"edges": [
                {"source": "source.rs", "target": "rust:test:owner", "kind": "observed", "source_sha256": "old", "evidence": "old-run"},
                {"source": "earlier-caller.rs", "target": "rust:test:owner", "kind": "observed", "source_sha256": "earlier", "evidence": "old-run"},
            ]}
            test = {"id": "rust:test:owner", "kind": "rust", "source": "test.rs", "case_filters": ["related"]}
            active = {"id": "current-run", "snapshot": "current-snapshot"}
            hashes = {"source.rs": "current", "test.rs": "test-hash"}
            refresh_observations(graph, ["source.rs"], test, active, hashes, binary, coverage, root / "profiles")
            self.assertEqual(len(graph["edges"]), 2)
            edge = graph["edges"][0]
            self.assertEqual(edge["source_sha256"], "current")
            self.assertEqual(edge["evidence"], "current-run")
            self.assertEqual(edge["snapshot"], "current-snapshot")
            self.assertEqual(edge["test_source_sha256"], "test-hash")
            self.assertEqual(edge["binary_sha256"], digest(binary))
            self.assertEqual(edge["coverage_sha256"], digest(coverage))
            self.assertEqual(edge["history"], [{"evidence": "old-run", "source_sha256": "old"}])
            self.assertEqual(graph["edges"][1]["evidence"], "old-run")
            refresh_observations(graph, ["source.rs"], test, active, hashes, binary, coverage, root / "profiles")
            self.assertEqual(len(edge["history"]), 1)
            refresh_observations(graph, ["source.rs"], test, active, hashes, binary, coverage, root / "profiles", complete_owner=True)
            self.assertEqual(len(graph["edges"]), 1)
            self.assertEqual(graph["historical_observations"][0]["source"], "earlier-caller.rs")

    def test_production_scope_keeps_raw_counters_and_new_executable_declarations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            helper = root / "backend/bluejs/coverage_file.py"
            helper.parent.mkdir(parents=True)
            helper.write_text("NO_COUNTER_REASONS = {'test.rs':'Test source; not a coverage target', 'declarations.rs':'Declaration-only source'}")
            values = {key: {"count": 3, "covered": 2} for key in ("lines", "functions", "regions")}
            payload = {"data": [{"files": [{"filename": str(root / "backend/bluejs/src" / name), "summary": values}
                                           for name in ("engine.rs", "test.rs", "declarations.rs")] }]}
            raw, totals = coverage_summary(payload, root)
            self.assertEqual(set(raw), {"engine.rs", "declarations.rs"})
            self.assertEqual(raw["engine.rs"], values)
            self.assertEqual(totals["regions"], {"count": 6, "covered": 4})

    def test_preservation_reference_never_uses_lower_percentages_or_unions(self):
        def value(covered, count):
            return {key: {"covered": covered, "count": count} for key in ("lines", "functions", "regions")}
        graph = {"baseline": {"full_coverage": {"a.rs": value(99, 100)}}}
        retain_coverage_reference(graph, {"a.rs": value(198, 200), "b.rs": value(5, 5)}, "first")
        retain_coverage_reference(graph, {"a.rs": value(98, 100), "b.rs": value(6, 7)}, "second")
        self.assertEqual(graph["coverage_reference"]["a.rs"], value(99, 100))
        self.assertEqual(graph["coverage_reference"]["b.rs"], value(5, 5))
        retain_coverage_reference(graph, {"a.rs": value(100, 100)}, "third")
        self.assertEqual(graph["coverage_reference"]["a.rs"], value(100, 100))
        self.assertEqual(graph["coverage_reference_provenance"], ["first", "second", "third"])

    def test_corpus_preflight_checks_hashes_missing_files_and_extra_cases(self):
        with tempfile.TemporaryDirectory() as directory:
            corpus = Path(directory)
            (corpus / "test").mkdir()
            case = corpus / "test/case.js"
            case.write_text("42;")
            manifest = corpus / ".bluejs-manifest.json"
            manifest.write_text(json.dumps({"test/case.js": digest(case)}))
            pinned = {"revision": "pinned", "manifest_sha256": digest(manifest)}
            (corpus / ".bluejs-snapshot.json").write_text(json.dumps(pinned))
            self.assertEqual(validate_corpus(corpus, pinned)["verified_files"], 1)
            case.write_text("7;")
            with self.assertRaisesRegex(ValueError, "test/case.js"):
                validate_corpus(corpus, pinned)
            case.unlink()
            with self.assertRaisesRegex(ValueError, "test/case.js"):
                validate_corpus(corpus, pinned)
            case.write_text("42;")
            (corpus / "test/extra.js").write_text("7;")
            with self.assertRaisesRegex(ValueError, "Untracked test files"):
                validate_corpus(corpus, pinned)
            (corpus / ".bluejs-snapshot.json").unlink()
            with self.assertRaisesRegex(ValueError, "pinned snapshot"):
                validate_corpus(corpus, pinned)

    def test_latest_baseline_uses_completed_dashboard_run_and_skips_failed_round(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for run_id, status in (("current", "passed"), ("failed", "failed")):
                folder = root / "target/bluejs-selftest/runs" / run_id
                folder.mkdir(parents=True)
                (folder / "report-data.json").write_text("{}")
                (folder / "run.json").write_text(json.dumps({"status": status, "mode": "pipeline"}))
            self.assertEqual(latest_baseline(root).name, "current")

    def test_dashboard_reindex_preserves_original_files_and_higher_comparison_thresholds(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "backend/bluejs/src/lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text("pub struct Vm;")
            scope = root / "backend/bluejs/selftest/acceptance-scope.json"
            scope.parent.mkdir(parents=True)
            scope.write_text(json.dumps({"originally_complete_files": ["lib.rs"], "modified_production_files": ["lib.rs"], "selected_files": {"lib.rs": 5}}))
            baseline = root / "target/bluejs-selftest/runs/current"
            baseline.mkdir(parents=True)
            binary = root / "retained-binary"
            binary.write_bytes(b"verified executable")
            tasks = []
            for key in ("rust:lib", "test262:all"):
                profiles = baseline / "full/profiles" / key.replace(":", "-")
                profiles.mkdir(parents=True)
                (profiles / "fresh.profraw").write_bytes(b"retained profile")
                tasks.append({"id": "full:" + key, "status": "passed", "binary": str(binary), "binary_sha256": digest(binary),
                              "profile_directory": str(profiles), "elapsed_seconds": 1})
            def save(name, value):
                path = baseline / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(value))
            raw = {key: {"count": 100, "covered": 98} for key in ("lines", "functions", "regions")}
            before = {key: {"count": 100, "covered": 99} for key in ("lines", "functions", "regions")}
            summary = {"complete_inventory": True, "scheduled_modes": 1, "results": {"pass": 1}, "adapter_sha256": digest(binary)}
            save("run.json", {"status": "passed", "mode": "pipeline", "snapshot": "verified", "tasks": tasks})
            save("source-state.json", {"identity": "verified", "files": {"backend/bluejs/src/lib.rs": digest(source)}})
            save("source-structures.json", {"backend/bluejs/src/lib.rs": []})
            save("full/bins.json", {"bluejs-test262": str(binary)})
            save("report-data.json", {"snapshot": "verified", "measured_at": "current", "rust": {"targets": 1}, "test262": summary,
                 "files": {"lib.rs": raw}, "no_counters": {}, "totals": raw, "coverage_regressions": ["lib.rs"],
                 "selected": {"lib.rs": 5}, "modified": ["lib.rs"], "coverage_reference": {"lib.rs": before}})
            payload = {"data": [{"files": [{"filename": str(source), "summary": raw}]}]}
            def export(binary, profiles, destination, *args):
                destination.mkdir(parents=True, exist_ok=True)
                (destination / "coverage.json").write_text(json.dumps(payload))
                return payload
            with patch("backend.bluejs.selftest.coverage.export_profiles", side_effect=export):
                graph = import_baseline(baseline, root / "state", root)
            self.assertEqual(graph["observed_targets"], ["rust:lib", "test262:all"])
            self.assertEqual(graph["originally_complete"], ["lib.rs"])
            self.assertEqual(graph["coverage_reference"]["lib.rs"], before)
            self.assertEqual(graph["baseline"]["full_coverage"]["lib.rs"], raw)
            save("run.json", {"status": "failed", "mode": "pipeline", "snapshot": "verified", "tasks": tasks})
            with self.assertRaisesRegex(ValueError, "verified complete run"):
                import_baseline(baseline, root / "state", root)

    def test_generated_report_preserves_classification_totals_and_combined_typed_arrays(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "backend/bluejs/src").mkdir(parents=True)
            (root / "backend/bluejs/src/a.rs").write_text("pub struct A;")
            paths = ["language/case.js", "built-ins/TypedArray/a.js", "built-ins/TypedArrayConstructors/a.js",
                     "built-ins/Temporal/root.js", "intl402/Temporal/PlainDate/a.js", "intl402/other.js"]
            values = {key: {"count": 1, "covered": 1} for key in ("lines", "functions", "regions")}
            data = {"measured_at": "current", "snapshot": "current", "files": {"a.rs": values}, "totals": values,
                    "test262": {"adapter_sha256": "current", "snapshot": {"revision": "current"}, "results": {"pass": len(paths)},
                                "scheduled_modes": len(paths), "test_files": len(paths), "elapsed_seconds": 1},
                    "outcomes": [{"path": p, "mode": "strict", "status": "pass", "actual": {}} for p in paths],
                    "selected": {"a.rs": 5}, "modified": ["a.rs"], "preserved": {"passed": 1, "total": 1},
                    "coverage_regressions": [], "rust": {"targets": 1, "counts": [1, 0, 0]},
                    "contracts": {"comparison_available": True, "compared_modes": len(paths), "contract_changes": 0}, "workspace": False}
            report = render_report(data, root, root / "report.md")
            self.assertIn("| ECMA-262 Core (`language/` + `built-ins/`) | 4 | 4 |", report)
            self.assertIn("| **All Test262 runner modes** | 6 | 6 |", report)
            self.assertIn("| `built-ins/TypedArray/` + `built-ins/TypedArrayConstructors/` | 2 | 2 |", report)
            self.assertIn("| Temporal root files | 1 | 1 |", report)
            self.assertIn("| **Total Temporal** | 2 | 2 |", report)
            self.assertIn("| **Total ECMA-402** | 2 | 2 |", report)
            self.assertIn("| Top-level Intl fixtures and locale methods on other built-ins | 1 | 1 |", report)
            self.assertIn("completion gates are not yet satisfied", report)
            data["workspace"] = True
            data["static_gates"] = {"snapshot": "older", **{key: {"status": "passed"} for key in ("rustfmt", "diff_check", "workspace_clippy")}}
            self.assertIn("completion gates are not yet satisfied", render_report(data, root, root / "report.md"))
            data["static_gates"]["snapshot"] = "current"
            self.assertIn("completion gates are satisfied", render_report(data, root, root / "report.md"))

            # A reviewed test-only source has no production counters. Unknown
            # missing executable paths must continue to block completion.
            data["modified"].append("heap/tests.rs")
            data["no_counters"] = {"heap/tests.rs": "Test source; not a coverage target"}
            audited = render_report(data, root, root / "report.md")
            self.assertIn("Modified production files complete: **1 / 1**", audited)
            self.assertIn("Test source; not a coverage target", audited)
            self.assertIn("completion gates are satisfied", audited)
            data["modified"].append("missing.rs")
            unreviewed = render_report(data, root, root / "report.md")
            self.assertIn("Modified production files complete: **1 / 2**", unreviewed)
            self.assertIn("Missing counters", unreviewed)
            self.assertIn("completion gates are not yet satisfied", unreviewed)

    def test_profile_crate_names_use_declared_lengths(self):
        symbols = "_RNvCsABC_7feature5check _RNvCsXYZ_14blueice_bluejs2vm"
        self.assertEqual(profile_owners(symbols, {"feature": "rust:test:feature", "blueice_bluejs": "rust:lib"}),
                         {"rust:test:feature", "rust:lib"})

    def test_profile_crate_name_does_not_match_function_name(self):
        self.assertEqual(profile_owners("_RNvCsABC_5other7feature", {"feature": "selected"}), set())

    def test_only_positive_covered_source_enters_graph(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def file(name, covered):
                return {"filename": str(root / name), "summary": {
                    key: {"covered": covered, "count": 2} for key in ("lines", "functions", "regions")}}
            payload = {"data": [{"files": [file("backend/bluejs/src/a.rs", 1), file("backend/bluejs/src/b.rs", 0), file("external.rs", 1)]}]}
            self.assertEqual(covered_sources(payload, root), ["backend/bluejs/src/a.rs"])

    def test_corrupt_counter_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = {"data": [{"files": [{"filename": str(root / "backend/bluejs/src/a.rs"), "summary": {
                key: {"covered": 3, "count": 2} for key in ("lines", "functions", "regions")}}]}]}
            with self.assertRaises(ValueError):
                covered_sources(payload, root)

    def test_full_gate_rejects_stale_or_changed_selection(self):
        current = {"snapshot": "fresh", "tests": [{"id": "rust:test:a"}]}
        previous = {"status": "passed", "mode": "impact", "snapshot": "fresh", "selected_ids": ["rust:test:a"]}
        validate_impact(previous, current)
        for field, value in [("snapshot", "old"), ("status", "failed"), ("selected_ids", [])]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_impact({**previous, field: value}, current)

    def test_excluded_modes_do_not_hide_a_semantic_failure(self):
        summary = {"complete_inventory": True, "scheduled_modes": 12, "results": {"pass": 10, "excluded": 1, "stale_corpus": 1}}
        self.assertTrue(test262_passed(summary, True))
        self.assertFalse(test262_passed({**summary, "results": {"pass": 9, "fail": 1, "excluded": 1, "stale_corpus": 1}}, True))
        self.assertFalse(test262_passed({"complete_inventory": True}, True))
        self.assertFalse(test262_passed({**summary, "scheduled_modes": 13}, True))
        self.assertFalse(test262_passed({**summary, "complete_inventory": False}, True))

    def test_outcome_comparison_ignores_diagnostics_and_retains_reason(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory) / "a.jsonl", Path(directory) / "b.jsonl"
            first = {"path": "a.js", "mode": "strict", "status": "pass", "sha256": "same", "actual": {"kind": "ok", "phase": "runtime", "message": "process 1"}}
            a.write_text(json.dumps(first) + "\n")
            b.write_text(json.dumps({**first, "actual": {**first["actual"], "message": "process 2"}}) + "\n")
            self.assertEqual(compare_outcomes(a, b)["contract_changes"], 0)
            b.write_text(json.dumps({**first, "elapsed_seconds": 0.25}) + "\n")
            self.assertEqual(compare_outcomes(a, b)["contract_changes"], 0)
            a.write_text(json.dumps({**first, "elapsed_seconds": 0.5}) + "\n")
            self.assertEqual(compare_outcomes(a, b)["contract_changes"], 0)
            b.write_text(json.dumps({**first, "elapsed_seconds": 0.25, "flags": ["onlyStrict"]}) + "\n")
            self.assertEqual(compare_outcomes(a, b)["contract_changes"], 1)
            b.write_text(json.dumps({**first, "actual": {"kind": "TypeError", "phase": "runtime"}}) + "\n")
            self.assertEqual(compare_outcomes(a, b)["contract_changes"], 1)
            b.write_text(json.dumps({**first, "path": "other.js"}) + "\n")
            with self.assertRaises(ValueError):
                compare_outcomes(a, b)


class ExecutionContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "backend/bluejs/src").mkdir(parents=True)
        (self.root / "backend/bluejs/src/lib.rs").write_text("pub struct Vm;")
        (self.root / "backend/bluejs/tests").mkdir()
        self.test = self.root / "backend/bluejs/tests/test_selftest.py"
        self.test.write_text("import unittest\nclass Simple(unittest.TestCase):\n def test_value(self): self.assertEqual(2+2,4)\n")
        self.manager = Manager(self.root, self.root / "state", jobs=1)
        # These execution contracts use a miniature corpus; no engine case runs.
        self.manager.preflight_corpus = lambda: None

    def tearDown(self):
        self.manager.cancel()
        if self.manager.thread:
            self.manager.thread.join(5)
        self.temp.cleanup()

    def wait(self, timeout=8):
        self.manager.thread.join(timeout)
        self.assertFalse(self.manager.thread.is_alive())
        return self.manager.active

    def retain_rust_anchor(self):
        frozen = snapshot(self.root)
        directory = self.manager.state / "runs/20261006-120000-abcdef12"
        directory.mkdir(parents=True)
        (directory / "source-state.json").write_text(json.dumps(frozen))
        (directory / "frozen-rust-sources.json").write_text(json.dumps({name: (self.root / name).read_text()
            for name in frozen["files"] if name.endswith(".rs")}))
        (directory / "run.json").write_text(json.dumps({"id": directory.name, "status": "passed", "mode": "full",
            "snapshot": frozen["identity"], "tasks": [{"id": "full:" + key, "status": "passed", "counts": [[1, 0, 0]]}
                for key, target in inventory(self.root).items() if target["kind"] == "rust"]}))

    def test_buffer_graph_selects_script_matrices_and_rejects_changed_shared_constants(self):
        typed = self.root / "backend/bluejs/src/vm/builtins/typed_arrays.rs"
        typed.parent.mkdir(parents=True)
        before = "const BUDGET: usize = 1;\nfn typed_array_slice() { old(); }"
        typed.write_text(before)
        public = self.root / "backend/bluejs/tests/buffer_matrix.rs"
        public.write_text('const SETUP: &str = "new Uint8Array(new ArrayBuffer(8))";\n#[test] fn buffer_contract() {run(SETUP);}')
        unrelated = self.root / "backend/bluejs/tests/unrelated.rs"
        unrelated.write_text("#[test] fn math() {}")
        self.retain_rust_anchor()
        typed.write_text(before.replace("old();", "corrected();"))
        selection = partition_plan(self.manager)
        self.assertEqual({test["id"] for test in selection["tests"]},
                         {"rust:lib", "rust:test:buffer_matrix", "test262:partition:buffer-contracts"})
        self.assertEqual(selection["unknown_sources"], [])
        conformance = next(test for test in selection["tests"] if test["kind"] == "test262")
        self.assertIn("built-ins/DataView/", conformance["filter"])
        self.assertIn("built-ins/TypedArrayConstructors/", conformance["filter"])
        typed.write_text(typed.read_text().replace("BUDGET: usize = 1", "BUDGET: usize = 2"))
        selection = partition_plan(self.manager)
        self.assertIn(typed.relative_to(self.root).as_posix(), selection["unknown_sources"])
        self.assertIn("rust:test:unrelated", {test["id"] for test in selection["tests"]})

    def test_changed_fixture_selects_its_ordinary_library_entry_case(self):
        fixture = self.root / "backend/bluejs/tests/fixtures/boundary.rs"
        fixture.parent.mkdir(parents=True)
        fixture.write_text("#[cfg_attr(test, test)] fn actual_boundary() {old();}\npub fn verify_boundary_contracts() {actual_boundary();}")
        public = self.root / "backend/bluejs/tests/ordinary_library.rs"
        public.write_text('#[test] fn boundary_case() {Vm::verify_boundary_contracts();}\n'
                          '#[test] fn unrelated_case() {run("verify_boundary_contracts()");}')
        self.retain_rust_anchor()
        fixture.write_text(fixture.read_text().replace("old();", "corrected();"))
        selection = partition_plan(self.manager)
        self.assertEqual(selection["unknown_sources"], [])
        self.assertEqual({test["id"] for test in selection["tests"]}, {"rust:lib", "rust:test:ordinary_library"})
        ordinary = next(test for test in selection["tests"] if test["id"] == "rust:test:ordinary_library")
        self.assertEqual(ordinary["case_filters"], ["boundary_case"])

    def test_registered_fixtures_select_both_builds_when_new_helpers_change(self):
        ordinary = self.root / "backend/bluejs/tests/ordinary_library.rs"
        ordinary.write_text("#[test] fn unrelated_case() {}")
        self.retain_rust_anchor()
        for name, wrapper, prefix in [
            ("common_boundary_contracts", "verify_common_boundary_contracts", "vm::classification::boundary_contracts::"),
            ("payload_accounting_contracts", "verify_payload_accounting_contracts", "ast::retained_payload::accounting_contracts::"),
            ("independent_boundary_contracts", "verify_independent_boundary_contracts", "vm::tests::independent_boundary_contracts::"),
        ]:
            with self.subTest(fixture=name):
                ordinary.write_text(f"#[test] fn boundary_case() {{Vm::{wrapper}();}}\n"
                                    "#[test] fn unrelated_case() {}")
                fixture = self.root / f"backend/bluejs/tests/fixtures/{name}.rs"
                fixture.parent.mkdir(parents=True, exist_ok=True)
                fixture.write_text("fn shared_setup() { install_real_host(); }\n"
                                   "#[cfg_attr(test, test)] fn actual_boundary() {shared_setup();}\n"
                                   f"pub fn {wrapper}() {{actual_boundary();}}")
                selection = partition_plan(self.manager, [str(fixture)])
                self.assertEqual(selection["unknown_sources"], [])
                self.assertEqual({t["id"] for t in selection["tests"]}, {"rust:lib", "rust:test:ordinary_library"})
                library = next(t for t in selection["tests"] if t["id"] == "rust:lib")
                self.assertEqual(library["case_filters"], [prefix])
                public = next(t for t in selection["tests"] if t["id"] == "rust:test:ordinary_library")
                self.assertEqual(public["case_filters"], ["boundary_case"])
                fixture.write_text(fixture.read_text().replace("install_real_host", "protect_vm_roots"))
                self.assertEqual(partition_plan(self.manager, [str(fixture)])["unknown_sources"], [])

    def test_registered_common_fixture_without_expected_wrapper_keeps_broad_fallback(self):
        self.retain_rust_anchor()
        fixture = self.root / "backend/bluejs/tests/fixtures/common_boundary_contracts.rs"
        fixture.parent.mkdir(parents=True)
        fixture.write_text("#[cfg_attr(test, test)] fn actual_boundary() {}")
        selection = partition_plan(self.manager, [str(fixture)])
        self.assertEqual(selection["unknown_sources"], ["backend/bluejs/tests/fixtures/common_boundary_contracts.rs"])


    def test_regex_pool_graph_selects_lifecycle_consumers_and_ordinary_fixtures(self):
        worker = self.root / "backend/bluejs/src/regex_worker.rs"
        before = 'const MEMO_UNITS: usize = 1024;\nfn remember() {old();}\nfn with_worker() {old();}\n'
        worker.write_text(before)
        for name in ("regex_worker_reuse", "regex_deadlines", "regexp_observable_ordering", "unrelated"):
            (self.root / f"backend/bluejs/tests/{name}.rs").write_text("#[test] fn contract() {}")
        ordinary = self.root / "backend/bluejs/tests/ordinary_library.rs"
        ordinary.write_text("#[test] fn lifecycle() {regex_worker::verify_regex_pool_boundary_contracts();}\n"
                            "#[test] fn other() {Vm::verify_other_contracts();}")
        self.retain_rust_anchor()
        worker.write_text(before.replace("old();", "corrected();") +
            '/// Retire outside the lock.\nfn recycle_worker() {}\n'
            '#[cfg(any(test, coverage))]\n#[path = "../tests/fixtures/regex_pool_boundaries.rs"]\n'
            'mod pool_boundary_contracts;\n#[cfg(coverage)]\n#[doc(hidden)]\n'
            'pub use pool_boundary_contracts::verify_regex_pool_boundary_contracts;\n')
        selection = partition_plan(self.manager)
        self.assertEqual(selection["unknown_sources"], [])
        self.assertEqual({test["id"] for test in selection["tests"]}, {
            "rust:lib", "rust:test:regex_worker_reuse", "rust:test:regex_deadlines",
            "rust:test:regexp_observable_ordering", "rust:test:ordinary_library", "test262:partition:regex-pool"})
        library = next(test for test in selection["tests"] if test["id"] == "rust:lib")
        self.assertEqual(library["case_filters"], ["regex_worker::"])
        public = next(test for test in selection["tests"] if test["id"] == "rust:test:ordinary_library")
        self.assertEqual(public["case_filters"], ["lifecycle"])
        conformance = next(test for test in selection["tests"] if test["kind"] == "test262")
        self.assertIn("built-ins/RegExp/", conformance["case_filters"])
        self.assertIn("built-ins/String/prototype/split/", conformance["case_filters"])

    def test_regex_pool_contract_refuses_changed_constants_imports_and_other_bodies(self):
        worker = self.root / "backend/bluejs/src/regex_worker.rs"
        before = 'const MEMO_UNITS: usize = 1024;\nconst VERSION: &str = "old string";\nfn remember() {old();}\nfn find() {old();}\n'
        worker.write_text(before)
        (self.root / "backend/bluejs/tests/unrelated.rs").write_text("#[test] fn contract() {}")
        self.retain_rust_anchor()
        for after in (before.replace("1024", "2048"), before.replace("old string", "changed string"), "use new_dependency::*;\n" + before,
                      before.replace("fn find() {old();}", "fn find() {changed();}")):
            worker.write_text(after)
            selection = partition_plan(self.manager)
            self.assertIn("backend/bluejs/src/regex_worker.rs", selection["unknown_sources"])
            self.assertIn("rust:test:unrelated", {test["id"] for test in selection["tests"]})

    def test_fixture_selection_follows_only_relevant_public_wrapper_roots(self):
        fixture = self.root / "backend/bluejs/tests/fixtures/boundary.rs"
        fixture.parent.mkdir(parents=True)
        fixture.write_text("#[cfg_attr(test, test)] fn actual_boundary() {old();}\n"
                           "fn helper() {actual_boundary();}\n"
                           "pub fn verify_actual_contracts() {helper();}\n"
                           'pub fn verify_unrelated_contracts() {run("actual_boundary()");}')
        public = self.root / "backend/bluejs/tests/ordinary_library.rs"
        public.write_text("#[test] fn actual_case() {Vm::verify_actual_contracts();}\n"
                          "#[test] fn unrelated_case() {Vm::verify_unrelated_contracts();}")
        self.retain_rust_anchor()
        fixture.write_text(fixture.read_text().replace("old();", "corrected();"))
        selection = partition_plan(self.manager)
        self.assertEqual(selection["unknown_sources"], [])
        ordinary = next(test for test in selection["tests"] if test["id"] == "rust:test:ordinary_library")
        self.assertEqual(ordinary["case_filters"], ["actual_case"])

    def test_changed_public_cases_are_filtered_until_a_shared_input_changes(self):
        public = self.root / "backend/bluejs/tests/ordinary_library.rs"
        before = ('const SHARED: &str = "old shared value";\n'
                  '#[cfg(coverage)]\n#[test]\nfn unchanged() {run(SHARED);}\n')
        public.write_text(before)
        self.retain_rust_anchor()
        public.write_text(before + '#[cfg(coverage)]\n#[test]\nfn added_case() {run("new case");}\n')
        selection = partition_plan(self.manager)
        ordinary = next(test for test in selection["tests"] if test["id"] == "rust:test:ordinary_library")
        self.assertEqual(ordinary["case_filters"], ["added_case"])
        public.write_text('const SHARED: &str = "old shared value";\n'
                          '#[cfg(coverage)]\n#[test]\nfn added_case() {run("new case");}\n')
        selection = partition_plan(self.manager)
        ordinary = next(test for test in selection["tests"] if test["id"] == "rust:test:ordinary_library")
        self.assertEqual(ordinary["case_filters"], [""])
        public.write_text(before + '#[cfg(coverage)]\n#[test]\nfn added_case() {run("new case");}\n')
        public.write_text(public.read_text().replace("old shared value", "changed shared value"))
        selection = partition_plan(self.manager)
        ordinary = next(test for test in selection["tests"] if test["id"] == "rust:test:ordinary_library")
        self.assertEqual(ordinary["case_filters"], [""])

    def test_deletion_graph_selects_all_public_delete_contracts_and_combines_conformance(self):
        objects = self.root / "backend/bluejs/src/vm/builtins/object.rs"
        objects.parent.mkdir(parents=True)
        objects.write_text("fn object_delete() { old(); }")
        execution = self.root / "backend/bluejs/src/vm/execution.rs"
        execution.write_text("fn delete_unbound_name() { old(); }")
        public = self.root / "backend/bluejs/tests/deletion.rs"
        public.write_text('const CASE: &str = "Reflect.deleteProperty(globalThis, \'gone\')";\n#[test] fn deletion() {run(CASE);}')
        unrelated = self.root / "backend/bluejs/tests/unrelated.rs"
        unrelated.write_text("#[test] fn math() {}")
        self.retain_rust_anchor()
        for path in (objects, execution):
            path.write_text(path.read_text().replace("old();", "corrected();"))
        selection = partition_plan(self.manager)
        self.assertEqual(selection["unknown_sources"], [])
        self.assertEqual({test["id"] for test in selection["tests"]},
                         {"rust:lib", "rust:test:deletion", "test262:partition:global-deletion"})
        conformance = [test for test in selection["tests"] if test["kind"] == "test262"]
        self.assertEqual(len(conformance), 1)
        self.assertIn("language/expressions/delete/", conformance[0]["filter"])
        self.assertIn("language/eval-code/", conformance[0]["filter"])
        objects.write_text("use changed_import;\n" + objects.read_text())
        selection = partition_plan(self.manager)
        self.assertIn(objects.relative_to(self.root).as_posix(), selection["unknown_sources"])
        self.assertIn("rust:test:unrelated", {test["id"] for test in selection["tests"]})

    def test_native_discovery_uses_bounded_parallel_listing_and_stable_selection(self):
        directory = self.manager.state / "runs/20261006-120000-abcdef12"
        (directory / "partition").mkdir(parents=True)
        self.manager.active = {"id": directory.name, "tasks": [], "message": ""}
        self.manager.jobs = 2
        barrier, calls = threading.Barrier(2), []
        tests = [{"id": "rust:test:" + name, "kind": "rust", "label": name, "case_filters": ["actual"]}
                 for name in ("first", "second")]
        conformance = {"id": "test262:all", "kind": "test262"}

        def command(key, label, args, env, stage):
            self.assertEqual(args[1:], ["--list", "--format", "terse"])
            self.assertTrue(env["LLVM_PROFILE_FILE"].startswith(str(directory / "partition/discovery")))
            barrier.wait(timeout=2)
            calls.append(key)
            task = next(t for t in self.manager.active["tasks"] if t["id"] == key)
            log = directory / task["log"]
            log.parent.mkdir(exist_ok=True)
            log.write_text("scope::actual: test\nscope::unrelated: test\n")
            task["status"] = "passed"
            return True

        self.manager.command = command
        result = self.manager.discover_partitions(tests + [conformance], {t["id"]: t["label"] for t in tests}, {}, directory)
        self.assertEqual(len(calls), 2)
        self.assertEqual([t["target_id"] for t in result[:-1]], [t["id"] for t in tests])
        self.assertTrue(all(t["case_names"] == ["scope::actual"] for t in result[:-1]))
        self.assertEqual(result[-1], conformance)
        self.manager.command = lambda *args: False
        with self.assertRaisesRegex(RuntimeError, "no selected case has started"):
            self.manager.discover_partitions(tests, {t["id"]: t["label"] for t in tests}, {}, directory)

    def test_real_selected_run_retains_log_and_source_identity(self):
        run = self.manager.start("impact", ["backend/bluejs/tests/test_selftest.py"])
        finished = self.wait()
        self.assertEqual(finished["status"], "passed")
        self.assertEqual(finished["selected_ids"], ["python:selftest"])
        task = finished["tasks"][0]
        self.assertIn("OK", (self.manager.state / "runs" / run["id"] / task["log"]).read_text())
        self.assertEqual(finished["snapshot"], snapshot(self.root)["identity"])

    def test_failed_impact_never_starts_full_build(self):
        self.test.write_text("import unittest\nclass Simple(unittest.TestCase):\n def test_value(self): self.fail('regression')\n")
        self.manager.environment = lambda: self.fail("Full environment started before affected tests passed")
        self.manager.start("pipeline", ["backend/bluejs/tests/test_selftest.py"])
        finished = self.wait()
        self.assertEqual(finished["status"], "failed")
        self.assertTrue(all(task["stage"] == "impact" for task in finished["tasks"]))

    def test_rust_failure_skips_later_targets_and_conformance_even_when_input_is_reversed(self):
        run_id = "20261006-120000-abcdef12"
        directory = self.manager.state / "runs" / run_id
        directory.mkdir(parents=True)
        self.manager.active = {"id": run_id, "status": "running", "tasks": [], "message": ""}
        failed_binary = self.root / "failed-rust"
        failed_binary.write_text("#!/bin/sh\necho 'test result: FAILED. 0 passed; 1 failed; 0 ignored'\nexit 1\n")
        failed_binary.chmod(0o755)
        forbidden = self.root / "must-not-run"
        next_binary = self.root / "next-rust"
        next_binary.write_text("#!/bin/sh\ntouch must-not-run\necho 'test result: ok. 1 passed; 0 failed; 0 ignored'\n")
        next_binary.chmod(0o755)
        tests = [
            {"id": "test262:all", "label": "conformance", "kind": "test262"},
            {"id": "rust:test:other", "label": "other", "kind": "rust"},
            {"id": "rust:lib", "label": "critical", "kind": "rust"},
        ]
        self.assertFalse(self.manager.run_tests(tests, "impact", artifacts={
            "rust:lib": str(failed_binary), "rust:test:other": str(next_binary)}, bins={}))
        statuses = {task["id"]: task["status"] for task in self.manager.active["tasks"]}
        self.assertEqual(statuses["impact:rust:lib"], "failed")
        self.assertEqual(statuses["impact:rust:test:other"], "skipped")
        self.assertEqual(statuses["impact:test262:all"], "skipped")
        self.assertFalse(forbidden.exists())

    def test_full_run_requires_matching_passed_impact(self):
        with self.assertRaises(ValueError):
            self.manager.start("full", ["backend/bluejs/tests/test_selftest.py"])

    def test_partition_filters_deduplicate_cases_and_group_native_names(self):
        target = {"id": "rust:lib", "kind": "rust", "label": "unit", "case_filters": ["eval", "capture"]}
        listing = "vm::scope::eval_capture: test\nvm::scope::eval_capture: test\nvm::scope::eval_local: test\nvm::math::unrelated: test\n"
        groups = case_partitions(target, listing)
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]["case_names"], ["vm::scope::eval_capture", "vm::scope::eval_local"])
        self.assertEqual(groups[0]["target_id"], "rust:lib")
        with self.assertRaises(ValueError):
            case_partitions({**target, "case_filters": ["missing"]}, listing)

    def test_reference_graph_selects_changed_boundaries_since_a_verified_rust_anchor(self):
        operations = self.root / "backend/bluejs/src/vm/operations.rs"
        operations.parent.mkdir(parents=True)
        operations.write_text("fn load_with_reference() { old(); }")
        public = self.root / "backend/bluejs/tests/eval_var_environment_capture.rs"
        public.write_text("#[test] fn captures() {}")
        unrelated = self.root / "backend/bluejs/tests/unrelated.rs"
        unrelated.write_text("#[test] fn math() {}")
        operators = self.root / "backend/bluejs/tests/neutral_coverage.rs"
        operators.write_text('#[test] fn delayed_assignment() { run(r#"with ({}) { missing = eval(\'var missing = 1\') }"#); }\n'
                             '#[test] fn unrelated_numbers() { run("1 + 2"); }')
        frozen = snapshot(self.root)
        anchor = self.manager.state / "runs/20261006-120000-abcdef12"
        anchor.mkdir(parents=True)
        (anchor / "source-state.json").write_text(json.dumps(frozen))
        (anchor / "frozen-rust-sources.json").write_text(json.dumps({name: (self.root / name).read_text()
            for name in frozen["files"] if name.endswith(".rs")}))
        targets = inventory(self.root)
        (anchor / "run.json").write_text(json.dumps({"id": anchor.name, "status": "failed", "mode": "pipeline",
            "snapshot": frozen["identity"], "tasks": [{"id": "impact:" + key, "status": "passed", "counts": [[1, 0, 0]]}
                for key, target in targets.items() if target["kind"] == "rust"]}))
        operations.write_text("fn load_with_reference() { corrected(); }")
        result = partition_plan(self.manager)
        self.assertEqual(result["anchor"], anchor.name)
        self.assertEqual({t["id"] for t in result["tests"]}, {"rust:lib", "rust:test:eval_var_environment_capture", "rust:test:neutral_coverage",
                                                             "test262:partition:reference-resolution"})
        selected = next(t for t in result["tests"] if t["id"] == "rust:test:neutral_coverage")
        self.assertEqual(selected["case_filters"], ["delayed_assignment"])
        self.assertEqual(result["unknown_sources"], [])
        self.assertTrue(result["full_verification_required"])
        # UI history is bounded; the verified source anchor survives many
        # correction attempts without requiring another complete Rust run.
        for index in range(31):
            newer = self.manager.state / "runs" / f"20261006-1300{index:02}-abcdef12"
            newer.mkdir()
            (newer / "run.json").write_text(json.dumps({"id": newer.name, "status": "failed", "mode": "partition", "tasks": []}))
        self.assertEqual(len(self.manager.history()), 30)
        self.assertEqual(latest_rust_anchor(self.manager)[0], anchor.name)
        operations.write_text("fn unrelated_new_boundary() { changed(); }")
        self.assertIn(operations.relative_to(self.root).as_posix(), partition_plan(self.manager)["unknown_sources"])

    def test_tool_only_partitions_run_contracts_without_building_or_starting_rust(self):
        self.manager.environment = lambda: self.fail("Tool changes started a Rust environment")
        with patch.object(self.manager, "build", side_effect=AssertionError("Tool changes started Cargo")), \
             patch.object(self.manager, "worker_ready", side_effect=AssertionError("Tool changes started the worker")), \
             patch.object(self.manager, "learn", side_effect=AssertionError("Tool changes exported LLVM")):
            self.manager.start("partition", ["backend/bluejs/tests/test_selftest.py"])
            result = self.wait()
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["selected_ids"], ["python:selftest"])
        self.assertEqual(len(result["tasks"]), 1)
        self.assertEqual(result["tasks"][0]["id"], "partition:python:selftest")

    def test_partition_tool_failure_stops_before_the_selected_rust_build(self):
        self.test.write_text("import unittest\nclass Simple(unittest.TestCase):\n def test_value(self): self.fail('regression')\n")
        targets = inventory(self.root)
        selected = {"snapshot": snapshot(self.root)["identity"], "changed_files": [],
                    "tests": [targets["python:selftest"], {**targets["rust:lib"], "case_filters": [""]}]}
        self.manager.environment = lambda: {}
        with patch("backend.bluejs.selftest.partitions.partition_plan", return_value=selected), \
             patch.object(self.manager, "build", side_effect=AssertionError("Rust built before tools passed")):
            self.manager.start("partition", [])
            result = self.wait()
        self.assertEqual(result["status"], "failed")
        self.assertIn("no Rust partition build has started", result["message"])
        self.assertEqual([task["id"] for task in result["tasks"]], ["partition:python:selftest"])

    def test_partition_planner_includes_external_rust_and_build_dependencies(self):
        for name in ("backend/ecma402/src/lib.rs", "Cargo.lock"):
            selection = partition_plan(self.manager, [name])
            self.assertEqual({t["id"] for t in selection["tests"] if t["kind"] == "rust"},
                             {key for key, t in inventory(self.root).items() if t["kind"] == "rust"})

    def test_partition_gate_requires_identical_case_filters_and_test262_modes(self):
        directory = self.manager.state / "runs/20261006-120000-abcdef12"
        directory.mkdir(parents=True)
        selection = {"snapshot": "same-source", "tests": [
            {"id": "rust:lib", "kind": "rust", "case_filters": ["first"]},
            {"id": "test262:related", "kind": "test262", "filter": "first.js", "required_modes": [["first.js", "strict"]]}]}
        record = {"id": directory.name, "mode": "partition", "status": "passed", "snapshot": "same-source", "tasks": []}
        (directory / "run.json").write_text(json.dumps(record))
        (directory / "plan.json").write_text(json.dumps(selection))
        self.assertEqual(passed_partition(self.manager, selection)["id"], directory.name)
        changed = json.loads(json.dumps(selection))
        changed["tests"][0]["case_filters"] = ["second"]
        self.assertIsNone(passed_partition(self.manager, changed))
        changed = json.loads(json.dumps(selection))
        changed["tests"][1]["required_modes"].append(["first.js", "sloppy"])
        self.assertIsNone(passed_partition(self.manager, changed))
        (directory / "plan.json").unlink()
        self.assertIsNone(passed_partition(self.manager, selection))

    def test_source_edit_during_planning_rejects_the_run_before_starting_tests(self):
        selected = {"snapshot": snapshot(self.root)["identity"], "tests": [], "changed_files": []}
        (self.root / "backend/bluejs/src/lib.rs").write_text("pub struct Changed;")
        with patch("backend.bluejs.selftest.partitions.partition_plan", return_value=selected):
            with self.assertRaisesRegex(ValueError, "Sources changed while selecting tests"):
                self.manager.start("partition", [])
        self.assertIsNone(self.manager.active)
        self.assertIsNone(self.manager.thread)

    def test_public_reference_cases_inspect_real_test_bodies_and_skip_literal_declarations(self):
        source = '''
        const FAKE: &str = r##"#[test] fn pretend() { with({}); }"##;
        /* #[test] fn commented() { with({}); } */
        fn helper() { run("with ({}) { x }"); }
        #[test]
        fn assignment_order() { run(r#"with ({}) { x = eval('var x = 1') }"#); }
        #[test] #[ignore]
        fn global_binding_contract() { run("with ({}) { globalThis.x }"); }
        #[test] fn temporal_with_method() { run("date.with({year: 2000})"); }
        #[test] fn ordinary_math() { run("1 + 2"); }
        '''
        self.assertEqual(public_reference_cases(source), ["assignment_order", "global_binding_contract"])

    def test_rust_scope_parser_ignores_literals_and_nested_comments(self):
        first = 'fn run() {let text = r##"fn pretend() { Opcode::Fake => { } }"##; /* outer /* nested {} */ end */ match op { Opcode::ResolveWithReference => { old(); } }}'
        second = first.replace("old();", "corrected();")
        self.assertEqual(changed_symbols(first, second), ["run"])
        self.assertEqual(changed_opcodes(first, second), ["ResolveWithReference"])

    def test_native_partition_executes_each_selected_case_once_and_keeps_profiles_separate(self):
        run_id = "20261006-120000-abcdef12"
        directory = self.manager.state / "runs" / run_id
        directory.mkdir(parents=True)
        self.manager.active = {"id": run_id, "status": "running", "tasks": [], "message": ""}
        binary = self.root / "native-partition"
        binary.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\"\necho 'test result: ok. 2 passed; 0 failed; 0 ignored'\n")
        binary.chmod(0o755)
        case = {"id": "rust:lib:partition:vm::scope", "target_id": "rust:lib", "case": "vm::scope", "partition": True,
                "case_names": ["vm::scope::first", "vm::scope::second"], "kind": "rust", "label": "scope"}
        self.assertTrue(self.manager.run_tests([case], "partition", artifacts={"rust:lib": str(binary)}))
        task = self.manager.active["tasks"][0]
        self.assertIn("--exact\nvm::scope::first\nvm::scope::second", (directory / task["log"]).read_text())
        self.assertIn("/partition/profiles/", task["profile_directory"])
        self.assertFalse((directory / "full/profiles").exists())
        self.assertNotIn(case["id"], self.manager.graph()["tests"])
        binary.write_text("#!/bin/sh\necho 'test result: ok. 1 passed; 0 failed; 0 ignored'\n")
        self.assertFalse(self.manager.run_tests([case], "partition", artifacts={"rust:lib": str(binary)}))
        self.assertEqual(task["status"], "failed")

    def test_passed_partitions_start_only_one_full_stage_and_source_edits_invalidate_the_gate(self):
        identity = snapshot(self.root)["identity"]
        previous = self.manager.state / "runs/20261006-120000-abcdef12"
        previous.mkdir(parents=True)
        (previous / "run.json").write_text(json.dumps({"id": previous.name, "mode": "partition", "status": "passed",
            "snapshot": identity, "selected_ids": ["rust:lib"], "tasks": []}))
        target = inventory(self.root)["rust:lib"]
        selection = {"snapshot": identity, "tests": [target], "changed_files": ["backend/bluejs/src/lib.rs"]}
        (previous / "plan.json").write_text(json.dumps(selection))
        def report(manager, *_):
            directory = manager.state / "runs" / manager.active["id"]
            data = {"files": {}, "no_counters": {}, "totals": {}, "rust": {"counts": [1, 0, 0], "targets": 1},
                    "test262": {}, "coverage_regressions": [], "measured_at": "2026-10-06"}
            (directory / "report-data.json").write_text(json.dumps(data))
            return directory / "report.md"
        self.manager.environment = lambda: {}
        with patch("backend.bluejs.selftest.partitions.partition_plan", return_value=selection), \
             patch.object(self.manager, "build", return_value=({}, {})) as build, \
             patch.object(self.manager, "worker_ready", return_value=True), \
             patch.object(self.manager, "run_tests", return_value=True) as run_tests, \
             patch.object(self.manager, "learn"), \
             patch("backend.bluejs.selftest.report.full_report", side_effect=report):
            self.manager.start("pipeline", ["backend/bluejs/src/lib.rs"])
            result = self.wait()
            self.assertEqual(result["status"], "passed")
            self.assertEqual(result["partition_gate"], previous.name)
            frozen_path = self.manager.state / "runs" / result["id"] / "frozen-rust-sources.json"
            self.assertEqual(json.loads(frozen_path.read_text())["backend/bluejs/src/lib.rs"], "pub struct Vm;")
            self.assertEqual([call.args[2] for call in build.call_args_list], ["full"])
            self.assertEqual([call.args[1] for call in run_tests.call_args_list], ["full"])
            (self.root / "backend/bluejs/src/lib.rs").write_text("pub struct Changed;")
            with self.assertRaises(ValueError):
                self.manager.start("full", ["backend/bluejs/src/lib.rs"])

    def test_same_snapshot_partition_retires_only_observed_exact_cases(self):
        target = {"id": "rust:lib", "kind": "rust", "label": "unit"}
        failed = self.manager.state / "runs/20261006-120000-abcdef12"
        failed.mkdir(parents=True)
        (failed / "case.log").write_text("test vm::one ... FAILED\ntest vm::two ... FAILED\n")
        (failed / "run.json").write_text(json.dumps({"id": failed.name, "mode": "pipeline", "status": "failed",
            "tasks": [{"id": "impact:rust:lib", "status": "failed", "log": "case.log"}]}))
        passed = self.manager.state / "runs/20261006-120050-abcdef13"
        passed.mkdir()
        binary = self.root / "current-unit-artifact"
        binary.write_bytes(b"retained native artifact identity")
        task = {"id": "partition:rust:lib:case:vm::one", "target_id": "rust:lib", "case": "vm::one",
                "status": "passed", "binary": str(binary), "binary_sha256": digest(binary),
                "counts": [[1, 0, 0]], "command": [str(binary), "--test-threads=8", "--exact", "vm::one"], "log": "case.log"}
        row = {"id": passed.name, "mode": "partition", "status": "passed", "snapshot": "same-source", "tasks": [task]}
        (passed / "case.log").write_text("test vm::one ... ok\n")
        self.manager.active = {"id": "20261006-120100-abcdef14", "snapshot": "same-source"}
        invalid = [
            ("source", {"snapshot": "older-source"}, {}),
            ("hash", {}, {"binary_sha256": "different"}),
            ("zero", {}, {"counts": [[0, 0, 0]]}),
            ("ignored", {}, {"counts": [[0, 0, 1]]}),
            ("failed", {}, {"status": "failed"}),
            ("wrong-selection", {}, {"command": [str(binary), "--exact", "vm::two"]}),
            ("not-exact", {}, {"command": [str(binary), "vm::one"]}),
            ("missing-log", {}, {"log": "missing.log"}),
            ("missing-binary", {}, {"binary": str(self.root / "absent")}),
            ("unexpected-count", {}, {"counts": [[2, 0, 0]]}),
        ]
        for label, changes, task_changes in invalid:
            with self.subTest(label=label):
                (passed / "run.json").write_text(json.dumps({**row, **changes, "tasks": [{**task, **task_changes}]}))
                self.assertEqual({t["case"] for t in self.manager.prior_failed_cases([target])}, {"vm::one", "vm::two"})
        (passed / "run.json").write_text(json.dumps(row))
        self.assertEqual([t["case"] for t in self.manager.prior_failed_cases([target])], ["vm::two"])
        # Native selection identity also guards against an artifact replaced
        # after a partition passed, even when its path is unchanged.
        binary.write_bytes(b"rebuilt native artifact")
        self.assertEqual({t["case"] for t in self.manager.prior_failed_cases([target])}, {"vm::one", "vm::two"})

    def test_cancelled_round_keeps_known_failure_cases_for_the_next_partition_run(self):
        target = {"id": "rust:lib", "kind": "rust", "label": "unit"}
        directory = self.manager.state / "runs/20261006-120000-abcdef12"
        directory.mkdir(parents=True)
        (directory / "case.log").write_text("test vm::scope::failure ... FAILED\n")
        (directory / "run.json").write_text(json.dumps({"id": directory.name, "status": "cancelled", "mode": "pipeline",
            "tasks": [{"id": "impact:rust:lib", "status": "failed", "log": "case.log"}]}))
        self.manager.active = {"id": "20261006-120100-abcdef13"}
        case, = self.manager.prior_failed_cases([target])
        self.assertEqual(case["case"], "vm::scope::failure")

    def test_failure_cases_are_selected_from_the_latest_failed_relevant_run(self):
        target = {"id": "rust:lib", "kind": "rust", "label": "library", "cargo_args": ["--lib"]}
        previous = self.manager.state / "runs/20261006-120000-abcdef12"
        previous.mkdir(parents=True)
        (previous / "case.log").write_text("test vm::boundary ... FAILED\ntest vm::other ... ok\n")
        (previous / "run.json").write_text(json.dumps({"id": previous.name, "mode": "pipeline", "status": "failed",
            "tasks": [{"id": "impact:rust:lib", "status": "failed", "log": "case.log"}]}))
        self.manager.active = {"id": "20261006-120100-abcdef13"}
        cases = self.manager.prior_failed_cases([target])
        self.assertEqual([(t["target_id"], t["case"]) for t in cases], [("rust:lib", "vm::boundary")])
        self.assertEqual(self.manager.prior_failed_cases([{**target, "id": "rust:test:unrelated"}]), [])
        successful = self.manager.state / "runs/20261006-120050-abcdef14"
        successful.mkdir()
        (successful / "run.json").write_text(json.dumps({"id": successful.name, "mode": "pipeline", "status": "passed", "tasks": []}))
        self.assertEqual(self.manager.prior_failed_cases([target]), [])

    def test_failure_recheck_uses_exact_filter_and_keeps_partial_profiles_separate(self):
        run_id = "20261006-120000-abcdef12"
        directory = self.manager.state / "runs" / run_id
        directory.mkdir(parents=True)
        self.manager.active = {"id": run_id, "status": "running", "tasks": [], "message": ""}
        binary = self.root / "fake-case"
        binary.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\"\necho 'test result: ok. 1 passed; 0 failed; 0 ignored'\n")
        binary.chmod(0o755)
        case = {"id": "rust:lib:case:vm::boundary", "target_id": "rust:lib", "case": "vm::boundary", "label": "case", "kind": "rust"}
        self.assertTrue(self.manager.run_tests([case], "recheck", artifacts={"rust:lib": str(binary)}))
        task = self.manager.active["tasks"][0]
        self.assertIn("--exact\nvm::boundary", (directory / task["log"]).read_text())
        self.assertIn("/recheck/profiles/", task["profile_directory"])
        self.assertFalse((directory / "full/profiles").exists())
        self.assertNotIn(case["id"], self.manager.graph()["tests"])
        self.assertIn(case["id"], self.manager.graph()["case_history"])

    def test_test262_failures_recheck_only_retained_paths_and_require_the_failed_modes(self):
        previous = self.manager.state / "runs/20261006-120000-abcdef12"
        output = previous / "impact/test262"
        output.mkdir(parents=True)
        rows = [
            {"path": "language/a.js", "mode": "sloppy", "status": "pass"},
            {"path": "language/a.js", "mode": "strict", "status": "fail"},
            {"path": "language/b.js", "mode": "sloppy", "status": "timeout"},
            {"path": "language/c.js", "mode": "strict", "status": "excluded"},
        ]
        (output / "results.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))
        (previous / "run.json").write_text(json.dumps({"id": previous.name, "mode": "pipeline", "status": "failed",
            "tasks": [{"id": "impact:test262:all", "stage": "impact", "status": "failed", "log": "case.log"}]}))
        self.manager.active = {"id": "20261006-120100-abcdef13"}
        target = {"id": "test262:all", "kind": "test262", "label": "conformance"}
        case, = self.manager.prior_failed_cases([target])
        self.assertEqual(case["filter"], "language/a.js,language/b.js")
        self.assertEqual(case["required_modes"], [("language/a.js", "strict"), ("language/b.js", "sloppy")])
        with patch.object(self.manager, "build", return_value=({}, {"bluejs-regexp-worker": "worker"})) as build, \
             patch.object(self.manager, "worker_ready", return_value=True), \
             patch.object(self.manager, "run_tests", return_value=True) as run_tests:
            self.manager.active.update(tasks=[], status="running", message="")
            (self.manager.state / "runs" / self.manager.active["id"]).mkdir()
            self.manager.recheck_prior_failures([target], {})
            build.assert_called_once_with([target], {}, "recheck")
            self.assertEqual(run_tests.call_args.args[0][0]["required_modes"], case["required_modes"])
        # A related fixture set is an alias of the same adapter inventory.
        # Retained failures must still be rechecked before its affected cases.
        alias = {**target, "id": "test262:partition:buffer-contracts", "target_id": "test262:all",
                 "filter": "built-ins/TypedArray/"}
        aliased_case, = self.manager.prior_failed_cases([alias])
        self.assertEqual(aliased_case["target_id"], "test262:all")
        self.assertEqual(aliased_case["required_modes"], case["required_modes"])
        with patch.object(self.manager, "build", return_value=({}, {"bluejs-regexp-worker": "worker"})) as build, \
             patch.object(self.manager, "worker_ready", return_value=True), \
             patch.object(self.manager, "run_tests", return_value=True) as run_tests:
            self.manager.recheck_prior_failures([alias], {})
            build.assert_called_once_with([{**alias, "id": "test262:all"}], {}, "recheck")
            self.assertEqual(run_tests.call_args.args[0][0]["filter"], "language/a.js,language/b.js")

    def test_test262_recheck_rejects_missing_or_excluded_previous_failure_modes(self):
        case = {"id": "test262:all:case:previous-failures", "target_id": "test262:all",
                "case": "previous-failures", "kind": "test262", "label": "conformance recheck",
                "filter": "language/a.js", "required_modes": [("language/a.js", "strict")]}
        for status, expected in [(None, False), ("excluded", False), ("pass", True)]:
            run_id = "20261006-120000-" + (status or "missing")
            directory = self.manager.state / "runs" / run_id
            directory.mkdir(parents=True)
            self.manager.active = {"id": run_id, "status": "running", "tasks": [], "message": ""}
            def command(key, label, args, env, stage, accept):
                task = self.manager.add_task(key, label, stage)
                output = directory / "recheck/test262"
                output.mkdir(parents=True)
                rows = [{"path": "language/a.js", "mode": "sloppy", "status": "pass"}]
                if status:
                    rows.append({"path": "language/a.js", "mode": "strict", "status": status})
                (output / "summary.json").write_text(json.dumps({"scheduled_modes": len(rows),
                    "results": {name: sum(r["status"] == name for r in rows) for name in {r["status"] for r in rows}}}))
                (output / "results.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
                result = accept(0)
                task.update(status="passed" if result else "failed", elapsed_seconds=0)
                self.assertEqual(args[args.index("--filter") + 1], "language/a.js")
                return result
            with patch.object(self.manager, "command", side_effect=command):
                self.assertEqual(self.manager.run_tests([case], "recheck", bins={"bluejs-test262": "adapter"}), expected)
            self.assertNotIn(case["id"], self.manager.graph()["tests"])
            self.assertFalse((directory / "full/profiles").exists())

    def test_a_later_complete_target_pass_clears_older_case_failures(self):
        target = {"id": "rust:lib", "kind": "rust", "label": "library"}
        for run_id, status in [("20261006-120000-abcdef12", "failed"), ("20261006-120050-abcdef13", "passed")]:
            directory = self.manager.state / "runs" / run_id
            directory.mkdir(parents=True)
            (directory / "case.log").write_text("test vm::boundary ... FAILED\n")
            (directory / "run.json").write_text(json.dumps({"id": run_id, "mode": "pipeline", "status": "failed",
                "tasks": [{"id": "impact:rust:lib", "status": status, "log": "case.log"}]}))
        self.manager.active = {"id": "20261006-120100-abcdef14"}
        self.assertEqual(self.manager.prior_failed_cases([target]), [])

    def test_ignored_failure_recheck_blocks_and_missing_case_defers_to_the_inventory(self):
        for ignored, expected in [(1, False), (0, True)]:
            run_id = f"20261006-12000{ignored}-abcdef12"
            directory = self.manager.state / "runs" / run_id
            directory.mkdir(parents=True)
            self.manager.active = {"id": run_id, "status": "running", "tasks": [], "message": ""}
            binary = self.root / "fake-case"
            binary.write_text(f"#!/bin/sh\necho 'test result: ok. 0 passed; 0 failed; {ignored} ignored'\n")
            binary.chmod(0o755)
            case = {"id": "rust:lib:case:vm::boundary", "target_id": "rust:lib", "case": "vm::boundary", "label": "case", "kind": "rust"}
            self.assertEqual(self.manager.run_tests([case], "recheck", artifacts={"rust:lib": str(binary)}), expected)
            self.assertEqual(self.manager.active["tasks"][0]["status"], "failed" if ignored else "skipped")

    def test_remaining_failure_stops_before_the_affected_build(self):
        self.manager.environment = lambda: {}
        self.manager.recheck_prior_failures = lambda *args: (_ for _ in ()).throw(RuntimeError("Previous failure remains"))
        with patch.object(self.manager, "build") as build:
            self.manager.start("pipeline", ["backend/bluejs/src/lib.rs"])
            finished = self.wait()
            build.assert_not_called()
        self.assertEqual(finished["status"], "failed")
        self.assertEqual(finished["message"], "Previous failure remains")

    def test_missing_corpus_stops_pipeline_before_build_or_affected_tests(self):
        self.manager.corpus = self.root / "missing-corpus"
        pinned = self.root / "backend/bluejs/test262/snapshot.json"
        pinned.parent.mkdir(parents=True)
        pinned.write_text(json.dumps({"revision": "pinned", "manifest_sha256": "missing"}))
        self.manager.preflight_corpus = lambda: Manager.preflight_corpus(self.manager)
        self.manager.environment = lambda: self.fail("Build environment started before corpus validation")
        self.manager.start("pipeline", ["backend/bluejs/src/lib.rs"])
        finished = self.wait()
        self.assertEqual(finished["status"], "failed")
        self.assertIn("no build or runtime test has started", finished["message"])
        self.assertEqual([t["id"] for t in finished["tasks"]], ["prerequisite:corpus"])

    def test_sibling_discovery_runs_from_harness_and_adapter_without_environment_override(self):
        run_id = "20261006-120000-abcdef12"
        directory = self.manager.state / "runs" / run_id
        directory.mkdir(parents=True)
        self.manager.active = {"id": run_id, "status": "running", "tasks": [], "message": ""}
        binary = self.root / "fake-worker-test"
        binary.write_text("#!/bin/sh\necho 'test result: ok. 1 passed; 0 failed; 0 ignored'\n")
        observed = {}
        def command(key, label, argv, env, stage, accept):
            observed[key] = dict(env)
            task = next(t for t in self.manager.active["tasks"] if t["id"] == key)
            log = directory / task["log"]
            log.parent.mkdir(exist_ok=True)
            log.write_text('test result: ok. 1 passed; 0 failed; 0 ignored')
            task.update(status="passed", elapsed_seconds=1)
            return True
        self.manager.command = command
        tests = [{"id": key, "label": key, "kind": kind} for key, kind in (
            ("rust:test:regex_worker_reuse", "rust"), ("rust:test:ordinary", "rust"), ("test262:all", "test262"))]
        self.assertTrue(self.manager.run_tests(tests, "impact", {"BLUEJS_REGEXP_WORKER": "inherited-worker"},
            {t["id"]: str(binary) for t in tests if t["kind"] == "rust"},
            {"bluejs-test262": "adapter", "bluejs-regexp-worker": "verified-worker"}))
        self.assertNotIn("BLUEJS_REGEXP_WORKER", observed["impact:rust:test:regex_worker_reuse"])
        self.assertNotIn("BLUEJS_REGEXP_WORKER", observed["impact:test262:all"])
        self.assertEqual(observed["impact:rust:test:ordinary"]["BLUEJS_REGEXP_WORKER"], "verified-worker")

    def test_empty_worker_handshake_is_validated_without_runtime_profiles(self):
        run_id = "20261006-120000-abcdef12"
        (self.manager.state / "runs" / run_id).mkdir(parents=True)
        self.manager.active = {"id": run_id, "status": "running", "tasks": [], "message": ""}
        worker = self.root / "worker"
        worker.write_text("#!/bin/sh\nread input && exit 2\nprintf '\\026\\000\\000\\000bluejs-regexp-worker/1'\n")
        worker.chmod(0o755)
        self.assertTrue(self.manager.worker_ready({"bluejs-regexp-worker": str(worker)}, None, "impact"))
        task = self.manager.active["tasks"][0]
        row = json.loads((self.manager.state / "runs" / run_id / task["log"]).read_text())
        self.assertTrue(row["ready"])
        self.assertEqual(row["javascript_cases"], 0)
        self.assertFalse((self.manager.state / "runs" / run_id / "impact/profiles").exists())

    def test_invalid_worker_frame_stops_pipeline_before_any_test(self):
        worker = self.root / "bad-worker"
        worker.write_text("#!/bin/sh\nprintf 'unframed worker output'\n")
        worker.chmod(0o755)
        self.manager.environment = lambda: {}
        self.manager.build = lambda *args: ({}, {"bluejs-regexp-worker": str(worker)})
        with patch.object(self.manager, "run_tests") as run_tests:
            self.manager.start("pipeline", ["backend/bluejs/src/lib.rs"])
            finished = self.wait()
            run_tests.assert_not_called()
        self.assertEqual(finished["status"], "failed")
        self.assertIn("Regex worker startup failed", finished["message"])
        self.assertEqual(len(finished["tasks"]), 1)
        self.assertEqual(finished["tasks"][0]["status"], "failed")

    def test_source_edit_stops_run_and_marks_result_stale(self):
        self.test.write_text("import time,unittest\nclass Simple(unittest.TestCase):\n def test_wait(self): time.sleep(20)\n")
        self.manager.start("impact", ["backend/bluejs/tests/test_selftest.py"])
        deadline = time.monotonic() + 3
        while not self.manager.processes and time.monotonic() < deadline:
            time.sleep(.01)
        (self.root / "backend/bluejs/src/lib.rs").write_text("pub struct Changed;")
        finished = self.wait(7)
        self.assertEqual(finished["status"], "stale")

    def test_cancel_terminates_owned_process_and_retains_status(self):
        self.test.write_text("import time,unittest\nclass Simple(unittest.TestCase):\n def test_wait(self): time.sleep(20)\n")
        self.manager.start("impact", ["backend/bluejs/tests/test_selftest.py"])
        deadline = time.monotonic() + 3
        while not self.manager.processes and time.monotonic() < deadline:
            time.sleep(.01)
        self.manager.cancel()
        self.assertEqual(self.wait(7)["status"], "cancelled")
        self.assertFalse(self.manager.processes)

    def test_partial_pass_cannot_publish_full_report(self):
        run = self.manager.start("impact", ["backend/bluejs/tests/test_selftest.py"])
        self.wait()
        with self.assertRaises(ValueError):
            publish_report(self.manager, run["id"])

    def test_mutable_state_cannot_enter_source_snapshot(self):
        with self.assertRaises(ValueError):
            Manager(self.root, self.root / "backend/bluejs/state")

    def test_complete_current_selection_is_promoted_without_duplicate_execution(self):
        run_id = "20261005-120000-abcdef12"
        directory = self.manager.state / "runs" / run_id
        folder = directory / "impact/profiles/lib"
        folder.mkdir(parents=True)
        (folder / "fresh.profraw").write_text("retained current-run profile")
        self.manager.active = {"id": run_id, "status": "running", "tasks": [{"id": "impact:rust:lib", "stage": "impact",
            "status": "passed", "profile_directory": str(folder), "counts": [[1, 0, 0]], "log": "logs/lib.log"}]}
        self.manager.promote_complete_impact()
        task = self.manager.active["tasks"][0]
        self.assertEqual(task["id"], "full:rust:lib")
        self.assertEqual(task["stage"], "full")
        self.assertEqual(task["counts"], [[1, 0, 0]])
        self.assertTrue((Path(task["profile_directory"]) / "fresh.profraw").is_file())
        self.assertFalse((directory / "impact").exists())
        self.assertEqual(len(self.manager.active["tasks"]), 1)


class DashboardContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        (root / "backend/bluejs/src").mkdir(parents=True)
        (root / "backend/bluejs/src/lib.rs").write_text("pub struct Vm;")
        self.manager = Manager(root, root / "state")
        self.app = Application(self.manager)
        self.app.files = []
        self.server = make_server(self.app, 0)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}"

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(2)
        self.temp.cleanup()

    def request(self, path, body=None, token=True):
        headers = {"X-BlueJS-Token": self.app.token} if token else {}
        data = json.dumps(body).encode() if body is not None else None
        return urllib.request.urlopen(urllib.request.Request(self.url + path, data=data, headers=headers), timeout=3)

    def test_dashboard_and_plan_are_served_from_local_endpoint(self):
        with self.request("/") as response:
            self.assertIn(b"Test impact graph", response.read())
            self.assertIn("frame-ancestors 'none'", response.headers["Content-Security-Policy"])
        with self.request("/api/plan", {"files": ["backend/bluejs/src/lib.rs"]}) as response:
            self.assertIn("test262:all", {t["id"] for t in json.load(response)["tests"]})

    def test_dashboard_restores_retained_progress_after_the_runner_restarts(self):
        run_id = "20261006-120000-abcdef12"
        directory = self.manager.state / "runs" / run_id
        directory.mkdir(parents=True)
        record = {"id": run_id, "mode": "partition", "snapshot": snapshot(self.manager.root)["identity"],
                  "tasks": [{"id": "partition:python:selftest", "status": "passed", "log": "case.log"}]}
        (directory / "case.log").write_text("retained test output")
        for saved, shown in (("passed", "passed"), ("running", "interrupted")):
            with self.subTest(saved=saved):
                (directory / "run.json").write_text(json.dumps({**record, "status": saved}))
                with self.request("/api/state") as response:
                    state = json.load(response)
                self.assertEqual(state["active"]["id"], run_id)
                self.assertEqual(state["active"]["status"], shown)
                self.assertIsNone(self.manager.active)
                with self.request("/api/log?run=" + run_id + "&task=partition:python:selftest") as response:
                    self.assertEqual(response.read(), b"retained test output")

    def test_actions_require_local_dashboard_token(self):
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.request("/api/plan", {"files": []}, token=False)
        self.assertEqual(error.exception.code, 403)

    def test_path_traversal_and_arbitrary_run_commands_are_rejected(self):
        for path, body in [("/api/plan", {"files": ["../../outside"]}), ("/api/run", {"mode": "shell"})]:
            with self.subTest(path=path), self.assertRaises(urllib.error.HTTPError) as error:
                self.request(path, body)
            self.assertEqual(error.exception.code, 400)
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.request("/api/log?run=../../outside&task=anything")
        self.assertEqual(error.exception.code, 400)


if __name__ == "__main__":
    unittest.main()
