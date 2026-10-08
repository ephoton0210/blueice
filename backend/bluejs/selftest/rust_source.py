# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Shared lexical Rust inspection; references do not resolve dynamic dispatch."""
import re


def mask_rust(text, literals=None):
    """Keep structural braces while hiding comments and Rust string literals."""
    pattern = re.compile(r'''//[^\n]*|/\*|r(\#*)"|"|'(?:\\(?:x[\da-fA-F]{2}|u\{[\da-fA-F]+\}|.)|[^'\\\n])'|\bfn\b''', re.M)
    result = list(text)
    position = 0
    while match := pattern.search(text, position):
        start, token = match.start(), match.group()
        if token == "fn":
            position = match.end()
            continue
        if token == "/*":
            end, depth = match.end(), 1
            while depth and end < len(text):
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        elif token.startswith("r"):
            delimiter = '"' + match.group(1)
            finish = text.find(delimiter, match.end())
            end = len(text) if finish < 0 else finish + len(delimiter)
        elif token == '"':
            end = match.end()
            while end < len(text):
                if text[end] == "\\":
                    end += 2
                elif text[end] == '"':
                    end += 1
                    break
                else:
                    end += 1
        else:
            end = match.end()
        if literals is not None and not token.startswith("//") and token != "/*":
            literals.append(text[start:end])
        result[start:end] = ["\n" if c == "\n" else " " for c in text[start:end]]
        position = end
    return "".join(result)


def bodies(text, pattern):
    masked, result = mask_rust(text), {}
    for match in re.finditer(pattern, masked):
        opening = masked.find("{", match.end())
        if opening < 0:
            continue
        depth, end = 1, opening + 1
        while depth and end < len(masked):
            depth += (masked[end] == "{") - (masked[end] == "}")
            end += 1
        result.setdefault(match.group(1), []).append(text[match.start():end])
    return result


