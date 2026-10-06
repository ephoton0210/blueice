# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import json
import mimetypes
import re
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

from .common import load, now
from .coverage import import_baseline, latest_baseline
from .graph import plan
from .report import publish_report
from .runner import validate_impact

STATIC = Path(__file__).resolve().parent / "web"


class Application:
    def __init__(self, manager):
        self.manager = manager
        self.indexing = {"status": "idle"}
        self.index_thread = None
        self.files = None
        self.base = None
        self.cached_plan = None
        self.last_plan_at = 0
        self.cached_partitions = None
        self.partition_snapshot = None
        self.lock = threading.RLock()
        self.token = __import__("secrets").token_urlsafe(32)

    def current_plan(self, refresh=False):
        with self.lock:
            if refresh or self.cached_plan is None or time.monotonic() - self.last_plan_at > 3:
                self.cached_plan = plan(self.manager.graph(), self.files, self.manager.root, self.base)
                self.last_plan_at = time.monotonic()
            return self.cached_plan

    def state(self):
        graph = self.manager.graph()
        current = self.current_plan()
        history = self.manager.history()
        previous = next((r for r in history if r.get("mode") == "impact" and r["status"] == "passed"), None)
        ready = False
        try:
            validate_impact(previous, current)
            ready = True
        except ValueError:
            pass
        from .partitions import partition_plan, passed_partition
        if self.partition_snapshot != current["snapshot"] or self.cached_partitions is None:
            self.cached_partitions = partition_plan(self.manager, self.files, self.base)
            self.partition_snapshot = current["snapshot"]
        partitions = self.cached_partitions
        ready = ready or passed_partition(self.manager, partitions) is not None
        baseline = graph.get("baseline")
        measured = None
        if baseline:
            test262 = baseline["test262"]
            measured = {"path": baseline["path"], "measured_at": baseline["measured_at"],
                        "rust": baseline["rust"], "test262": {k: test262[k] for k in ("scheduled_modes", "results", "elapsed_seconds")},
                        "audit": baseline["audit"]}
        with self.manager.lock:
            active = json.loads(json.dumps(self.manager.active)) if self.manager.active else None
        if active is None and history:
            active = history[0]
        # During a run the source fingerprint is checked by the owning runner.
        return {"token": self.token, "plan": {k: v for k, v in current.items() if k != "edges"},
                "indexing": self.indexing, "graph": {"created_at": graph["created_at"], "tests": len(graph["tests"]),
                "observed_targets": len(graph["observed_targets"]), "observed_edges": sum(e["kind"] == "observed" for e in graph["edges"]),
                "warnings": graph["warnings"]}, "baseline": measured, "active": active,
                "history": [{k: v for k, v in r.items() if k not in ("tasks",)} for r in history],
                "ready_for_full": ready, "workspace": self.manager.workspace,
                "partitions": partitions,
                "jobs": self.manager.jobs, "test_threads": self.manager.test_threads}

    def index(self):
        with self.lock:
            if self.manager.thread and self.manager.thread.is_alive():
                raise ValueError("Finish the current test run before rebuilding the graph")
            if self.index_thread and self.index_thread.is_alive():
                raise ValueError("The graph is already being indexed")
            self.indexing = {"status": "running", "message": "Reading retained measurement", "completed": 0, "total": 1}
            def execute():
                try:
                    def progress(message, completed, total):
                        self.indexing.update(message=message, completed=completed, total=total)
                    graph = import_baseline(latest_baseline(self.manager.root, self.manager.state), self.manager.state, self.manager.root, progress)
                    self.indexing.update(status="passed", message=f"Indexed {len(graph['observed_targets'])} measured targets", finished_at=now())
                    self.current_plan(refresh=True)
                except Exception as error:
                    self.indexing.update(status="failed", message=str(error))
            self.index_thread = threading.Thread(target=execute, daemon=True)
            self.index_thread.start()
            return self.indexing


