#!/usr/bin/env python3
# Copyright 2026 Google LLC
# SPDX-License-Identifier: Apache-2.0
"""Review and apply one revision-bound Google Docs text replacement (stdlib).

Only the gws executable communicates with Google. Plans contain data, never
commands. See README.md for the intentionally limited structural support.
"""

import argparse
from contextlib import contextmanager
import difflib
import hashlib
import hmac
import json
import math
import os
import re
import stat
import subprocess
import sys
import tempfile


MAX_PLAN = 1024 * 1024
MAX_TEXT = 16 * 1024
MAX_DOCUMENT = 16 * 1024 * 1024
PLAN_KEYS = {
    "version", "document_id", "tab_id", "revision_id", "source_sha256",
    "find", "replacement", "expected_occurrences", "target", "diff", "digest",
}


class Refusal(Exception):
    """A fixed, safe message; never construct one from subprocess output."""


class Ambiguous(Refusal):
    """A submission may have applied; never retry automatically."""


def require(condition, message):
    if not condition:
        raise Refusal(message)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=True, allow_nan=False).encode("utf-8")


def sha256(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def utf16(text):
    return len(text.encode("utf-16-le")) // 2


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate JSON keys are not supported.")
        result[key] = value
    return result


def parse_json(data):
    try:
        return json.loads(data, object_pairs_hook=unique_object,
                          parse_constant=lambda _: invalid_json())
    except (ValueError, UnicodeError, RecursionError):
        raise Refusal("Invalid UTF-8 JSON; regenerate the plan or check gws.") from None


def invalid_json():
    raise Refusal("Non-finite JSON numbers are not supported.")


def identifier(value, tab=False):
    pattern = r"[A-Za-z0-9_.-]{1,200}" if tab else r"[A-Za-z0-9_-]{1,200}"
    require(isinstance(value, str) and re.fullmatch(pattern, value) is not None
            and ".." not in value, "Invalid document or tab ID; supply an ID, not a URL.")
    return value


def revision(value):
    require(isinstance(value, str) and 0 < len(value) <= 4096
            and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value),
            "Missing or invalid revision; read an editable document again.")
    return value


def text_input(value, allow_empty=False):
    require(isinstance(value, str), "Find and replacement must be UTF-8 text.")
    require(allow_empty or bool(value), "Find text must not be empty.")
    # Docs can strip these or treat them as structural edits. Reject rather
    # than silently submit a replacement different from the reviewed text.
    require(not any(
        ord(c) < 32 or 127 <= ord(c) <= 159
        or 0xD800 <= ord(c) <= 0xF8FF or 0xFFF9 <= ord(c) <= 0xFFFF
        or ord(c) in (0x2028, 0x2029)
        or 0xF0000 <= ord(c) <= 0xFFFFD
        or 0x100000 <= ord(c) <= 0x10FFFD
        for c in value
    ), "Unsupported structural/control character; use single-paragraph plain text.")
    require(len(value.encode("utf-8")) <= MAX_TEXT, "Text input exceeds 16 KiB.")
    return value


@contextmanager
def confined_parent(path):
    """Hold directory descriptors so symlink swaps cannot redirect file I/O.

    All symlinks (even inward ones) are rejected. POSIX openat/O_NOFOLLOW is
    required; fail closed on platforms without it.
    """
    require(isinstance(path, str) and path and not path.startswith("/")
            and "\\" not in path and ":" not in path
            and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in path),
            "Use a relative path within the current directory.")
    parts = path.split("/")
    require(all(p not in ("", ".", "..") for p in parts),
            "Path traversal and empty path components are not allowed.")
    require(hasattr(os, "O_NOFOLLOW") and os.open in os.supports_dir_fd,
            "Safe file access requires a POSIX platform with O_NOFOLLOW.")
    descriptors = []
    try:
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
        descriptors.append(os.open(".", flags))
        for part in parts[:-1]:
            descriptors.append(os.open(part, flags, dir_fd=descriptors[-1]))
        yield descriptors[-1], parts[-1]
    except OSError:
        raise Refusal(
            "Cannot access path safely; check parents, symlinks and permissions."
        ) from None
    finally:
        for descriptor in reversed(descriptors):
            os.close(descriptor)


