# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

import hashlib
import json
import os
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DEFAULT_STATE = ROOT / "target/bluejs-selftest"
DEFAULT_CORPUS = DEFAULT_STATE / "corpus" / (
    "test262-" + json.loads((ROOT / "backend/bluejs/test262/snapshot.json").read_text())["revision"]
)
METRICS = ("lines", "functions", "regions")


def now():
    return datetime.now(timezone.utc).isoformat()


def digest(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def load(path, default=None):
    path = Path(path)
    return json.loads(path.read_text()) if path.is_file() else default


def save(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + f".{os.getpid()}.tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    temporary.replace(path)


def relative(root, value):
    root = Path(root).resolve()
    path = Path(value)
    path = (path if path.is_absolute() else root / path).resolve()
    if not path.is_relative_to(root):
        raise ValueError("Paths must remain inside the repository")
    return path.relative_to(root).as_posix()


def snapshot(root=ROOT):
    root = Path(root)
    paths = set()
    for directory in (root / "backend", root / "scripts", root / ".cargo"):
        if directory.exists():
            paths.update(p for p in directory.rglob("*") if p.is_file()
                         and p.suffix in (".rs", ".toml", ".py", ".json", ".js", ".css", ".html")
                         and "node_modules" not in p.parts and "__pycache__" not in p.parts)
    paths.update(p for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain")
                 if (p := root / name).is_file())
    hashes = {}
    for path in sorted(paths):
        try:
            hashes[path.relative_to(root).as_posix()] = digest(path)
        except FileNotFoundError:
            # A file removed during a snapshot is represented as absent.
            continue
    identity = hashlib.sha256(json.dumps(hashes, sort_keys=True).encode()).hexdigest()
    return {"identity": identity, "files": hashes}


def checked(command, root=ROOT, env=None):
    result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout[-3000:] or f"Command failed: {command[0]}")
    return result.stdout


def changed_files(root=ROOT, base=None):
    root = Path(root)
    command = ["git", "diff", "--name-only", "-z"]
    if base:
        if base.startswith("-") or not all(c.isalnum() or c in "/._-~^" for c in base):
            raise ValueError("Invalid comparison revision")
        command.append(base)
    command.append("--")
    paths = set(checked(command, root).split("\0"))
    paths.update(checked(["git", "diff", "--cached", "--name-only", "-z", "--"], root).split("\0"))
    paths.update(checked(["git", "ls-files", "--others", "--exclude-standard", "-z"], root).split("\0"))
    return sorted(p for p in paths if p and not p.startswith((".worktrees/", "target/")))


def complete(summary):
    return all(summary[key]["count"] > 0 and summary[key]["count"] == summary[key]["covered"]
               for key in METRICS)


def validate_corpus(corpus, pinned):
    """Check every pinned fixture before spending time building or running Rust."""
    corpus = Path(corpus)
    if load(corpus / ".bluejs-snapshot.json") != pinned:
        raise ValueError("Corpus lacks the pinned snapshot; fetch it into the configured persistent corpus directory")
    manifest_path = corpus / ".bluejs-manifest.json"
    if not manifest_path.is_file() or digest(manifest_path) != pinned["manifest_sha256"]:
        raise ValueError("Corpus manifest differs from the pinned archive")
    manifest = load(manifest_path)
    for name, expected in manifest.items():
        # Reject escaping paths as well as missing or changed fixtures.
        relative(corpus, name)
        path = corpus / name
        if not path.is_file() or digest(path) != expected:
            raise ValueError("Corpus file differs from the pinned archive: " + name)
    extras = {path.relative_to(corpus).as_posix() for path in (corpus / "test").rglob("*.js")} - manifest.keys()
    if extras:
        raise ValueError("Untracked test files in corpus: " + ", ".join(sorted(extras)))
    return {"revision": pinned["revision"], "verified_files": len(manifest), "javascript_cases": 0}


def retain_coverage_reference(graph, current, evidence):
    """Keep the highest verified percentage for comparison, never for completion."""
    previous = graph.get("coverage_reference") or (graph.get("baseline") or {}).get("full_coverage", {})
    reference = {name: dict(value.get("_raw", value)) for name, value in previous.items()}
    for name, value in current.items():
        value = value.get("_raw", value)
        retained = reference.setdefault(name, {})
        for key in METRICS:
            old, new = retained.get(key), value[key]
            if not old or (new["count"] and (not old["count"] or new["covered"] * old["count"] > old["covered"] * new["count"])):
                retained[key] = dict(new)
    graph["coverage_reference"] = reference
    graph["coverage_reference_provenance"] = sorted(set(graph.get("coverage_reference_provenance", [])) | {str(evidence)})
