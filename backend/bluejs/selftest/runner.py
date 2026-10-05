# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import concurrent.futures
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

from .common import DEFAULT_CORPUS, DEFAULT_STATE, ROOT, checked, complete, digest, load, now, save, snapshot
from .coverage import artifact_id, covered_sources, export_profiles, llvm_tool
from .graph import empty_graph, inventory, plan


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

    def history(self):
        rows = []
        for path in sorted((self.state / "runs").glob("*/run.json"), reverse=True):
            row = load(path)
            if row.get("status") == "running" and (not self.active or row["id"] != self.active["id"]):
                row["status"] = "interrupted"
                row["message"] = "The owning runner is no longer active"
            rows.append(row)
        return rows[:30]

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
        if mode not in ("impact", "full", "pipeline"):
            raise ValueError("Unknown run mode")
        with self.lock:
            if self.thread and self.thread.is_alive():
                raise ValueError("A run is already active")
            if workspace is not None:
                if not isinstance(workspace, bool):
                    raise ValueError("Workspace selection must be Boolean")
                self.workspace = workspace
            selected = plan(self.graph(), files, self.root, base)
            graph = self.graph()
            scope = graph.setdefault("acceptance_scope", {})
            modified = set(scope.get("modified_production_files", []))
            for name in selected["changed_files"]:
                if name.startswith("backend/bluejs/src/") and (self.root / name).is_file():
                    if digest(self.root / name) != graph.get("source_hashes", {}).get(name):
                        modified.add(name[len("backend/bluejs/src/"):])
            scope["modified_production_files"] = sorted(modified)
            save(self.state / "graph.json", graph)
            if mode == "full":
                previous = next((r for r in self.history() if r.get("mode") == "impact" and r["status"] == "passed"), None)
                validate_impact(previous, selected)
            run_id = time.strftime("%Y%m%d-%H%M%S-") + uuid.uuid4().hex[:8]
            directory = self.state / "runs" / run_id
            directory.mkdir(parents=True)
            save(directory / "plan.json", selected)
            frozen = snapshot(self.root)
            save(directory / "source-state.json", frozen)
            self.cancelled.clear()
            self.active = {"id": run_id, "mode": mode, "status": "running", "stage": "queued", "started_at": now(),
                           "snapshot": frozen["identity"], "selected_ids": [t["id"] for t in selected["tests"]],
                           "changed_files": selected["changed_files"], "tasks": [], "message": "Preparing the selected tests"}
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
        if needs_adapter:
            args = ["cargo", "build", "-p", "blueice-bluejs", "--bins", "--offline", "--jobs", str(self.jobs), "--message-format=json"]
            if not self.command(stage + ":bins", "Build Test262 adapter and regex worker", args, env, stage):
                raise RuntimeError("Adapter build failed")
            task = next(t for t in self.active["tasks"] if t["id"] == stage + ":bins")
            for line in (self.state / "runs" / self.active["id"] / task["log"]).read_text().splitlines():
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if row.get("reason") == "compiler-artifact" and row.get("executable"):
                    bins[row["target"]["name"]] = row["executable"]
            if not all(name in bins for name in ("bluejs-test262", "bluejs-regexp-worker")):
                raise RuntimeError("Cargo did not produce both adapter binaries")
        save(directory / "artifacts.json", artifacts)
        save(directory / "bins.json", bins)
        return artifacts, bins

    def run_tests(self, tests, stage, env=None, artifacts=None, bins=None):
        artifacts, bins = artifacts or {}, bins or {}
        directory = self.state / "runs" / self.active["id"] / stage
        directory.mkdir(exist_ok=True)
        for test in tests:
            self.add_task(stage + ":" + test["id"], test["label"], stage)
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
            if test["kind"] == "python":
                command = [sys.executable, "-m", "unittest", "discover", "-s", "backend/bluejs/tests", "-p", "test_selftest.py", "-v"]
                accepted = None
            elif test["kind"] == "rust":
                binary = artifacts[test["id"]]
                task["binary_sha256"] = digest(binary)
                task["binary"] = binary
                command = [binary, "--test-threads=" + str(self.test_threads)]
                accepted = None
            else:
                output = directory / "test262"
                command = [sys.executable, "backend/bluejs/test262/run.py", "--corpus", str(self.corpus),
                           "--adapter", bins["bluejs-test262"], "--output", str(output), "--jobs", str(self.jobs),
                           "--progress-interval", "5", "--flush-profiles"]
                if test.get("filter"):
                    command.extend(["--filter", test["filter"]])
                accepted = lambda code: code in (0, 1) and test262_passed(load(output / "summary.json"), not test.get("filter"))
            result = self.command(key, test["label"], command, test_env, stage, accepted)
            if not result:
                failed.set()
            task["profile_directory"] = str(folder)
            if test["kind"] == "rust":
                log = (self.state / "runs" / self.active["id"] / task["log"]).read_text()
                task["counts"] = [list(map(int, row)) for row in re.findall(
                    r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored", log)]
                if result and not task["counts"]:
                    task["status"] = "failed"
                    failed.set()
                    result = False
            self.persist()
            return result

        with concurrent.futures.ThreadPoolExecutor(max_workers=self.jobs) as pool:
            results = list(pool.map(execute, tests))
        graph = self.graph()
        for test in tests:
            task = next(t for t in self.active["tasks"] if t["id"] == stage + ":" + test["id"])
            if task["status"] == "passed":
                graph["tests"][test["id"]] = {**graph["tests"].get(test["id"], {}), **test,
                                               "elapsed_seconds": task["elapsed_seconds"]}
        graph["created_at"] = now()
        save(self.state / "graph.json", graph)
        return all(results)

    def learn(self, tests, stage, artifacts, bins):
        graph = self.graph()
        directory = self.state / "runs" / self.active["id"] / stage
        hashes = snapshot(self.root)["files"]
        for test in tests:
            if test["kind"] not in ("rust", "test262"):
                continue
            task = next(t for t in self.active["tasks"] if t["id"] == stage + ":" + test["id"])
            profiles = list(Path(task["profile_directory"]).glob("*.profraw"))
            binary = artifacts[test["id"]] if test["kind"] == "rust" else bins["bluejs-test262"]
            objects = [] if test["kind"] == "rust" else list(bins.values())
            destination = directory / "coverage" / uuid.uuid5(uuid.NAMESPACE_URL, test["id"]).hex
            if not profiles:
                raise RuntimeError("No fresh profiles from " + test["id"])
            payload = export_profiles(binary, profiles, destination, self.root, objects)
            sources = covered_sources(payload, self.root)
            # Retain the union of observations to protect previously exercised paths.
            existing = {e["source"] for e in graph["edges"] if e["target"] == test["id"] and e["kind"] == "observed"}
            graph["edges"].extend({"source": source, "target": test["id"], "kind": "observed",
                                   "evidence": self.active["id"], "source_sha256": hashes.get(source)}
                                  for source in sources if source not in existing)
            graph["observed_targets"] = sorted(set(graph["observed_targets"] + [test["id"]]))
            graph["tests"][test["id"]] = {**test, "elapsed_seconds": task["elapsed_seconds"]}
        from .graph import static_edges
        _, graph["structures"] = static_edges(self.root)
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
            engine_selected = any(t["kind"] in ("rust", "test262") for t in selected["tests"])
            env = self.environment() if engine_selected or mode == "full" else dict(os.environ)
            if mode in ("impact", "pipeline"):
                self.active["stage"] = "impact"
                artifacts, bins = self.build(selected["tests"], env, "impact") if engine_selected else ({}, {})
                if not self.run_tests(selected["tests"], "impact", env, artifacts, bins):
                    raise RuntimeError("Affected tests failed; full verification has not started")
                if snapshot(self.root)["identity"] != self.active["snapshot"]:
                    raise ValueError("Sources changed during affected tests")
                if engine_selected:
                    self.active["stage"] = "learning"
                    self.event("learning", message="Updating measured test relationships")
                    self.learn(selected["tests"], "impact", artifacts, bins)
                self.event("impact-passed", message="All affected tests passed for this source snapshot")
            if mode in ("full", "pipeline"):
                if not engine_selected and mode == "pipeline":
                    env = self.environment()
                self.active["stage"] = "full"
                full = [t for t in inventory(self.root).values() if t["kind"] in ("rust", "test262", "python")]
                engine_ids = {t["id"] for t in full if t["kind"] in ("rust", "test262")}
                selected_ids = {t["id"] for t in selected["tests"]}
                if mode == "pipeline" and engine_ids <= selected_ids:
                    self.promote_complete_impact()
                    remaining = [t for t in full if t["id"] not in selected_ids]
                    if not self.run_tests(remaining, "full", env, artifacts, bins):
                        raise RuntimeError("Full verification tool gate failed")
                else:
                    artifacts, bins = self.build(full, env, "full")
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