def read_file(path, limit):
    with confined_parent(path) as (parent, name):
        descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                             dir_fd=parent)
        with os.fdopen(descriptor, "rb") as stream:
            require(stat.S_ISREG(os.fstat(stream.fileno()).st_mode),
                    "Input must be a regular file.")
            data = stream.read(limit + 1)
    require(len(data) <= limit, "Input file exceeds the supported size limit.")
    try:
        return data.decode("utf-8")
    except UnicodeError:
        raise Refusal("Input file must be valid UTF-8.") from None


def unused_output(parent, name):
    try:
        os.stat(name, dir_fd=parent, follow_symlinks=False)
    except FileNotFoundError:
        return
    raise Refusal("Output already exists; choose a new plan path.")


def write_plan(parent, name, plan):
    data = json.dumps(plan, ensure_ascii=True, sort_keys=True, indent=2) + "\n"
    require(len(data.encode()) <= MAX_PLAN, "Plan exceeds 1 MiB; use a smaller patch.")
    # Exclusive creation prevents overwrites/hardlink attacks; plans are private.
    descriptor = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         0o600, dir_fd=parent)
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


class Gws:
    def __init__(self, timeout):
        self.timeout = timeout
        self.diagnostics = False
        self.mutation_state = "not_attempted"

    def call(self, method, document_id, request=None):
        params = {"documentId": document_id}
        if method == "get":
            params.update(includeTabsContent=True, suggestionsViewMode="SUGGESTIONS_INLINE")
        args = ["gws", "docs", "documents", method,
                "--params", canonical(params).decode(), "--format", "json"]
        if request is not None:
            args += ["--json", canonical(request).decode()]
        try:
            # Inherit gws auth/Model Armor settings. Raw diagnostics never reach
            # the terminal (they may contain document text or credentials).
            with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as diagnostics:
                result = subprocess.run(
                    args, stdin=subprocess.DEVNULL, stdout=output,
                    stderr=diagnostics, timeout=self.timeout, check=False,
                )
                self.diagnostics |= diagnostics.tell() > 0
                require(result.returncode == 0,
                        "gws failed; check access, revision and Model Armor settings.")
                output.seek(0)
                data = output.read(MAX_DOCUMENT + 1)
            require(len(data) <= MAX_DOCUMENT, "gws response exceeds 16 MiB.")
            value = parse_json(data)
            require(isinstance(value, dict) and "error" not in value,
                    "gws did not return a successful JSON object.")
            return value
        except subprocess.TimeoutExpired:
            raise Refusal("gws timed out; check connectivity and timeout settings.") from None
        except OSError:
            raise Refusal("Cannot execute gws; check installation and PATH.") from None

    def get(self, document_id):
        return self.call("get", document_id)


def walk(value, path=()):
    if isinstance(value, dict):
        yield path, value
        for key, item in value.items():
            yield from walk(item, path + (key,))
    elif isinstance(value, list):
        for index, item in enumerate(value):
            yield from walk(item, path + (index,))


def at(value, path):
    for key in path:
        value = value[key]
    return value


