# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import tempfile
from pathlib import Path
import unittest

from run import Worker, classify, metadata, modes, selected_files


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