def make_server(application, port=8765):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def send(self, status, data, content_type="application/json"):
            body = json.dumps(data).encode() if content_type == "application/json" else data
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Content-Security-Policy", "default-src 'self'; style-src 'self'; script-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'")
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            try:
                parsed = urlparse(self.path)
                query = parse_qs(parsed.query)
                if parsed.path == "/api/state":
                    return self.send(200, application.state())
                if parsed.path == "/api/graph":
                    graph = application.manager.graph()
                    sources = sorted({e["source"] for e in graph["edges"]} | {p for p in graph["source_hashes"] if p.endswith(".rs")})
                    return self.send(200, {"sources": sources, "tests": graph["tests"],
                        "edges": [{k: e[k] for k in ("source", "target", "kind")} for e in graph["edges"]],
                        "coverage": graph.get("baseline", {}).get("full_coverage", {}) if graph.get("baseline") else {}})
                if parsed.path == "/api/current-report":
                    path = application.manager.root / "development/browser_core/phase-13-bluejs-engine/TEST262_MACOS_REPORT.md"
                    if not path.is_file():
                        return self.send(404, {"error": "Current report not found"})
                    return self.send(200, path.read_bytes(), "text/plain; charset=utf-8")
                if parsed.path in ("/api/log", "/api/report"):
                    run_id = query.get("run", [""])[0]
                    if not re.fullmatch(r"\d{8}-\d{6}-[a-f0-9]{8}", run_id):
                        raise ValueError("Invalid run identifier")
                    directory = application.manager.state / "runs" / run_id
                    record = load(directory / "run.json")
                    if record is None:
                        return self.send(404, {"error": "Run not found"})
                    if parsed.path == "/api/report":
                        path = directory / "report.md"
                    else:
                        task_id = query.get("task", [""])[0]
                        task = next((t for t in record["tasks"] if t["id"] == task_id), None)
                        if task is None:
                            return self.send(404, {"error": "Task not found"})
                        path = directory / task["log"]
                    if not path.is_file():
                        return self.send(404, {"error": "Artifact not available yet"})
                    # Limit displayed logs; retained files remain complete on disk.
                    with path.open("rb") as stream:
                        if parsed.path == "/api/log":
                            stream.seek(max(0, path.stat().st_size - 200000))
                        return self.send(200, stream.read(), "text/plain; charset=utf-8")
                assets = {"/": "index.html", "/app.js": "app.js", "/style.css": "style.css"}
                if parsed.path in assets:
                    path = STATIC / assets[parsed.path]
                    return self.send(200, path.read_bytes(), mimetypes.guess_type(path)[0] or "application/octet-stream")
                return self.send(404, {"error": "Not found"})
            except (ValueError, RuntimeError, OSError) as error:
                self.send(400, {"error": str(error)})

        def do_POST(self):
            try:
                if self.headers.get("X-BlueJS-Token") != application.token:
                    return self.send(403, {"error": "Open the local dashboard before issuing actions"})
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 <= length <= 65536:
                    raise ValueError("Request body is too large")
                body = json.loads(self.rfile.read(length) or b"{}")
                if self.path == "/api/plan":
                    files = body.get("files")
                    if files is not None and (not isinstance(files, list) or not all(isinstance(p, str) for p in files)):
                        raise ValueError("Files must be a list of repository paths")
                    candidate = plan(application.manager.graph(), files, application.manager.root, body.get("base"))
                    application.files, application.base, application.cached_plan = files, body.get("base"), candidate
                    application.cached_partitions = None
                    return self.send(200, candidate)
                if self.path == "/api/index":
                    return self.send(202, application.index())
                if self.path == "/api/run":
                    if application.index_thread and application.index_thread.is_alive():
                        raise ValueError("Finish indexing before running the selected tests")
                    application.current_plan(refresh=True)
                    return self.send(202, application.manager.start(body.get("mode", "impact"), application.files, application.base, body.get("workspace")))
                if self.path == "/api/cancel":
                    application.manager.cancel()
                    return self.send(200, {"status": "cancelling"})
                if self.path == "/api/publish":
                    run_id = body.get("run", "")
                    if not re.fullmatch(r"\d{8}-\d{6}-[a-f0-9]{8}", run_id):
                        raise ValueError("Invalid run identifier")
                    return self.send(200, {"path": str(publish_report(application.manager, run_id))})
                self.send(404, {"error": "Not found"})
            except (ValueError, RuntimeError, OSError, TypeError) as error:
                self.send(400, {"error": str(error)})

    return ThreadingHTTPServer(("127.0.0.1", port), Handler)