def select_tab(document, document_id, tab_id):
    require(isinstance(document, dict) and document.get("documentId") == document_id,
            "Document identity mismatch.")
    revision(document.get("revisionId"))
    require(document.get("suggestionsViewMode") == "SUGGESTIONS_INLINE",
            "Expected suggestions-inline source; refusing an incomplete view.")
    require(isinstance(document.get("tabs"), list) and document["tabs"],
            "Missing all-tabs content; check gws Docs discovery support.")
    tabs = []

    def visit(items, path):
        require(isinstance(items, list), "Malformed document tabs.")
        for i, item in enumerate(items):
            require(isinstance(item, dict)
                    and isinstance(item.get("tabProperties"), dict)
                    and isinstance(item.get("documentTab"), dict), "Unsupported tab shape.")
            identity = identifier(item["tabProperties"].get("tabId"), tab=True)
            tabs.append((identity, path + (i, "documentTab")))
            if "childTabs" in item:
                visit(item["childTabs"], path + (i, "childTabs"))

    visit(document["tabs"], ("tabs",))
    require(len({identity for identity, _ in tabs}) == len(tabs), "Duplicate tab IDs.")
    if tab_id is None:
        require(len(tabs) == 1, "Multiple tabs; select exactly one with --tab ID.")
        tab_id = tabs[0][0]
    matches = [path for identity, path in tabs if identity == tab_id]
    require(len(matches) == 1, "Selected tab does not exist.")
    return tab_id, matches[0]


def check_supported(tab):
    require(isinstance(tab.get("body"), dict)
            and isinstance(tab["body"].get("content"), list), "Tab body is missing.")
    require(set(tab) <= {
        "body", "headers", "footers", "footnotes", "documentStyle", "namedStyles",
        "lists", "namedRanges", "inlineObjects", "positionedObjects", "bookmarks",
        "suggestedDocumentStyleChanges", "suggestedNamedStylesChanges",
    }, "Unsupported tab region; this example requires a known document structure.")
    for _, obj in walk(tab):
        require(not any(k.startswith("suggested") and v for k, v in obj.items()),
                "Suggested content in selected tab is unsupported; resolve suggestions first.")
        require(not obj.get("namedRanges") and not obj.get("bookmarks"),
                "Named ranges and bookmarks in the selected tab are unsupported.")
        if "paragraph" not in obj:
            continue
        for element in obj["paragraph"]["elements"]:
            kinds = set(element) - {"startIndex", "endIndex"}
            require(len(kinds) == 1 and kinds <= {
                "textRun", "inlineObjectElement", "footnoteReference",
                "horizontalRule", "pageBreak", "columnBreak"},
                "Unsupported paragraph element; rich links, equations and chips are excluded.")


def index_range(value):
    start, end = value.get("startIndex", 0), value.get("endIndex")
    require(type(start) is int and type(end) is int and 0 <= start < end,
            "Invalid structural or paragraph element indices.")
    return start, end


def check_region(region):
    require(isinstance(region, dict) and isinstance(region.get("content"), list),
            "Malformed document region; expected structural content.")


def check_content(content):
    for block in content:
        require(isinstance(block, dict), "Malformed structural block.")
        kinds = set(block) - {"startIndex", "endIndex"}
        require(len(kinds) == 1 and kinds <= {
            "paragraph", "sectionBreak", "table", "tableOfContents"},
            "Unsupported document structure.")
        require(isinstance(block[next(iter(kinds))], dict), "Malformed structural block value.")
        index_range(block)


def check_table(table):
    require(isinstance(table, dict), "Malformed table.")
    require(type(table.get("rows")) is int and table["rows"] > 0
            and type(table.get("columns")) is int and table["columns"] > 0,
            "Malformed table dimensions.")
    rows = table.get("tableRows")
    require(isinstance(rows, list) and len(rows) == table["rows"], "Malformed table rows.")
    for row in rows:
        require(isinstance(row, dict) and isinstance(row.get("tableCells"), list)
                and 0 < len(row["tableCells"]) <= table["columns"], "Malformed table row.")
        for cell in row["tableCells"]:
            check_region(cell)


