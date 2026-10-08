# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import concurrent.futures
import hashlib
import json
import os
import re
import shlex
import signal
import subprocess
import sys
import threading
import time
import uuid
from pathlib import Path

from .common import DEFAULT_CORPUS, DEFAULT_STATE, ROOT, checked, complete, digest, load, now, retain_coverage_reference, save, snapshot
from .coverage import artifact_id, covered_sources, export_profiles, llvm_tool
from .graph import empty_graph, inventory, plan


CRITICAL_RUST_TARGETS = (
    "rust:lib", "rust:test:internal_boundary_contracts", "rust:test:coverage_hard_",
    "rust:test:coverage_difficult_", "rust:test:cov_g8_heap", "rust:test:try_completion",
    "rust:test:eval_var_environment_capture", "rust:test:cov_g4_test262_language",
    "rust:test:cov_g4_binary_data", "rust:test:test262_host", "rust:test:eval_var_catch_parameter",
    "rust:test:page_runtime_coverage", "rust:test:cov_g7_typed_arrays",
)


def refresh_observations(graph, sources, test, active, hashes, binary, coverage, profile_directory, complete_owner=False):
    """Refresh current evidence without losing conservative historical callers."""
    from .partitions import selection_identity
    provenance = {"evidence": active["id"], "snapshot": active["snapshot"],
                  "selection_identity": selection_identity({"tests": [test]}),
                  "test_source_sha256": hashes.get(test.get("source")),
                  "binary_sha256": digest(binary), "coverage_sha256": digest(coverage),
                  "profile_directory": str(profile_directory)}
    existing = {edge["source"]: edge for edge in graph["edges"]
                if edge["target"] == test["id"] and edge["kind"] == "observed"}
    if complete_owner:
        # Only an entire owning harness can retire an obsolete active path.
        # Partial selections keep the remaining owner relationships intact.
        obsolete = [edge for source, edge in existing.items() if source not in sources]
        graph.setdefault("historical_observations", []).extend(obsolete)
        graph["edges"] = [edge for edge in graph["edges"] if edge not in obsolete]
    for source in sources:
        current = {**provenance, "source_sha256": hashes.get(source)}
        if source in existing:
            edge = existing[source]
            if edge.get("source_sha256") != current["source_sha256"]:
                history = edge.setdefault("history", [])
                history.append({key: edge[key] for key in (*provenance, "source_sha256") if key in edge})
            edge.update(current)
        else:
            graph["edges"].append({"source": source, "target": test["id"], "kind": "observed", **current})


def validate_impact(previous, current):
    if not previous or previous.get("status") != "passed" or previous.get("mode") not in ("impact", "pipeline"):
        raise ValueError("Run the affected tests successfully before starting the full verification")
    if previous.get("snapshot") != current["snapshot"]:
        raise ValueError("Sources changed after the affected tests passed; create a fresh impact run")
    if set(previous.get("selected_ids", [])) != {t["id"] for t in current["tests"]}:
        raise ValueError("The affected test selection changed; run the current selection first")


def test262_passed(summary, complete_inventory=False):
    if not summary or (complete_inventory and not summary.get("complete_inventory")):
        return False
    results = summary.get("results", {})
    return (bool(results.get("pass")) and summary.get("scheduled_modes", 0) == sum(results.values())
            and set(results) <= {"pass", "excluded", "stale_corpus"})


