# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import tempfile
from pathlib import Path
import unittest

from run import (
    Worker,
    classify,
    instruction_budget,
    metadata,
    modes,
    module_sources,
    selected_files,
)


class RunnerTests(unittest.TestCase):
    def test_metadata_and_modes(self):
        self.assertEqual(modes(metadata("/*---\nflags: [async]\n---*/")), ["sloppy", "strict"])
        for flag, expected in [("raw", "raw"), ("module", "module"), ("onlyStrict", "strict"), ("noStrict", "sloppy")]:
            self.assertEqual(modes(metadata(f"/*---\nflags: [{flag}]\n---*/")), [expected])
        for source in ["", "/*---\nflags: [unknown]\n---*/", "/*---\nflags: [noStrict,onlyStrict]\n---*/", "/*---\nnegative: {phase: runtime}\n---*/"]:
            with self.assertRaises(ValueError):
                metadata(source)

    def test_negative_errors_require_phase_type_and_known_syntax(self):
        expected = {"phase": "parse", "type": "SyntaxError"}
        self.assertEqual(classify({"phase": "parse", "kind": "SyntaxError"}, expected), "pass")
        for reply in [{"phase": "runtime", "kind": "SyntaxError"}, {"phase": "parse", "kind": "TypeError"}, {"kind": "ok"}]:
            self.assertEqual(classify(reply, expected), "fail")
        for kind in ["unsupported", "unclassified_parse_error"]:
            self.assertEqual(classify({"phase": "parse", "kind": kind}, expected), "unsupported")
        self.assertEqual(classify({"kind": "timeout"}, {"phase": "runtime", "type": "RangeError"}), "timeout")

    def test_tail_call_feature_receives_a_budget_large_enough_for_the_standard_harness(self):
        self.assertEqual(instruction_budget({"features": []}, 100_000), 100_000)
        self.assertEqual(instruction_budget({"features": ["tail-call-optimization"]}, 100_000), 3_000_000)
        self.assertEqual(instruction_budget({"features": ["tail-call-optimization"]}, 4_000_000), 4_000_000)

    def test_filter_must_select_at_least_one_non_fixture_test(self):
        with tempfile.TemporaryDirectory() as temporary:
            corpus = Path(temporary)
            test = corpus / "test"
            test.mkdir()
            matching = test / "language" / "match.js"
            fixture = test / "language" / "match_FIXTURE.js"
            matching.parent.mkdir()
            matching.write_text("/*---\n---*/")
            fixture.write_text("/*---\n---*/")
            all_files = sorted(test.rglob("*.js"))

            self.assertEqual(selected_files(all_files, corpus, "language/match"), [matching])
            with self.assertRaisesRegex(ValueError, "selected no test files"):
                selected_files(all_files, corpus, "language/missing")

    def test_module_sources_collects_only_reachable_relative_fixtures(self):
        with tempfile.TemporaryDirectory() as temporary:
            test = Path(temporary) / "test"
            entry = test / "modules" / "entry.js"
            dependency = test / "modules" / "nested" / "dependency.js"
            unrelated = test / "modules" / "unrelated.js"
            dependency.parent.mkdir(parents=True)
            entry.write_text("import { value } from './nested/dependency.js'; value;")
            dependency.write_text("export { value } from '../entry.js';")
            unrelated.write_text("export const ignored = true;")

            sources = module_sources(entry, test)
            self.assertEqual(
                set(sources),
                {"modules/entry.js", "modules/nested/dependency.js"},
            )

    def test_supervisor_terminates_and_restarts_a_stalled_process(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "adapter"
            executable.write_text("#!/usr/bin/env python3\nimport json,sys,time\nprint('{\"ready\":1}',flush=True)\nfor line in sys.stdin:\n request=json.loads(line)\n if request.get('stall'): time.sleep(10)\n print('{\"kind\":\"ok\"}',flush=True)\n")
            executable.chmod(0o755)
            worker = Worker(executable, 0.05)
            try:
                self.assertEqual(worker.run({"stall": True})["kind"], "timeout")
                self.assertEqual(worker.run({})["kind"], "ok")
            finally:
                worker.close()


if __name__ == "__main__":
    unittest.main()