def check_paragraph(block):
    paragraph = block["paragraph"]
    require(isinstance(paragraph, dict)
            and isinstance(paragraph.get("elements"), list)
            and paragraph["elements"], "Malformed paragraph.")
    require(isinstance(paragraph.get("paragraphStyle", {}), dict),
            "Malformed paragraph style.")
    cursor, paragraph_end = index_range(block)
    for element in paragraph["elements"]:
        require(isinstance(element, dict), "Malformed paragraph element.")
        start, end = index_range(element)
        require(start == cursor, "Non-contiguous paragraph indices.")
        kinds = set(element) - {"startIndex", "endIndex"}
        # Extra siblings of textRun would otherwise disappear in normalization.
        # Unrecognized non-text unions are retained whole in unselected tabs.
        require(len(kinds) == 1 and isinstance(element[next(iter(kinds))], dict),
                "Malformed or unsupported paragraph element fields.")
        if "textRun" in element:
            run = element["textRun"]
            require(isinstance(run.get("content"), str)
                    and isinstance(run.get("textStyle", {}), dict), "Malformed text run.")
            require(utf16(run["content"]) == end - start,
                    "Text run does not match its UTF-16 indices.")
        cursor = end
    require(paragraph_end == cursor, "Paragraph end index mismatch.")


def check_document_structure(document):
    """Validate every region/range the normalizer interprets, in every tab.

    Unknown metadata is retained unchanged. Unknown structural blocks or
    text-element siblings that cannot be retained safely are refused.
    """
    for _, obj in walk(document):
        if "documentTab" in obj:
            require(isinstance(obj["documentTab"], dict) and "body" in obj["documentTab"],
                    "Missing document tab body.")
        if "body" in obj:
            check_region(obj["body"])
        for group in ("headers", "footers", "footnotes"):
            if group in obj:
                require(isinstance(obj[group], dict), "Malformed document region map.")
                for region in obj[group].values():
                    check_region(region)
        if isinstance(obj.get("content"), list):
            check_content(obj["content"])
        if "paragraph" in obj:
            check_paragraph(obj)
        if "table" in obj:
            check_table(obj["table"])
        if "tableOfContents" in obj:
            check_region(obj["tableOfContents"])
        if "sectionBreak" in obj:
            require(isinstance(obj["sectionBreak"], dict)
                    and isinstance(obj["sectionBreak"].get("sectionStyle", {}), dict),
                    "Malformed section break.")


def check_document_budget(document):
    """Bound character/style expansion before constructing canonical atoms."""
    expanded_bytes = 0
    characters = 0
    for _, obj in walk(document):
        if "textRun" not in obj:
            continue
        run = obj["textRun"]
        require(isinstance(run, dict) and isinstance(run.get("content"), str),
                "Malformed text run.")
        length = len(run["content"])
        characters += length
        metadata = {k: v for k, v in run.items() if k != "content"}
        expanded_bytes += length * (len(canonical(metadata)) + 64)
        require(characters <= 250000 and expanded_bytes <= MAX_DOCUMENT,
                "Document is too large for safe text/style verification.")


def normalized(document):
    # This gate is part of normalization itself, so no caller can accidentally
    # compare discarded text-run indices before validating the complete source.
    check_document_structure(document)
    return _normalized(document)


def _normalized(value, path=()):
    """Canonical semantic shape; text-run splitting is not a style change."""
    if isinstance(value, list):
        return [_normalized(v, path + (i,)) for i, v in enumerate(value)]
    if not isinstance(value, dict):
        return value
    result = {}
    for key, item in value.items():
        if not path and key in ("revisionId", "_sanitization"):
            continue
        # Google refreshes this temporary URL on reads; keep all other image
        # metadata, including sourceUri, object IDs, dimensions and crop data.
        if key == "contentUri" and path and path[-1] == "imageProperties":
            continue
        if key == "elements" and path and path[-1] == "paragraph":
            elements = []
            for element in item:
                if "textRun" in element:
                    run = element["textRun"]
                    metadata = {k: v for k, v in run.items() if k != "content"}
                    metadata.setdefault("textStyle", {})
                    for char in run["content"]:
                        elements.append({"text": char, "format": metadata})
                else:
                    elements.append(_normalized(element, path + (key,)))
            result[key] = elements
        else:
            result[key] = _normalized(item, path + (key,))
    return result


