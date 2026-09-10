#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Complete, pinned Test262 inventory and supervised BlueJS execution.

Unsupported cases remain in the denominator. Exit 0 requires every mode to
pass; exit 1 means non-passing cases, exit 2 means runner/configuration failure.
Only the named _FIXTURE resources are excluded from test discovery.
"""
import argparse
import collections
import concurrent.futures
import hashlib
import io
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import tarfile
import threading
import time
import urllib.request

import yaml

ROOT = Path(__file__).resolve().parents[3]
SNAPSHOT = json.loads(Path(__file__).with_name("snapshot.json").read_text())
FRONTMATTER = re.compile(r"/\*---(.*?)---\*/", re.DOTALL)
NATIVE_INCLUDES = frozenset({"sta.js", "assert.js", "propertyHelper.js", "isConstructor.js"})
TAIL_CALL_INSTRUCTION_BUDGET = 3_000_000


def fetch(destination):
    if destination.exists():
        raise ValueError(f"refusing to replace existing corpus: {destination}")
    revision = SNAPSHOT["revision"]
    url = f"https://codeload.github.com/tc39/test262/tar.gz/{revision}"
    with urllib.request.urlopen(url, timeout=60) as response:
        archive = response.read()
    if hashlib.sha256(archive).hexdigest() != SNAPSHOT["archive_sha256"]:
        raise ValueError("Test262 archive checksum mismatch")
    destination.mkdir(parents=True)
    manifest = {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        for member in tar:
            relative = Path(*Path(member.name).parts[1:])
            if relative.is_absolute() or ".." in relative.parts or member.issym() or member.islnk():
                raise ValueError("unsafe archive entry")
            target = destination / relative
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                contents = tar.extractfile(member).read()
                target.write_bytes(contents)
                manifest[relative.as_posix()] = hashlib.sha256(contents).hexdigest()
    manifest_bytes = (json.dumps(manifest, sort_keys=True) + "\n").encode()
    if hashlib.sha256(manifest_bytes).hexdigest() != SNAPSHOT["manifest_sha256"]:
        raise ValueError("archive file manifest mismatch")
    (destination / ".bluejs-manifest.json").write_bytes(manifest_bytes)
    (destination / ".bluejs-snapshot.json").write_text(json.dumps(SNAPSHOT, indent=2) + "\n")


def metadata(source):
    match = FRONTMATTER.search(source)
    if not match:
        raise ValueError("missing Test262 frontmatter")
    data = yaml.safe_load(match.group(1))
    if not isinstance(data, dict):
        raise ValueError("frontmatter is not a mapping")
    for field in ("flags", "features", "includes"):
        items = data.get(field, [])
        if not isinstance(items, list) or not all(isinstance(item, str) for item in items):
            raise ValueError(f"invalid {field} metadata")
    flags = set(data.get("flags", []))
    known = {"onlyStrict", "noStrict", "module", "raw", "async", "generated", "CanBlockIsTrue", "CanBlockIsFalse", "non-deterministic"}
    if flags - known:
        raise ValueError(f"unknown flags: {sorted(flags - known)}")
    if {"onlyStrict", "noStrict"} <= flags or {"raw", "onlyStrict"} <= flags:
        raise ValueError("conflicting execution mode flags")
    negative = data.get("negative")
    if negative is not None and (not isinstance(negative, dict) or negative.get("phase") not in {"parse", "resolution", "runtime"} or not isinstance(negative.get("type"), str)):
        raise ValueError("invalid negative metadata")
    return data


def modes(data):
    flags = data.get("flags", [])
    for flag, mode in [("module", "module"), ("raw", "raw"), ("onlyStrict", "strict"), ("noStrict", "sloppy")]:
        if flag in flags:
            return [mode]
    return ["sloppy", "strict"]


def selected_files(all_files, corpus, pattern):
    files = [
        path
        for path in all_files
        if "_FIXTURE" not in path.name
        and pattern in path.relative_to(corpus / "test").as_posix()
    ]
    if pattern and not files:
        raise ValueError(f"--filter selected no test files: {pattern!r}")
    return files


def classify(reply, negative):
    kind = reply.get("kind")
    if kind == "unsupported":
        return "unsupported"
    if kind in {"timeout", "resource_error"}:
        return "timeout" if kind == "timeout" else "fail"
    if kind in {"harness_error", "worker_error"}:
        return "harness_error"
    if kind == "unclassified_parse_error":
        # A subset parser may reject valid grammar. This cannot establish the
        # required early error of a negative test.
        return "unsupported" if negative else "fail"
    if negative:
        return "pass" if reply.get("phase") == negative["phase"] and kind == negative["type"] else "fail"
    return "pass" if kind == "ok" else "fail"


def instruction_budget(data, default):
    """Keep standard tail-call conformance probes within a bounded budget."""
    if "tail-call-optimization" in data.get("features", []):
        return max(default, TAIL_CALL_INSTRUCTION_BUDGET)
    return default


class Worker:
    def __init__(self, executable, timeout):
        self.executable = executable
        self.timeout = timeout
        self.process = None

    def close(self):
        if self.process is not None:
            # The group also contains any isolated regex helper. This is needed
            # for crashes/whole-case timeouts outside the regex API deadline.
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.process.wait()
            self.process.stdin.close()
            self.process.stdout.close()
            self.process = None

    def exchange(self, payload, timeout):
        process = self.process
        outgoing = memoryview(payload)
        result = bytearray()
        deadline = time.monotonic() + timeout
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            if outgoing:
                selector.register(process.stdin, selectors.EVENT_WRITE)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("whole-case wall deadline exceeded")
                for key, _ in selector.select(remaining):
                    if key.fileobj is process.stdin:
                        size = os.write(process.stdin.fileno(), outgoing[:65536])
                        outgoing = outgoing[size:]
                        if not outgoing:
                            selector.unregister(process.stdin)
                    else:
                        chunk = os.read(process.stdout.fileno(), 65536)
                        if not chunk:
                            raise EOFError(f"adapter exited with code {process.poll()}")
                        result.extend(chunk)
                        if len(result) > 1024 * 1024:
                            raise ValueError("adapter response exceeds limit")
                        if b"\n" in result:
                            return json.loads(result)

    def run(self, request):
        try:
            if self.process is None:
                self.process = subprocess.Popen([str(self.executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, start_new_session=True, bufsize=0)
                os.set_blocking(self.process.stdin.fileno(), False)
                os.set_blocking(self.process.stdout.fileno(), False)
                if self.exchange(b"", 5) != {"ready": 1}:
                    raise ValueError("invalid adapter handshake")
            return self.exchange((json.dumps(request, ensure_ascii=True) + "\n").encode(), self.timeout)
        except TimeoutError as error:
            self.close()
            return {"kind": "timeout", "message": str(error)}
        except (EOFError, BrokenPipeError) as error:
            self.close()
            return {"kind": "crash", "message": str(error)}
        except (OSError, ValueError) as error:
            self.close()
            return {"kind": "harness_error", "message": str(error)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=ROOT / "development/browser_core/reference/test262")
    parser.add_argument("--adapter", type=Path, default=ROOT / "target/debug/bluejs-test262")
    parser.add_argument("--output", type=Path, default=ROOT / "target/test262")
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--timeout", type=float, default=2)
    parser.add_argument("--instruction-budget", type=int, default=100_000)
    parser.add_argument("--filter", default="", help="path substring; reports clearly identify partial runs")
    parser.add_argument("--fetch", action="store_true")
    args = parser.parse_args()
    if args.fetch:
        fetch(args.corpus)
    marker = args.corpus / ".bluejs-snapshot.json"
    if not marker.is_file() or json.loads(marker.read_text()) != SNAPSHOT:
        parser.error("corpus lacks matching verified snapshot; use --fetch with an absent destination")
    manifest_bytes = (args.corpus / ".bluejs-manifest.json").read_bytes()
    if hashlib.sha256(manifest_bytes).hexdigest() != SNAPSHOT["manifest_sha256"]:
        parser.error("corpus manifest differs from pinned archive")
    manifest = json.loads(manifest_bytes)
    for relative, expected in manifest.items():
        path = args.corpus / relative
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            parser.error(f"corpus file differs from pinned archive: {relative}")
    extras = {path.relative_to(args.corpus).as_posix() for path in (args.corpus / "test").rglob("*.js")} - manifest.keys()
    if extras:
        parser.error(f"untracked test files in corpus: {sorted(extras)}")
    if not args.adapter.is_file() or args.jobs < 1 or args.timeout <= 0 or args.instruction_budget < 1:
        parser.error("build the adapter and provide positive jobs, timeout, and instruction budget")
    args.output.mkdir(parents=True, exist_ok=True)
    all_files = sorted((args.corpus / "test").rglob("*.js"))
    fixtures = [path for path in all_files if "_FIXTURE" in path.name]
    try:
        files = selected_files(all_files, args.corpus, args.filter)
    except ValueError as error:
        parser.error(str(error))
    counters = collections.Counter()
    features = collections.defaultdict(collections.Counter)
    groups = collections.defaultdict(collections.Counter)
    workers = []
    local = threading.local()
    lock = threading.Lock()
    start = time.monotonic()

    def run_file(path):
        relative = path.relative_to(args.corpus / "test").as_posix()
        source = path.read_bytes().decode("utf-8")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        try:
            data = metadata(source)
            for include in data.get("includes", []):
                if Path(include).is_absolute() or ".." in Path(include).parts or not (args.corpus / "harness" / include).is_file():
                    raise ValueError(f"missing/invalid harness include {include}")
        except (ValueError, yaml.YAMLError) as error:
            return [{"path": relative, "mode": "metadata", "status": "harness_error", "message": str(error), "sha256": digest}]
        if not hasattr(local, "worker"):
            local.worker = Worker(args.adapter.resolve(), args.timeout)
            with lock:
                workers.append(local.worker)
        results = []
        harness_sources = [
            (args.corpus / "harness" / include).read_text(encoding="utf-8")
            for include in data.get("includes", [])
            if include not in NATIVE_INCLUDES
        ]
        for mode in modes(data):
            negative = data.get("negative")
            reply = local.worker.run({"source": source, "mode": mode, "includes": data.get("includes", []), "harness_sources": harness_sources, "asynchronous": "async" in data.get("flags", []), "parse_only": bool(negative and negative["phase"] == "parse"), "is_html_dda": "IsHTMLDDA" in data.get("features", []), "instruction_budget": instruction_budget(data, args.instruction_budget)})
            results.append({"path": relative, "mode": mode, "status": classify(reply, negative), "expected": negative, "actual": reply, "features": data.get("features", []), "flags": data.get("flags", []), "sha256": digest})
        return results

    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool, (args.output / "results.jsonl").open("w") as output:
            for index, results in enumerate(pool.map(run_file, files), 1):
                for result in results:
                    output.write(json.dumps(result, ensure_ascii=True) + "\n")
                    counters[result["status"]] += 1
                    for feature in result.get("features", []):
                        features[feature][result["status"]] += 1
                    groups[result["path"].split("/")[0]][result["status"]] += 1
                if index % 1000 == 0:
                    output.flush()
                    print(f"{index}/{len(files)} files: {dict(counters)}", flush=True)
    finally:
        for worker in workers:
            worker.close()
    report = {"snapshot": SNAPSHOT, "adapter_sha256": hashlib.sha256(args.adapter.read_bytes()).hexdigest(), "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "regex_worker_sha256": hashlib.sha256(args.adapter.with_name("bluejs-regexp-worker").read_bytes()).hexdigest(), "complete_inventory": not args.filter, "filter": args.filter, "discovered_js": len(all_files), "fixture_resources": len(fixtures), "test_files": len(files), "scheduled_modes": sum(counters.values()), "results": counters, "groups": groups, "features": features, "elapsed_seconds": round(time.monotonic() - start, 3), "timeout_seconds": args.timeout, "instruction_budget": args.instruction_budget, "tail_call_instruction_budget": TAIL_CALL_INSTRUCTION_BUDGET, "jobs": args.jobs, "limitations": ["module host and await execution unavailable", "unclassified parser rejections cannot pass negative tests", "harness sources still require supported grammar and APIs", "native overrides for sta.js, assert.js, propertyHelper.js, and isConstructor.js; raw tests receive no harness", "each mode has a bounded interpreter instruction budget; tail-call fixtures receive at least the recorded tail-call budget"]}
    (args.output / "summary.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps({key: report[key] for key in ("test_files", "scheduled_modes", "results", "elapsed_seconds")}, indent=2))
    return 0 if counters["pass"] == sum(counters.values()) else 1


if __name__ == "__main__":
    raise SystemExit(main())
