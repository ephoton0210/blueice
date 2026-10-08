# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Source references for isolated shared boundaries, with current source hashes.

These references add conservative dependency edges. They never authorize
omitting a measured target and do not claim to resolve Rust dynamic dispatch.
"""
import hashlib
import re
from pathlib import Path

from .rust_source import mask_rust


BOUNDARIES = {
    "classification": ("object_capabilities", "value_capabilities", "has_construct",
                       "is_callable", "is_constructor", "to_boolean", "typeof_value"),
    "temporal-receiver": ("validate_temporal_receiver",),
    "temporal-kind-table": ("temporal_receiver_kind",),
    "host-registration": ("host_registration_index", "host_function_index"),
    "promise-reactions": ("perform_promise_then", "add_then_reaction"),
    "private-declarations": ("private_element_or_throw",),
    "for-in": ("for_in_iterator", "is_for_in_record", "for_in_step", "for_in_next_key"),
    "retained-payload": ("add_payload",),
    "list-format": ("resolve_list_format", "create_list_format", "list_format_values", "list_format_parts"),
    "duration-format": ("resolve_duration_format", "duration_record", "create_duration_format"),
}


def shared_function_graph(root):
    root = Path(root)
    names = {name: boundary for boundary, symbols in BOUNDARIES.items() for name in symbols}
    definitions, references, hashes = {}, [], {}
    for path in sorted((root / "backend/bluejs/src").rglob("*.rs")):
        text = path.read_text()
        masked = mask_rust(text)
        source = path.relative_to(root).as_posix()
        hashes[source] = hashlib.sha256(text.encode()).hexdigest()
        declarations = list(re.finditer(r"\bfn\s+(\w+)\s*(?:<[^;{}]*>)?\s*\(", masked))
        definition_positions = {match.start(1) for match in declarations}
        scopes = []
        for match in declarations:
            if match.group(1) in names:
                definitions.setdefault(match.group(1), []).append({
                    "source": source, "line": text.count("\n", 0, match.start()) + 1,
                    "source_sha256": hashes[source]})
            opening = masked.find("{", match.end())
            semicolon = masked.find(";", match.end(), opening)
            if opening >= 0 and semicolon < 0:
                depth, end = 1, opening + 1
                while depth and end < len(masked):
                    depth += (masked[end] == "{") - (masked[end] == "}")
                    end += 1
                scopes.append((opening, end, match.group(1)))
        for name in names:
            for match in re.finditer(r"\b" + re.escape(name) + r"\s*\(", masked):
                if match.start() in definition_positions:
                    continue
                caller = next((name for opening, end, name in reversed(scopes)
                               if opening < match.start() < end), None)
                references.append({"symbol": name, "source": source, "function": caller,
                                   "line": text.count("\n", 0, match.start()) + 1,
                                   "source_sha256": hashes[source]})
    return [{"symbol": name, "boundary": names[name], "definitions": locations,
             "references": [ref for ref in references if ref["symbol"] == name],
             "resolution": "lexical source reference; all matching definitions retained"}
            for name, locations in sorted(definitions.items())]


def shared_function_edges(functions):
    edges = set()
    for function in functions:
        for definition in function["definitions"]:
            for reference in function["references"]:
                if definition["source"] != reference["source"]:
                    edges.add((definition["source"], reference["source"], "shared-function"))
    return [{"source": source, "target": target, "kind": kind} for source, target, kind in sorted(edges)]


def body_contract(text):
    """Hash signatures, types, imports, attributes and data outside fn bodies."""
    masked, parts, position = mask_rust(text), [], 0
    for match in re.finditer(r"\bfn\s+\w+\s*(?:<[^;{}]*>)?\s*\(", masked):
        if match.start() < position:
            continue
        opening = masked.find("{", match.end())
        if opening < 0 or masked.find(";", match.end(), opening) >= 0:
            continue
        depth, end = 1, opening + 1
        while depth and end < len(masked):
            depth += (masked[end] == "{") - (masked[end] == "}")
            end += 1
        parts.append(text[position:opening + 1])
        position = end - 1
    parts.append(text[position:])
    literals = []
    shared = re.sub(r"\s+", "", mask_rust("".join(parts), literals))
    return hashlib.sha256((shared + repr(literals)).encode()).hexdigest()


def verified_body_contracts(sources, snapshot):
    """Create contracts only from source texts belonging to a complete gate."""
    symbols = {symbol for names in BOUNDARIES.values() for symbol in names}
    result = {}
    for source, text in sources.items():
        if not source.startswith("backend/bluejs/src/"):
            continue
        definitions = set(re.findall(r"\bfn\s+(\w+)\s*", mask_rust(text)))
        if symbols & definitions:
            result[source] = {"structural_sha256": body_contract(text),
                              "source_sha256": hashlib.sha256(text.encode()).hexdigest(),
                              "snapshot": snapshot}
    return result