def locate(document, tab_path, find):
    """Count overlapping occurrences in every text segment of the chosen tab."""
    tab = at(document, tab_path)
    matches = []
    for path, block in walk(tab):
        if "paragraph" not in block:
            continue
        # Only top-level body paragraphs are editable. Tables, TOCs, headers,
        # footers and footnotes still participate in ambiguity detection.
        supported = (len(path) == 3 and path[:2] == ("body", "content")
                     and isinstance(path[2], int))
        cursor = block.get("startIndex", 0)
        tokens = []
        indices = []
        for element in block["paragraph"]["elements"]:
            if "textRun" in element:
                for char in element["textRun"]["content"]:
                    tokens.append(char)
                    indices.append(cursor)
                    cursor += utf16(char)
            else:
                tokens.append("\ufffc")
                indices.append(cursor)
                cursor = element["endIndex"]
        text = "".join(tokens)
        offset = text.find(find)
        while offset != -1:
            matches.append((supported, tab_path + path + ("paragraph", "elements"),
                            offset, indices[offset]))
            offset = text.find(find, offset + 1)
    require(len(matches) == 1,
            "Expected exactly one match in the selected tab; found zero or multiple.")
    supported, path, offset, start = matches[0]
    require(supported, "Target is outside a supported body paragraph.")
    return path, offset, start


def review_diff(find, replacement):
    return "".join(difflib.unified_diff(
        [find + "\n"], [replacement + "\n"], fromfile="before", tofile="after",
    ))


def build_plan(document, document_id, tab_id, find, replacement):
    identifier(document_id)
    text_input(find)
    text_input(replacement, allow_empty=True)
    require(find != replacement, "No-op replacement; choose different text.")
    tab_id, tab_path = select_tab(document, document_id, tab_id)
    check_document_budget(document)
    source = normalized(document)
    check_supported(at(document, tab_path))
    _, _, start = locate(document, tab_path, find)
    result = {
        "version": 1, "document_id": document_id, "tab_id": tab_id,
        "revision_id": document["revisionId"], "source_sha256": sha256(source),
        "find": find, "replacement": replacement, "expected_occurrences": 1,
        "target": {"start_index": start, "end_index": start + utf16(find)},
        "diff": review_diff(find, replacement),
    }
    result["digest"] = sha256(result)
    return result


def validate_plan(plan):
    require(isinstance(plan, dict) and set(plan) == PLAN_KEYS, "Unsupported plan schema.")
    require(type(plan["version"]) is int and plan["version"] == 1,
            "Unsupported plan version.")
    require(type(plan["expected_occurrences"]) is int and plan["expected_occurrences"] == 1,
            "Plan must specify exactly one replacement.")
    identifier(plan["document_id"])
    identifier(plan["tab_id"], tab=True)
    revision(plan["revision_id"])
    text_input(plan["find"])
    text_input(plan["replacement"], allow_empty=True)
    require(plan["find"] != plan["replacement"], "No-op replacement.")
    target = plan["target"]
    require(isinstance(target, dict) and set(target) == {"start_index", "end_index"}
            and all(type(v) is int and v >= 1 for v in target.values())
            and target["end_index"] - target["start_index"] == utf16(plan["find"]),
            "Invalid UTF-16 target range.")
    require(plan["diff"] == review_diff(plan["find"], plan["replacement"]),
            "Plan diff does not match its replacement.")
    for field in ("digest", "source_sha256"):
        require(isinstance(plan[field], str) and re.fullmatch(r"[0-9a-f]{64}", plan[field]),
                "Invalid SHA-256 field.")
    payload = {k: v for k, v in plan.items() if k != "digest"}
    require(hmac.compare_digest(plan["digest"], sha256(payload)),
            "Plan digest mismatch; regenerate and review a fresh plan.")
    return plan


