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
from backend.bluejs.selftest.common import relative, snapshot
from backend.bluejs.selftest.graph import empty_graph, inventory, plan, static_edges
from backend.bluejs.selftest.coverage import covered_sources, profile_owners
from backend.bluejs.selftest.report import compare_outcomes, publish_report
from backend.bluejs.selftest.runner import Manager, test262_passed, validate_impact
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

    def tearDown(self):
        self.manager.cancel()
        if self.manager.thread:
            self.manager.thread.join(5)
        self.temp.cleanup()

    def wait(self, timeout=8):
        self.manager.thread.join(timeout)
        self.assertFalse(self.manager.thread.is_alive())
        return self.manager.active

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

    def test_full_run_requires_matching_passed_impact(self):
        with self.assertRaises(ValueError):
            self.manager.start("full", ["backend/bluejs/tests/test_selftest.py"])

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
