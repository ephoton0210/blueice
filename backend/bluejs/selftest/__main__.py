# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import argparse
import json
import sys
import time
import webbrowser
from pathlib import Path

from .common import DEFAULT_CORPUS, DEFAULT_STATE, ROOT
from .coverage import import_baseline, latest_baseline
from .graph import plan
from .report import publish_report
from .runner import Manager
from .server import Application, make_server


def main(argv=None):
    parser = argparse.ArgumentParser(description="BlueJS test impact graph and local self-test dashboard")
    commands = parser.add_subparsers(dest="action", required=True)
    for name in ("index", "plan", "run", "serve", "publish"):
        command = commands.add_parser(name)
        command.add_argument("--root", type=Path, default=ROOT)
        command.add_argument("--state", type=Path, default=DEFAULT_STATE)
        if name == "index":
            command.add_argument("--baseline", type=Path)
        if name in ("plan", "run"):
            command.add_argument("--files", nargs="*")
            command.add_argument("--base")
        if name in ("run", "serve"):
            command.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS)
            command.add_argument("--jobs", type=int, default=4)
            command.add_argument("--test-threads", type=int, default=4)
            command.add_argument("--workspace", action="store_true")
        if name == "run":
            command.add_argument("--mode", choices=("impact", "full", "pipeline"), default="impact")
        if name == "serve":
            command.add_argument("--port", type=int, default=8765)
            command.add_argument("--no-open", action="store_true")
        if name == "publish":
            command.add_argument("--run", required=True)
    args = parser.parse_args(argv)
    try:
        if args.action == "index":
            def progress(message, done, total):
                if done % 20 == 0 or done == total:
                    print(f"{message}: {done}/{total}", flush=True)
            graph = import_baseline(args.baseline or latest_baseline(args.root), args.state, args.root, progress)
            print(json.dumps({"observed_targets": len(graph["observed_targets"]), "edges": len(graph["edges"]), "warnings": graph["warnings"]}, indent=2))
            return 0
        manager = Manager(args.root, args.state, getattr(args, "corpus", DEFAULT_CORPUS),
                          getattr(args, "jobs", 4), getattr(args, "test_threads", 4), getattr(args, "workspace", False))
        if args.action == "plan":
            print(json.dumps(plan(manager.graph(), args.files, args.root, args.base), indent=2))
            return 0
        if args.action == "publish":
            print(publish_report(manager, args.run))
            return 0
        if args.action == "run":
            manager.start(args.mode, args.files, args.base)
            previous = None
            try:
                while manager.thread.is_alive():
                    message = manager.active["message"]
                    if message != previous:
                        print(message, flush=True)
                        previous = message
                    manager.thread.join(1)
            except KeyboardInterrupt:
                manager.cancel()
                manager.thread.join(10)
            print(json.dumps(manager.active, indent=2))
            return 0 if manager.active["status"] == "passed" else 1
        application = Application(manager)
        server = make_server(application, args.port)
        url = f"http://127.0.0.1:{server.server_port}"
        print("BlueJS self-test dashboard: " + url, flush=True)
        if not args.no_open:
            webbrowser.open(url)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            manager.cancel()
        finally:
            server.server_close()
        return 0
    except (ValueError, RuntimeError, OSError) as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