def request_body(plan):
    return {
        "writeControl": {"requiredRevisionId": plan["revision_id"]},
        "requests": [{"replaceAllText": {
            "containsText": {"text": plan["find"], "matchCase": True, "searchByRegex": False},
            "replaceText": plan["replacement"],
            "tabsCriteria": {"tabIds": [plan["tab_id"]]},
        }}],
    }


def verify(before, after, plan, write_revision):
    _, tab_path = select_tab(before, plan["document_id"], plan["tab_id"])
    select_tab(after, plan["document_id"], plan["tab_id"])
    require(after["revisionId"] == write_revision, "Post-write revision mismatch.")
    check_document_budget(after)
    expected, actual = normalized(before), normalized(after)
    check_supported(at(after, tab_path))
    path, offset, _ = locate(before, tab_path, plan["find"])
    end = plan["target"]["end_index"]
    delta = utf16(plan["replacement"]) - utf16(plan["find"])
    # Indices in the body shift; headers/footers/footnotes have separate indices.
    for _, obj in walk(at(expected, tab_path)["body"]):
        for key in ("startIndex", "endIndex"):
            if key in obj and obj[key] >= end:
                obj[key] += delta
    replacement = [{"text": c} for c in plan["replacement"]]
    expected_elements = at(expected, path)
    expected_elements[offset:offset + len(plan["find"])] = replacement
    actual_elements = at(actual, path)
    # Formatting within newly inserted text is Google-controlled. Verify its
    # exact characters but compare formatting of every untouched character.
    for i in range(offset, offset + len(replacement)):
        require(i < len(actual_elements) and "text" in actual_elements[i],
                "Replacement missing from expected location.")
        actual_elements[i] = {"text": actual_elements[i]["text"]}
    require(actual == expected, "Post-write text, structure or untouched style mismatch.")


def apply_plan(plan, gws):
    before = gws.get(plan["document_id"])
    rebuilt = build_plan(before, plan["document_id"], plan["tab_id"],
                         plan["find"], plan["replacement"])
    require(rebuilt == plan,
            "Source revision or content changed; regenerate and review a new plan.")
    try:
        gws.mutation_state = "attempted"
        reply = gws.call("batchUpdate", plan["document_id"], request_body(plan))
        require(reply.get("documentId") == plan["document_id"], "Write identity mismatch.")
        replies = reply.get("replies")
        require(isinstance(replies, list) and len(replies) == 1
                and isinstance(replies[0], dict), "Missing replacement reply.")
        change = replies[0].get("replaceAllText")
        require(isinstance(change, dict) and type(change.get("occurrencesChanged")) is int
                and change["occurrencesChanged"] == 1, "Replacement count was not exactly one.")
        control = reply.get("writeControl")
        require(isinstance(control, dict), "Missing write revision.")
        write_revision = revision(control.get("requiredRevisionId"))
        require(write_revision != plan["revision_id"], "Write revision did not advance.")
        after = gws.get(plan["document_id"])
        verify(before, after, plan, write_revision)
        gws.mutation_state = "confirmed"
    except (Refusal, OSError, ValueError, KeyError, TypeError, IndexError, RecursionError,
            KeyboardInterrupt):
        raise Ambiguous(
            "Write may have applied; verification did not establish success. "
            "Keep the original plan, inspect the document and revision, and do not "
            "blindly retry. A concurrent revision rejection requires a newly reviewed plan."
        ) from None
    return {"status": "applied", "digest": plan["digest"], "revision_id": write_revision}


class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise Refusal("Invalid arguments; use --help for supported options.")


def silence_failed_stdout():
    """Prevent a failed buffered write from being retried at interpreter exit."""
    try:
        with open(os.devnull, "w") as sink:
            os.dup2(sink.fileno(), sys.stdout.fileno())
    except (OSError, ValueError, AttributeError):
        pass