class Manager:
    def __init__(self, root=ROOT, state=DEFAULT_STATE, corpus=DEFAULT_CORPUS, jobs=4, test_threads=4, workspace=False):
        self.root, self.state, self.corpus = Path(root), Path(state), Path(corpus)
        if not 1 <= jobs <= 8 or not 1 <= test_threads <= 8:
            raise ValueError("Worker and test thread counts must be between 1 and 8")
        self.jobs, self.test_threads, self.workspace = jobs, test_threads, workspace
        self.lock = threading.RLock()
        self.cancelled = threading.Event()
        self.active = None
        self.processes = {}
        self.thread = None
        self.state.mkdir(parents=True, exist_ok=True)
        if self.state.resolve().is_relative_to((self.root / "backend").resolve()) or self.state.resolve().is_relative_to((self.root / "scripts").resolve()):
            raise ValueError("Keep mutable run state outside the source directories, for example target/bluejs-selftest")
        if not (self.state / "graph.json").is_file():
            save(self.state / "graph.json", empty_graph(self.root))

    def graph(self):
        return load(self.state / "graph.json")

    def history(self, limit=30):
        rows = []
        for path in sorted((self.state / "runs").glob("*/run.json"), reverse=True):
            row = load(path)
            if row.get("status") == "running" and (not self.active or row["id"] != self.active["id"]):
                row["status"] = "interrupted"
                row["message"] = "The owning runner is no longer active"
            rows.append(row)
        return rows if limit is None else rows[:limit]

    def persist(self):
        with self.lock:
            save(self.state / "runs" / self.active["id"] / "run.json", self.active)

    def event(self, kind, **fields):
        with self.lock:
            row = {"time": now(), "kind": kind, **fields}
            path = self.state / "runs" / self.active["id"] / "events.jsonl"
            with path.open("a") as stream:
                stream.write(json.dumps(row) + "\n")
            self.active["message"] = fields.get("message", self.active.get("message", ""))
            self.persist()

    def start(self, mode="impact", files=None, base=None, workspace=None):
        if mode not in ("impact", "full", "pipeline", "partition"):
            raise ValueError("Unknown run mode")
        with self.lock:
            if self.thread and self.thread.is_alive():
                raise ValueError("A run is already active")
            if workspace is not None:
                if not isinstance(workspace, bool):
                    raise ValueError("Workspace selection must be Boolean")
                self.workspace = workspace
            selected = plan(self.graph(), files, self.root, base)
            from .partitions import partition_plan, passed_partition, selection_identity
            partition = partition_plan(self, files, base) if mode in ("partition", "full", "pipeline") else None
            if mode == "partition":
                selected = partition
            if partition and partition["snapshot"] != selected["snapshot"]:
                raise ValueError("Sources changed while planning partitions; create a fresh plan")
            frozen = snapshot(self.root)
            if frozen["identity"] != selected["snapshot"]:
                raise ValueError("Sources changed while selecting tests; create a fresh plan")
            texts = {}
            for name, expected in frozen["files"].items():
                if name.endswith(".rs"):
                    raw = (self.root / name).read_bytes()
                    if hashlib.sha256(raw).hexdigest() != expected:
                        raise ValueError("Sources changed while freezing Rust inputs")
                    texts[name] = raw.decode("utf-8")
            if snapshot(self.root)["identity"] != frozen["identity"]:
                raise ValueError("Sources changed while freezing inputs; create a fresh plan")
            graph = self.graph()
            scope = graph.setdefault("acceptance_scope", {})
            modified = set(scope.get("modified_production_files", []))
            for name in selected["changed_files"]:
                if name.startswith("backend/bluejs/src/") and (self.root / name).is_file():
                    if digest(self.root / name) != graph.get("source_hashes", {}).get(name):
                        modified.add(name[len("backend/bluejs/src/"):])
            scope["modified_production_files"] = sorted(modified)
            save(self.state / "graph.json", graph)
            partition_gate = passed_partition(self, partition) if partition else None
            if mode == "full" and not partition_gate:
                previous = next((r for r in self.history() if r.get("mode") == "impact" and r["status"] == "passed"), None)
                validate_impact(previous, selected)
            run_id = time.strftime("%Y%m%d-%H%M%S-") + uuid.uuid4().hex[:8]
            directory = self.state / "runs" / run_id
            directory.mkdir(parents=True)
            save(directory / "plan.json", selected)
            save(directory / "source-state.json", frozen)
            save(directory / "frozen-rust-sources.json", texts)
            from .graph import static_edges
            _, structures = static_edges(self.root)
            save(directory / "source-structures.json", structures)
            self.cancelled.clear()
            self.active = {"id": run_id, "mode": mode, "status": "running", "stage": "queued", "started_at": now(),
                           "snapshot": frozen["identity"], "selected_ids": [t["id"] for t in selected["tests"]],
                           "changed_files": selected["changed_files"], "tasks": [], "message": "Preparing the selected tests"}
            if partition:
                self.active["selection_identity"] = selection_identity(partition)
            if partition_gate and mode in ("full", "pipeline"):
                self.active["partition_gate"] = partition_gate["id"]
            self.persist()
            self.thread = threading.Thread(target=self.execute, args=(selected,), daemon=True)
            self.thread.start()
            return dict(self.active)

    def add_task(self, key, label, stage):
        with self.lock:
            existing = next((t for t in self.active["tasks"] if t["id"] == key), None)
            if existing:
                return existing
            task = {"id": key, "label": label, "stage": stage, "status": "queued", "elapsed_seconds": 0,
                    "log": "logs/" + uuid.uuid5(uuid.NAMESPACE_URL, key).hex + ".log"}
            self.active["tasks"].append(task)
            self.persist()
            return task

    def command(self, key, label, command, env=None, stage="impact", accept=None):
        task = self.add_task(key, label, stage)
        directory = self.state / "runs" / self.active["id"]
        log = directory / task["log"]
        log.parent.mkdir(exist_ok=True)
        if self.cancelled.is_set():
            task["status"] = "cancelled"
            self.persist()
            return False
        started = time.monotonic()
        with self.lock:
            task.update(status="running", command=command, started_at=now())
            self.persist()
        self.event("task-start", task=key, message="Running " + label)
        with log.open("w") as output:
            process = subprocess.Popen(command, cwd=self.root, env=env, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace", start_new_session=True)
            with self.lock:
                self.processes[key] = process
            last = time.monotonic()
            for line in process.stdout:
                output.write(line)
                if time.monotonic() - last > 1:
                    output.flush()
                    with self.lock:
                        task["last_output"] = line.rstrip()[-500:]
                        task["elapsed_seconds"] = time.monotonic() - started
                        self.persist()
                    last = time.monotonic()
            process.stdout.close()
            code = process.wait()
        with self.lock:
            self.processes.pop(key, None)
            task.update(exit_code=code, elapsed_seconds=time.monotonic() - started, finished_at=now())
            ok = (accept(code) if accept else code == 0) and not self.cancelled.is_set()
            task["status"] = "passed" if ok else ("cancelled" if self.cancelled.is_set() else "failed")
            self.persist()
        self.event("task-finish", task=key, status=task["status"], message=label + ": " + task["status"])
        return ok

    def cancel(self):
        self.cancelled.set()
        with self.lock:
            processes = list(self.processes.values())
        for process in processes:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
        def force():
            for process in processes:
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
        threading.Thread(target=force, daemon=True).start()

    def environment(self):
        env = os.environ.copy()
        env.update(LLVM_COV=llvm_tool("llvm-cov"), LLVM_PROFDATA=llvm_tool("llvm-profdata"), PYTHONDONTWRITEBYTECODE="1")
        env["CARGO_TARGET_DIR"] = str(self.root / ("target" if sys.platform.startswith("linux") else "target/llvm-cov-target"))
        for line in checked(["cargo", "llvm-cov", "show-env", "--sh"], self.root, env).splitlines():
            if line.startswith("export "):
                key, value = line[7:].split("=", 1)
                env[key] = shlex.split(value)[0]
        flags = env.get("CARGO_ENCODED_RUSTFLAGS", "")
        atomic = "-instrprof-atomic-counter-update-all"
        if atomic not in flags:
            env["CARGO_ENCODED_RUSTFLAGS"] = (flags + "\x1f" if flags else "") + "-C\x1fllvm-args=" + atomic
        return env

    def preflight_corpus(self):
        code = ("import json,sys; from pathlib import Path; sys.path.insert(0,sys.argv[2]); "
                "from backend.bluejs.selftest.common import load,validate_corpus; "
                "print(json.dumps(validate_corpus(Path(sys.argv[1]),load('backend/bluejs/test262/snapshot.json'))))")
        if not self.command("prerequisite:corpus", "Validate the pinned corpus before building",
                            [sys.executable, "-c", code, str(self.corpus), str(ROOT)], stage="prerequisite"):
            raise RuntimeError("Pinned corpus validation failed; no build or runtime test has started")

    def build(self, tests, env, stage):
        rust = [t for t in tests if t["kind"] == "rust"]
        needs_adapter = any(t["kind"] == "test262" for t in tests)
        directory = self.state / "runs" / self.active["id"] / stage
        directory.mkdir(exist_ok=True)
        artifacts = {}
        if rust:
            args = ["cargo", "test", "-p", "blueice-bluejs", "--offline", "--no-run", "--jobs", str(self.jobs), "--message-format=json"]
            if stage == "full":
                args.append("--all-targets")
            else:
                for test in rust:
                    args.extend(test["cargo_args"])
            if not self.command(stage + ":build", "Build instrumented Rust targets", args, env, stage):
                raise RuntimeError("Instrumented build failed")
            task = next(t for t in self.active["tasks"] if t["id"] == stage + ":build")
            for line in (self.state / "runs" / self.active["id"] / task["log"]).read_text().splitlines():
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if row.get("reason") == "compiler-artifact" and row.get("executable") and row["profile"]["test"]:
                    artifacts[artifact_id(row)] = row["executable"]
            missing = {t["id"] for t in rust} - artifacts.keys()
            if missing:
                raise RuntimeError("Cargo did not produce selected targets: " + ", ".join(sorted(missing)))
        bins = {}
        if rust or needs_adapter:
            required_bins = ["bluejs-regexp-worker"] + (["bluejs-test262"] if needs_adapter else [])
            args = ["cargo", "build", "-p", "blueice-bluejs", "--offline", "--jobs", str(self.jobs), "--message-format=json"]
            for name in required_bins:
                args.extend(["--bin", name])
            if not self.command(stage + ":bins", "Build regex worker and requested adapter", args, env, stage):
                raise RuntimeError("Adapter build failed")
            task = next(t for t in self.active["tasks"] if t["id"] == stage + ":bins")
            for line in (self.state / "runs" / self.active["id"] / task["log"]).read_text().splitlines():
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if row.get("reason") == "compiler-artifact" and row.get("executable"):
                    bins[row["target"]["name"]] = row["executable"]
            if not all(name in bins for name in required_bins):
                raise RuntimeError("Cargo did not produce the required adapter binaries")
        save(directory / "artifacts.json", artifacts)
        save(directory / "bins.json", bins)
        return artifacts, bins

    def worker_ready(self, bins, env, stage):
        binary = bins["bluejs-regexp-worker"]
        folder = self.state / "runs" / self.active["id"] / stage / "worker-readiness"
        folder.mkdir(parents=True, exist_ok=True)
        probe_env = dict(env or os.environ)
        probe_env["LLVM_PROFILE_FILE"] = str(folder / "readiness-%p-%m.profraw")
        # Empty input checks the framed READY response and closes the worker.
        # No JavaScript or test case executes; these profiles remain separate.
        code = ("import json,subprocess,sys,time; start=time.monotonic(); "
                "result=subprocess.run([sys.argv[1]],input=b'',stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=300); "
                "expected=b'bluejs-regexp-worker/1'; "
                "ready=result.returncode==0 and result.stdout==len(expected).to_bytes(4,'little')+expected; "
                "print(json.dumps({'ready':ready,'exit_code':result.returncode,'elapsed_seconds':time.monotonic()-start,"
                "'javascript_cases':0,'stderr':result.stderr.decode(errors='replace')}),flush=True); "
                "sys.exit(0 if ready else 1)")
        key = stage + ":worker-ready"
        task = self.add_task(key, "Check regex worker startup without JavaScript", stage)
        task.update(binary=binary, binary_sha256=digest(binary))
        return self.command(key, task["label"], [sys.executable, "-c", code, binary], probe_env, stage)

    def prior_failed_cases(self, tests):
        selected = {t.get("target_id", t["id"]): {**t, "id": t.get("target_id", t["id"])}
                    for t in tests if t["kind"] in ("rust", "test262")}
        cleared = set()
        for previous in self.history():
            if previous["id"] == self.active["id"]:
                continue
            if previous["status"] == "passed" and previous["mode"] in ("full", "pipeline"):
                return []
            if previous["status"] not in ("failed", "cancelled"):
                continue
            cases = {}
            directory = self.state / "runs" / previous["id"]
            for task in previous.get("tasks", []):
                target = task.get("target_id", task["id"].split(":", 1)[-1])
                if target not in selected or target in cleared:
                    continue
                if task["status"] == "passed" and not task.get("case"):
                    cleared.add(target)
                    continue
                if task["status"] != "failed":
                    continue
                if selected[target]["kind"] == "test262":
                    results = directory / task.get("stage", task["id"].split(":", 1)[0]) / "test262/results.jsonl"
                    if not results.resolve().is_relative_to(directory.resolve()) or not results.is_file():
                        continue
                    rows = [json.loads(line) for line in results.read_text().splitlines() if line]
                    required = sorted({(row["path"], row["mode"]) for row in rows
                                       if row["status"] not in ("pass", "excluded", "stale_corpus")})
                    paths = sorted({path for path, _ in required})
                    if any(not path.endswith(".js") or "," in path or
                           not (self.corpus / "test" / path).resolve().is_relative_to((self.corpus / "test").resolve())
                           for path in paths):
                        raise ValueError("Invalid retained Test262 failure path")
                    if required:
                        key = target + ":case:previous-failures"
                        cases[key] = {**selected[target], "id": key, "target_id": target,
                                      "case": "previous-failures", "required_modes": required,
                                      "filter": ",".join(paths),
                                      "label": f"Test262 failure recheck ({len(required)} modes in {len(paths)} files)"}
                    continue
                path = directory / task["log"]
                if not path.resolve().is_relative_to(directory.resolve()) or not path.is_file():
                    continue
                for case in re.findall(r"^test ([A-Za-z_][A-Za-z_0-9:]*) \.\.\. FAILED$", path.read_text(), re.M):
                    key = target + ":case:" + case
                    cases[key] = {**selected[target], "id": key, "target_id": target, "case": case,
                                  "label": selected[target]["label"] + " / " + case}
            if cases:
                return list(cases.values())
        return []

    def recheck_prior_failures(self, tests, env):
        cases = self.prior_failed_cases(tests)
        if not cases:
            return
        self.active["stage"] = "recheck"
        self.event("failure-recheck", message=f"Rechecking {len(cases)} previously failing cases before the affected suite")
        targets = {t.get("target_id", t["id"]): {**t, "id": t.get("target_id", t["id"])}
                   for t in tests if any(c["target_id"] == t.get("target_id", t["id"]) for c in cases)}
        artifacts, bins = self.build(list(targets.values()), env, "recheck")
        if not self.worker_ready(bins, env, "recheck"):
            raise RuntimeError("Regex worker startup failed; no failure recheck has started")
        if not self.run_tests(cases, "recheck", env, artifacts, bins):
            raise RuntimeError("Previously failing cases still fail; affected and full suites have not started")
        self.event("failure-recheck-passed", message="Previous failure checks completed; continuing with the affected suite")

    def run_tests(self, tests, stage, env=None, artifacts=None, bins=None):
        artifacts, bins = artifacts or {}, bins or {}
        directory = self.state / "runs" / self.active["id"] / stage
        directory.mkdir(exist_ok=True)
        for test in tests:
            task = self.add_task(stage + ":" + test["id"], test["label"], stage)
            if test.get("case"):
                task.update(case=test["case"], target_id=test["target_id"])
        failed = threading.Event()

        def execute(test):
            key = stage + ":" + test["id"]
            task = next(t for t in self.active["tasks"] if t["id"] == key)
            if failed.is_set() or self.cancelled.is_set():
                with self.lock:
                    task["status"] = "skipped"
                    self.persist()
                return False
            folder = directory / "profiles" / uuid.uuid5(uuid.NAMESPACE_URL, test["id"]).hex
            folder.mkdir(parents=True, exist_ok=True)
            test_env = dict(env or os.environ)
            test_env["LLVM_PROFILE_FILE"] = str(folder / "blueice-%p-%m.profraw")
            if test.get("target_id", test["id"]) == "rust:test:regex_worker_reuse" or test["kind"] == "test262":
                # Exercise sibling discovery from both a Cargo deps harness
                # and the adapter directory, using the already verified worker.
                test_env.pop("BLUEJS_REGEXP_WORKER", None)
            elif "bluejs-regexp-worker" in bins:
                test_env["BLUEJS_REGEXP_WORKER"] = bins["bluejs-regexp-worker"]
            if test["kind"] == "python":
                command = [sys.executable, "-m", "unittest", "discover", "-s", "backend/bluejs/tests", "-p", "test_selftest.py", "-v"]
                accepted = None
            elif test["kind"] == "rust":
                binary = artifacts[test.get("target_id", test["id"])]
                task["binary_sha256"] = digest(binary)
                task["binary"] = binary
                command = [binary, "--test-threads=" + str(self.test_threads)]
                if test.get("case"):
                    command.extend(["--exact", *test.get("case_names", [test["case"]])])
                accepted = None
            else:
                output = directory / "test262"
                command = [sys.executable, "backend/bluejs/test262/run.py", "--corpus", str(self.corpus),
                           "--adapter", bins["bluejs-test262"], "--output", str(output), "--jobs", str(self.jobs),
                           "--progress-interval", "5", "--flush-profiles"]
                if test.get("filter"):
                    command.extend(["--filter", test["filter"]])
                def accepted(code):
                    if code not in (0, 1) or not test262_passed(load(output / "summary.json"), not test.get("filter")):
                        return False
                    if test.get("required_modes"):
                        path = output / "results.jsonl"
                        if not path.is_file():
                            return False
                        rows = [json.loads(line) for line in path.read_text().splitlines() if line]
                        outcomes = {(r["path"], r["mode"]): r["status"] for r in rows}
                        return all(outcomes.get(tuple(mode)) == "pass" for mode in test["required_modes"])
                    return True
            result = self.command(key, test["label"], command, test_env, stage, accepted)
            if not result:
                failed.set()
            task["profile_directory"] = str(folder)
            if test["kind"] == "rust":
                log = (self.state / "runs" / self.active["id"] / task["log"]).read_text()
                task["counts"] = [list(map(int, row)) for row in re.findall(
                    r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored", log)]
                if result and test.get("case") and task["counts"] == [[0, 0, 0]]:
                    if test.get("partition"):
                        task["status"] = "failed"
                        result = False
                        failed.set()
                    else:
                        task["status"] = "skipped"
                        self.event("obsolete-failure-case", task=key, message="The prior case is absent; the current affected inventory still requires verification")
                elif result and test.get("case") and not test.get("partition") and any(c[2] for c in task["counts"]):
                    task["status"] = "failed"
                    result = False
                    failed.set()
                    self.event("ignored-failure-case", task=key, message="A previously failing case is ignored; verification cannot claim its correction")
                if result and test.get("partition") and sum(sum(c) for c in task["counts"]) != len(test["case_names"]):
                    task["status"] = "failed"
                    failed.set()
                    result = False
                    self.event("partition-count-mismatch", task=key, message="Rust partition results do not match the discovered case selection")
                if result and not task["counts"]:
                    task["status"] = "failed"
                    failed.set()
                    result = False
            self.persist()
            return result

        critical = [t for t in tests if t["kind"] == "rust" and t["id"].startswith(CRITICAL_RUST_TARGETS)]
        groups = [
            [t for t in tests if t["kind"] == "python"],
            critical,
            [t for t in tests if t["kind"] == "rust" and t not in critical],
            [t for t in tests if t["kind"] == "test262"],
        ]
        results = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=self.jobs) as pool:
            for group in groups:
                # Complete each prerequisite before allowing the next group
                # to consume runtime cost. Each target still executes once.
                results.extend(pool.map(execute, group))
        graph = self.graph()
        for test in tests:
            task = next(t for t in self.active["tasks"] if t["id"] == stage + ":" + test["id"])
            if task["status"] == "passed":
                collection = graph.setdefault("case_history", {}) if test.get("case") else graph["tests"]
                collection[test["id"]] = {**collection.get(test["id"], {}), **test,
                                          "elapsed_seconds": task["elapsed_seconds"]}
        graph["created_at"] = now()
        save(self.state / "graph.json", graph)
        return all(results)

    def learn(self, tests, stage, artifacts, bins):
        graph = self.graph()
        directory = self.state / "runs" / self.active["id"] / stage
        hashes = snapshot(self.root)["files"]
        measured = [test for test in tests if test["kind"] in ("rust", "test262")]
        for index, test in enumerate(measured, 1):
            if test["kind"] not in ("rust", "test262"):
                continue
            task = next(t for t in self.active["tasks"] if t["id"] == stage + ":" + test["id"])
            profiles = list(Path(task["profile_directory"]).glob("*.profraw"))
            binary = artifacts[test.get("target_id", test["id"])] if test["kind"] == "rust" else bins["bluejs-test262"]
            objects = [] if test["kind"] == "rust" else list(bins.values())
            destination = directory / "coverage" / uuid.uuid5(uuid.NAMESPACE_URL, test["id"]).hex
            if not profiles:
                raise RuntimeError("No fresh profiles from " + test["id"])
            payload = export_profiles(binary, profiles, destination, self.root, objects)
            save(destination / "evidence.json", {"inputs": {"binary": digest(binary), "profiles": {str(p): digest(p) for p in profiles}},
                                                  "coverage_sha256": digest(destination / "coverage.json")})
            sources = covered_sources(payload, self.root)
            refresh_observations(graph, sources, test, self.active, hashes, binary,
                                 destination / "coverage.json", task["profile_directory"],
                                 complete_owner=not test.get("case") and not test.get("filter")
                                 and (not test.get("case_filters") or test["case_filters"] == [""]))
            graph["observed_targets"] = sorted(set(graph["observed_targets"] + [test["id"]]))
            graph["tests"][test["id"]] = {**test, "elapsed_seconds": task["elapsed_seconds"]}
            if index == 1 or index % 20 == 0 or index == len(measured):
                self.event("learning-progress", done=index, total=len(measured),
                           message=f"Reading measured test relationships: {index}/{len(measured)}")
        from .graph import static_edges
        _, graph["structures"] = static_edges(self.root)
        engine_inventory = {key for key, value in inventory(self.root).items() if value["kind"] in ("rust", "test262")}
        complete_selection = {test["id"] for test in tests if not test.get("case") and not test.get("filter")
                              and (not test.get("case_filters") or test["case_filters"] == [""])}
        if engine_inventory <= complete_selection:
            from .shared_functions import verified_body_contracts
            frozen = load(self.state / "runs" / self.active["id"] / "frozen-rust-sources.json", {})
            graph["shared_body_contracts"] = verified_body_contracts(frozen, self.active["snapshot"])
        graph["source_hashes"] = hashes
        graph["created_at"] = now()
        save(self.state / "graph.json", graph)

    def promote_complete_impact(self):
        """Use a full selection from this run once, with the same frozen source."""
        directory = self.state / "runs" / self.active["id"]
        impact, full = directory / "impact", directory / "full"
        impact.rename(full)
        with self.lock:
            for task in self.active["tasks"]:
                if task["stage"] == "impact":
                    task["stage"] = "full"
                    task["id"] = "full:" + task["id"][7:]
                    task["origin"] = "Complete affected selection; executed once in this run"
                    if task.get("profile_directory"):
                        task["profile_directory"] = str(full / Path(task["profile_directory"]).relative_to(impact))
            self.active["complete_selection_reused"] = True
            self.persist()
        self.event("complete-selection", message="Affected selection covered the full engine inventory; using its current-run evidence once")

    def discover_partitions(self, tests, artifacts, env, directory):
        """List independent harnesses concurrently without executing cases."""
        from .partitions import case_partitions
        rust = [target for target in tests if target["kind"] == "rust"]
        if not rust:
            return list(tests)
        discovery = directory / "partition/discovery"
        discovery.mkdir(exist_ok=True)
        listing_env = dict(env)
        listing_env["LLVM_PROFILE_FILE"] = str(discovery / "list-%p-%m.profraw")
        for target in rust:
            self.add_task("partition:list:" + target["id"], "List cases in " + target["label"], "partition")
        failed = threading.Event()

        def discover(target):
            key = "partition:list:" + target["id"]
            task = next(t for t in self.active["tasks"] if t["id"] == key)
            if failed.is_set():
                task["status"] = "skipped"
                self.persist()
                return []
            if not self.command(key, task["label"], [artifacts[target["id"]], "--list", "--format", "terse"],
                                listing_env, "partition"):
                failed.set()
                return []
            try:
                return case_partitions(target, (directory / task["log"]).read_text())
            except ValueError:
                failed.set()
                raise

        with concurrent.futures.ThreadPoolExecutor(max_workers=self.jobs) as pool:
            groups = list(pool.map(discover, rust))
        if failed.is_set():
            raise RuntimeError("Rust test discovery failed; no selected case has started")
        indexed = {target["id"]: group for target, group in zip(rust, groups)}
        return [case for target in tests for case in (indexed[target["id"]] if target["kind"] == "rust" else [target])]

    def execute(self, selected):
        directory = self.state / "runs" / self.active["id"]
        started = time.monotonic()
        finished = threading.Event()
        def watch():
            while not finished.wait(2):
                if snapshot(self.root)["identity"] != self.active["snapshot"]:
                    with self.lock:
                        self.active["source_changed"] = True
                    self.cancel()
                    break
        watcher = threading.Thread(target=watch, daemon=True)
        watcher.start()
        try:
            mode = self.active["mode"]
            full = [t for t in inventory(self.root).values() if t["kind"] in ("rust", "test262", "python")]
            engine_ids = {t["id"] for t in full if t["kind"] in ("rust", "test262")}
            selected_ids = {t["id"] for t in selected["tests"]}
            promote = mode == "pipeline" and engine_ids <= selected_ids and not self.active.get("partition_gate")
            engine_selected = any(t["kind"] in ("rust", "test262") for t in selected["tests"])
            if mode in ("full", "pipeline") or any(t["kind"] == "test262" for t in selected["tests"]):
                self.active["stage"] = "prerequisite"
                self.preflight_corpus()
            env = self.environment() if engine_selected or mode == "full" else dict(os.environ)
            if mode == "partition":
                self.active["stage"] = "partition"
                tooling = [test for test in selected["tests"] if test["kind"] == "python"]
                if tooling and not self.run_tests(tooling, "partition", env):
                    raise RuntimeError("Tool contracts failed; no Rust partition build has started")
                engine = [test for test in selected["tests"] if test["kind"] != "python"]
                if engine_selected:
                    self.recheck_prior_failures(engine, env)
                self.active["stage"] = "partition"
                artifacts, bins = self.build(engine, env, "partition") if engine_selected else ({}, {})
                if engine_selected and not self.worker_ready(bins, env, "partition"):
                    raise RuntimeError("Regex worker startup failed; no Rust partition has started")
                cases = self.discover_partitions(engine, artifacts, env, directory) if engine_selected else []
                save(directory / "partition/selection.json", tooling + cases)
                self.active["partition_cases"] = sum(len(t.get("case_names", [])) for t in cases)
                self.active["partition_count"] = sum(t.get("partition", False) for t in cases)
                self.persist()
                if not self.run_tests(cases, "partition", env, artifacts, bins):
                    raise RuntimeError("Selected partition tests failed; full verification has not started")
                if engine_selected and cases:
                    self.learn(cases, "partition", artifacts, bins)
                self.event("partition-passed", message="All selected partitions passed; full verification is available for this snapshot")
            if mode == "impact" or (mode == "pipeline" and not self.active.get("partition_gate")):
                if engine_selected:
                    self.recheck_prior_failures(selected["tests"], env)
                self.active["stage"] = "impact"
                artifacts, bins = self.build(selected["tests"], env, "impact") if engine_selected else ({}, {})
                if engine_selected and not self.worker_ready(bins, env, "impact"):
                    raise RuntimeError("Regex worker startup failed; no affected test has started")
                if not self.run_tests(selected["tests"], "impact", env, artifacts, bins):
                    raise RuntimeError("Affected tests failed; full verification has not started")
                if snapshot(self.root)["identity"] != self.active["snapshot"]:
                    raise ValueError("Sources changed during affected tests")
                if engine_selected and not promote:
                    self.active["stage"] = "learning"
                    self.event("learning", message="Updating measured test relationships")
                    self.learn(selected["tests"], "impact", artifacts, bins)
                self.event("impact-passed", message="All affected tests passed for this source snapshot")
            if mode in ("full", "pipeline"):
                if not engine_selected and mode == "pipeline":
                    env = self.environment()
                self.active["stage"] = "full"
                if promote:
                    self.promote_complete_impact()
                    remaining = [t for t in full if t["id"] not in selected_ids]
                    if not self.run_tests(remaining, "full", env, artifacts, bins):
                        raise RuntimeError("Full verification tool gate failed")
                else:
                    artifacts, bins = self.build(full, env, "full")
                    if not self.worker_ready(bins, env, "full"):
                        raise RuntimeError("Regex worker startup failed; no full test has started")
                    if not self.run_tests(full, "full", env, artifacts, bins):
                        raise RuntimeError("Full verification failed")
                if self.workspace:
                    if not self.command("workspace:tests", "Workspace runtime tests", ["cargo", "test", "--workspace", "--exclude", "blueice-bluejs", "--offline", "--jobs", str(self.jobs), "--", "--test-threads=" + str(self.test_threads)], env, "full"):
                        raise RuntimeError("Workspace runtime gate failed")
                    if not self.command("workspace:docs", "BlueJS Rustdoc tests", ["cargo", "test", "-p", "blueice-bluejs", "--doc", "--offline", "--jobs", str(self.jobs)], env, "full"):
                        raise RuntimeError("Rustdoc gate failed")
                self.active["stage"] = "report"
                self.event("coverage-export", message="Exporting fresh full coverage and generating the report")
                from .report import full_report
                report = full_report(self, artifacts, bins)
                self.active["report"] = str(report.relative_to(self.state))
                self.learn(full, "full", artifacts, bins)
                graph = self.graph()
                data = load(directory / "report-data.json")
                retain_coverage_reference(graph, data["files"], directory)
                graph["baseline"] = {"path": str(directory / "full"), "measured_at": data["measured_at"],
                    "rust": {**data["rust"], "elapsed_seconds": time.monotonic() - started}, "test262": data["test262"],
                    "full_coverage": data["files"], "audit": {"source_files": len(data["files"]) + len(data["no_counters"]),
                        "instrumented_files": len(data["files"]), "complete_files": sum(complete(v) for v in data["files"].values()),
                        "totals": data["totals"], "raw_percentage_regressions": data["coverage_regressions"]}}
                graph["created_at"] = now()
                save(self.state / "graph.json", graph)
            if snapshot(self.root)["identity"] != self.active["snapshot"]:
                raise ValueError("Sources changed during verification")
            self.active["status"] = "passed"
            self.active["message"] = "Verification completed for the frozen source snapshot"
        except Exception as error:
            changed = snapshot(self.root)["identity"] != self.active["snapshot"]
            self.active["status"] = "stale" if changed else ("cancelled" if self.cancelled.is_set() else "failed")
            self.active["message"] = str(error)
        finally:
            finished.set()
            self.active["finished_at"] = now()
            self.active["elapsed_seconds"] = time.monotonic() - started
            self.persist()
            self.event("run-finish", status=self.active["status"], message=self.active["message"])