def report_failure(error, mutation_state):
    if mutation_state != "not_attempted":
        message = (
            "Write was confirmed, but final reporting failed. "
            if mutation_state == "confirmed" else
            "Write may have applied; verification did not establish success. "
        )
        outcome = {
            "status": "ambiguous", "mutation_state": mutation_state,
            "message": message + "Keep the original plan, inspect the document and revision, "
            "and do not blindly retry. Further changes require a newly reviewed plan.",
        }
        code = 3
    else:
        if isinstance(error, KeyboardInterrupt):
            message = "Interrupted before submission."
        elif isinstance(error, Refusal):
            message = str(error)
        else:
            message = "Malformed source or inaccessible file; check inputs and regenerate."
        outcome = {"status": "refused", "message": message}
        code = 2
    try:
        print(json.dumps(outcome), file=sys.stderr, flush=True)
    except (OSError, KeyboardInterrupt):
        # Neither an unavailable diagnostic channel nor another interruption
        # changes whether a mutation was attempted.
        pass
    return code


def main(argv=None):
    parser = Parser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    plan_parser = commands.add_parser("plan", help="Read a document and create a reviewable plan")
    plan_parser.add_argument("--document", required=True)
    plan_parser.add_argument(
        "--tab", help="Exact tab ID (required when there is more than one tab)")
    plan_parser.add_argument("--find", required=True, help="Relative UTF-8 input file")
    plan_parser.add_argument("--replacement", required=True, help="Relative UTF-8 input file")
    plan_parser.add_argument("--out", required=True, help="New relative plan file")
    apply_parser = commands.add_parser("apply", help="Validate, apply once, and verify")
    apply_parser.add_argument("--plan", required=True)
    apply_parser.add_argument("--dry-run", action="store_true",
                              help="Offline plan validation and request preview; no gws calls")
    for command in (plan_parser, apply_parser):
        command.add_argument("--timeout", type=float, default=60,
                             help="Per-gws-call timeout in seconds (default: 60)")
    gws = None
    reporting = False
    try:
        args = parser.parse_args(argv)
        require(math.isfinite(args.timeout) and 0 < args.timeout <= 600,
                "Timeout must be greater than zero and at most 600 seconds.")
        gws = Gws(args.timeout)
        if args.command == "plan":
            identifier(args.document)
            if args.tab is not None:
                identifier(args.tab, tab=True)
            find = text_input(read_file(args.find, MAX_TEXT))
            replacement = text_input(read_file(args.replacement, MAX_TEXT), allow_empty=True)
            with confined_parent(args.out) as (parent, name):
                unused_output(parent, name)
                plan = build_plan(
                    gws.get(args.document), args.document, args.tab, find, replacement)
                write_plan(parent, name, plan)
            result = {"status": "planned", "digest": plan["digest"],
                      "message": "Review the plan and digest before applying."}
        else:
            plan = validate_plan(parse_json(read_file(args.plan, MAX_PLAN)))
            if args.dry_run:
                result = {"status": "preview", "digest": plan["digest"],
                          "diff": plan["diff"], "request": request_body(plan)}
            else:
                result = apply_plan(plan, gws)
        if gws.diagnostics:
            result["gws_diagnostics"] = (
                "gws reported diagnostics; check gws and Model Armor settings. "
                "Raw output was suppressed to protect document content."
            )
        reporting = True
        print(json.dumps(result, ensure_ascii=True, sort_keys=True), flush=True)
        return 0
    except (Refusal, OSError, ValueError, KeyError, TypeError, IndexError,
            RecursionError, KeyboardInterrupt) as error:
        if reporting:
            silence_failed_stdout()
        mutation_state = gws.mutation_state if gws is not None else "not_attempted"
        return report_failure(error, mutation_state)


if __name__ == "__main__":
    sys.exit(main())
