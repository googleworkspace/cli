#!/usr/bin/env python3
# Copyright 2026 Google LLC
# SPDX-License-Identifier: Apache-2.0
"""Behavior tests: all gws calls go to an isolated executable, never Google."""

import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("docs_review.py")


def paragraph(parts, start=1):
    elements = []
    cursor = start
    for text, style in parts:
        end = cursor + len(text.encode("utf-16-le")) // 2
        elements.append({
            "startIndex": cursor, "endIndex": end,
            "textRun": {"content": text, "textStyle": style},
        })
        cursor = end
    return {
        "startIndex": start, "endIndex": cursor,
        "paragraph": {
            "elements": elements,
            "paragraphStyle": {"namedStyleType": "NORMAL_TEXT"},
        },
    }


def document(text="Hello world.\n", revision="rev-1"):
    return {
        "documentId": "synthetic-doc", "revisionId": revision,
        "title": "Synthetic review fixture",
        "suggestionsViewMode": "SUGGESTIONS_INLINE",
        "tabs": [{
            "tabProperties": {"tabId": "t.main", "title": "Main", "index": 0},
            "documentTab": {
                "body": {"content": [
                    {"endIndex": 1, "sectionBreak": {
                        "sectionStyle": {"sectionType": "CONTINUOUS"}}},
                    paragraph([(text, {})]),
                ]},
            },
        }],
    }


def body(doc):
    return doc["tabs"][0]["documentTab"]["body"]["content"]


def resign(plan):
    payload = {k: v for k, v in plan.items() if k != "digest"}
    plan["digest"] = hashlib.sha256(json.dumps(
        payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True,
        allow_nan=False,
    ).encode()).hexdigest()
    return plan


# The executable checks the actual argv contract and simulates the remote
# boundary only. Its output fixtures are independent of production helpers.
STUB = r'''
import json, os, pathlib, sys, time
root = pathlib.Path(os.environ["STUB_ROOT"])
args = sys.argv[1:]
with (root / "calls.jsonl").open("a") as f:
    f.write(json.dumps(args) + "\n")
assert args[:2] == ["docs", "documents"], args
assert args[args.index("--format") + 1] == "json", args
params = json.loads(args[args.index("--params") + 1])
assert params["documentId"] == "synthetic-doc", params
method = args[2]
mode = (root / "mode").read_text()
if method == "get":
    assert params["includeTabsContent"] is True, params
    assert params["suggestionsViewMode"] == "SUGGESTIONS_INLINE", params
    name = "after.json" if (root / "submitted").exists() else "before.json"
    if mode == "read-error" or (mode == "verify-error" and name == "after.json"):
        print("secret-token \x1b[31m remote private content", file=sys.stderr)
        sys.exit(1)
    if mode == "armor-block":
        print(json.dumps({"error": "Content blocked by Model Armor"}))
        sys.exit(1)
    if mode == "armor-warn":
        assert os.environ["GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE"] == "synthetic-template"
        print("secret-token \x1b[31m Model Armor warning", file=sys.stderr)
    print((root / name).read_text())
elif method == "batchUpdate":
    request = json.loads(args[args.index("--json") + 1])
    (root / "request.json").write_text(json.dumps(request))
    (root / "submitted").touch()
    if mode == "timeout":
        time.sleep(5)
    if mode in ("conflict", "write-error"):
        print(json.dumps({"error": {"code": 400 if mode == "conflict" else 503,
            "message": "secret-token \x1b[31m private response"}}))
        sys.exit(1)
    if mode == "bad-response":
        print("secret-token malformed")
    else:
        result = {"documentId": "synthetic-doc",
                  "replies": [{"replaceAllText": {"occurrencesChanged": 1}}],
                  "writeControl": {"requiredRevisionId": "rev-2"}}
        if mode == "zero":
            result["replies"][0]["replaceAllText"]["occurrencesChanged"] = 0
        if mode == "two":
            result["replies"][0]["replaceAllText"]["occurrencesChanged"] = 2
        if mode == "missing-reply":
            result["replies"] = []
        if mode == "missing-write-revision":
            result.pop("writeControl")
        print(json.dumps(result))
else:
    raise AssertionError(args)
'''


class ReviewCliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        stub = self.bin / "gws"
        stub.write_text("#!" + sys.executable + "\n" + STUB)
        stub.chmod(0o700)
        # Do not pass real credentials/configuration into any child process.
        self.env = {
            "PATH": str(self.bin) + os.pathsep + os.defpath,
            "STUB_ROOT": str(self.root),
            "PYTHONDONTWRITEBYTECODE": "1",
            "GOOGLE_WORKSPACE_CLI_CONFIG_DIR": str(self.root / "config"),
        }
        self.put("mode", "ok")
        self.put("find.txt", "world")
        self.put("replacement.txt", "reader")
        self.fixture(document(), document("Hello reader.\n", "rev-2"))

    def put(self, name, value):
        (self.root / name).write_text(value, encoding="utf-8")

    def fixture(self, before, after=None):
        self.put("before.json", json.dumps(before))
        self.put("after.json", json.dumps(after or before))

    def cli(self, *args):
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args], cwd=self.root,
            env=self.env, text=True, capture_output=True, timeout=10,
        )

    def plan(self, *extra):
        return self.cli("plan", "--document", "synthetic-doc",
                        "--find", "find.txt", "--replacement", "replacement.txt",
                        "--out", "plan.json", *extra)

    def apply(self, *extra):
        return self.cli("apply", "--plan", "plan.json", *extra)

    def load_plan(self):
        return json.loads((self.root / "plan.json").read_text())

    def calls(self):
        path = self.root / "calls.jsonl"
        return [json.loads(x) for x in path.read_text().splitlines()] if path.exists() else []

    def success(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def refused(self, result, status="refused"):
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("secret-token", result.stderr + result.stdout)
        self.assertNotIn("\x1b", result.stderr + result.stdout)
        try:
            payload = json.loads(result.stderr)
        except ValueError:
            self.fail("Expected a structured refusal, got: " + result.stderr[:200])
        self.assertEqual(payload["status"], status)

    def test_plan_is_deterministic_reviewable_and_reads_only_once(self):
        self.success(self.plan())
        original = (self.root / "plan.json").read_bytes()
        plan = self.load_plan()
        self.assertEqual(plan["version"], 1)
        self.assertEqual(plan["revision_id"], "rev-1")
        self.assertEqual(plan["tab_id"], "t.main")
        self.assertEqual(plan["expected_occurrences"], 1)
        self.assertEqual(plan["target"], {"start_index": 7, "end_index": 12})
        self.assertIn("-world", plan["diff"])
        self.assertIn("+reader", plan["diff"])
        self.assertNotIn("Synthetic review fixture", original.decode())
        self.assertNotIn("Hello", original.decode())
        self.assertEqual(resign(copy.deepcopy(plan)), plan)
        self.assertEqual([c[2] for c in self.calls()], ["get"])
        (self.root / "plan.json").unlink()
        self.success(self.plan())
        self.assertEqual((self.root / "plan.json").read_bytes(), original)

    def test_apply_submits_only_reviewed_revision_and_tab_then_verifies(self):
        self.success(self.plan())
        original = (self.root / "plan.json").read_bytes()
        result = self.success(self.apply())
        self.assertEqual(result["status"], "applied")
        self.assertEqual(json.loads((self.root / "request.json").read_text()), {
            "writeControl": {"requiredRevisionId": "rev-1"},
            "requests": [{"replaceAllText": {
                "containsText": {"text": "world", "matchCase": True,
                                 "searchByRegex": False},
                "replaceText": "reader", "tabsCriteria": {"tabIds": ["t.main"]},
            }}],
        })
        self.assertEqual([c[2] for c in self.calls()],
                         ["get", "get", "batchUpdate", "get"])
        self.assertEqual((self.root / "plan.json").read_bytes(), original)

    def test_zero_multiple_and_overlapping_matches_refuse_without_write(self):
        for text, find in [("Nothing.\n", "world"),
                           ("world world\n", "world"), ("aaa\n", "aa")]:
            with self.subTest(text=text):
                self.fixture(document(text))
                self.put("find.txt", find)
                self.refused(self.plan())
                self.assertFalse((self.root / "plan.json").exists())
        self.assertTrue(all(c[2] == "get" for c in self.calls()))

    def test_unicode_utf16_and_nested_tab_scope(self):
        before = document("😀 café world.\n")
        before["tabs"][0]["childTabs"] = [{
            "tabProperties": {"tabId": "t.child", "title": "Main", "index": 0},
            "documentTab": {"body": {"content": [paragraph([("world\n", {})])]}}
        }]
        after = copy.deepcopy(before)
        after["revisionId"] = "rev-2"
        body(after)[1] = paragraph([("😀 café reader.\n", {})])
        self.fixture(before, after)
        self.refused(self.plan())  # No silent first-tab selection.
        self.success(self.plan("--tab", "t.main"))
        self.assertEqual(self.load_plan()["target"],
                         {"start_index": 9, "end_index": 14})
        self.success(self.apply())
        request = json.loads((self.root / "request.json").read_text())
        self.assertEqual(request["requests"][0]["replaceAllText"]["tabsCriteria"],
                         {"tabIds": ["t.main"]})

    def test_replaces_unicode_in_selected_child_without_touching_parent(self):
        before = document("😀targetZ\n")
        child = copy.deepcopy(before["tabs"][0])
        child["tabProperties"]["tabId"] = "t.child"
        before["tabs"][0]["childTabs"] = [child]
        after = copy.deepcopy(before)
        after["revisionId"] = "rev-2"
        # Preserve Z/newline styles while allowing Google to style inserted text.
        after["tabs"][0]["childTabs"][0]["documentTab"]["body"]["content"][1] = (
            paragraph([("🛰️", {"italic": True}), ("Z\n", {})]))
        self.fixture(before, after)
        self.put("find.txt", "😀target")
        self.put("replacement.txt", "🛰️")
        self.success(self.plan("--tab", "t.child"))
        self.assertEqual(self.load_plan()["target"],
                         {"start_index": 1, "end_index": 9})
        self.success(self.apply())

    def test_literal_matching_does_not_enable_regex_or_shell(self):
        literal = "$(touch injected);.*"
        self.fixture(document(literal + "\n"), document("done\n", "rev-2"))
        self.put("find.txt", literal)
        self.put("replacement.txt", "done")
        self.success(self.plan())
        self.success(self.apply())
        self.assertFalse((self.root / "injected").exists())

    def test_preview_is_offline_and_does_not_submit(self):
        self.success(self.plan())
        calls = self.calls()
        before = (self.root / "plan.json").read_bytes()
        self.success(self.apply("--dry-run"))
        self.assertEqual(self.calls(), calls)
        self.assertEqual((self.root / "plan.json").read_bytes(), before)

    def test_source_revision_missing_or_changed_refuses(self):
        self.success(self.plan())
        original = (self.root / "plan.json").read_bytes()
        for doc in [document(revision="rev-new"), document("Different world.\n"),
                    {k: v for k, v in document().items() if k != "revisionId"}]:
            with self.subTest(doc=doc):
                self.fixture(doc)
                self.refused(self.apply())
                self.assertFalse((self.root / "submitted").exists())
                self.assertEqual((self.root / "plan.json").read_bytes(), original)

    def test_plan_without_revision_refuses(self):
        doc = document()
        doc.pop("revisionId")
        self.fixture(doc)
        self.refused(self.plan())

    def test_malformed_tampered_and_oversized_plan_refuse_offline(self):
        self.success(self.plan())
        original = self.load_plan()
        tampered = copy.deepcopy(original)
        tampered["replacement"] = "unreviewed"
        unknown = resign(dict(original, command="touch injected"))
        wrong_target = copy.deepcopy(original)
        wrong_target["target"]["start_index"] = -1
        wrong_version = resign(dict(original, version=True))
        for value in ["{", "[]", '{"version":1,"version":2}',
                      json.dumps(tampered), json.dumps(unknown),
                      json.dumps(resign(wrong_target)), json.dumps(wrong_version),
                      " " * (1024 * 1024 + 1)]:
            with self.subTest(value=value[:90]):
                self.put("plan.json", value)
                before = (self.root / "plan.json").read_bytes()
                calls = self.calls()
                self.refused(self.apply())
                self.assertEqual(self.calls(), calls)
                self.assertEqual((self.root / "plan.json").read_bytes(), before)

    def test_resigned_semantic_tampering_is_reconstructed_before_write(self):
        self.success(self.plan())
        plan = self.load_plan()
        plan["target"]["start_index"] = 8
        self.put("plan.json", json.dumps(resign(plan)))
        self.refused(self.apply())
        self.assertFalse((self.root / "submitted").exists())

    def test_unsupported_structural_edits_and_noops_refuse(self):
        for find, replacement in [("", "new"), ("world", "world"),
                                  ("world", "one\ntwo"), ("world", "\ufffc"),
                                  ("world.\n", "new"), ("world", "\x00")]:
            with self.subTest(find=find, replacement=replacement):
                self.put("find.txt", find)
                self.put("replacement.txt", replacement)
                self.refused(self.plan())
                self.assertFalse((self.root / "plan.json").exists())

    def test_table_header_suggestion_and_image_crossing_targets_refuse(self):
        table = document()
        body(table)[1] = {"startIndex": 1, "endIndex": 14, "table": {
            "rows": 1, "columns": 1, "tableRows": [{"tableCells": [{
                "content": [paragraph([("world\n", {})], 3)]}]}]}}
        header = document("Other.\n")
        header["tabs"][0]["documentTab"]["headers"] = {
            "h.1": {"content": [paragraph([("world\n", {})])]}}
        suggested = document()
        body(suggested)[1]["paragraph"]["elements"][0]["textRun"][
            "suggestedInsertionIds"] = ["suggestion-1"]
        image = document()
        body(image)[1] = paragraph([("wor", {}), ("ld\n", {})])
        elements = body(image)[1]["paragraph"]["elements"]
        elements.insert(1, {"startIndex": 4, "endIndex": 5,
                            "inlineObjectElement": {"inlineObjectId": "img-1"}})
        elements[2]["startIndex"] += 1
        elements[2]["endIndex"] += 1
        body(image)[1]["endIndex"] += 1
        for doc in [table, header, suggested, image]:
            with self.subTest(doc=doc):
                self.fixture(doc)
                self.refused(self.plan())
                self.assertFalse((self.root / "submitted").exists())

    def test_duplicate_in_table_or_header_also_refuses(self):
        doc = document()
        doc["tabs"][0]["documentTab"]["footers"] = {
            "f.1": {"content": [paragraph([("world\n", {})])]}}
        self.fixture(doc)
        self.refused(self.plan())

    def test_style_run_splitting_and_deletion_are_supported(self):
        before = document()
        body(before)[1] = paragraph([
            ("Hello wo", {"bold": True}), ("rld", {"italic": True}), (".\n", {})])
        after = document(revision="rev-2")
        body(after)[1] = paragraph([("Hello ", {"bold": True}), (".\n", {})])
        self.fixture(before, after)
        self.put("replacement.txt", "")
        self.success(self.plan())
        self.success(self.apply())

    def test_preserves_tables_images_styles_and_checks_untouched_content(self):
        before = document()
        body(before).append({"startIndex": 14, "endIndex": 15, "paragraph": {
            "elements": [{"startIndex": 14, "endIndex": 15,
                          "inlineObjectElement": {"inlineObjectId": "img-1"}}]}})
        before["tabs"][0]["documentTab"]["inlineObjects"] = {
            "img-1": {"inlineObjectProperties": {"embeddedObject": {
                "imageProperties": {"contentUri": "https://example.invalid/temporary"},
                "size": {"width": {"magnitude": 50, "unit": "PT"}}}}}}
        body(before).append({"startIndex": 15, "endIndex": 25, "table": {
            "rows": 1, "columns": 1, "tableRows": [{"tableCells": [{
                "content": [paragraph([("cell\n", {"bold": True})], 18)]}]}]}})
        after = copy.deepcopy(before)
        after["revisionId"] = "rev-2"
        body(after)[1] = paragraph([("Hello reader.\n", {})])
        # One extra UTF-16 unit shifts following structures, not their content.
        def shift(value):
            if isinstance(value, dict):
                for k, v in value.items():
                    if k in ("startIndex", "endIndex"):
                        value[k] = v + 1
                    else:
                        shift(v)
            elif isinstance(value, list):
                for v in value:
                    shift(v)
        shift(body(after)[2:])
        after["tabs"][0]["documentTab"]["inlineObjects"]["img-1"][
            "inlineObjectProperties"]["embeddedObject"]["imageProperties"][
                "contentUri"] = "https://example.invalid/refreshed"
        self.fixture(before, after)
        self.success(self.plan())
        self.success(self.apply())
        for kind in ["table", "image-id", "image-size"]:
            with self.subTest(kind=kind):
                changed = copy.deepcopy(after)
                if kind == "table":
                    table_paragraph = body(changed)[3]["table"]["tableRows"][0][
                        "tableCells"][0]["content"][0]
                    table_paragraph["paragraph"]["elements"][0]["textRun"]["content"] = "sell\n"
                elif kind == "image-id":
                    body(changed)[2]["paragraph"]["elements"][0][
                        "inlineObjectElement"]["inlineObjectId"] = "different-image"
                else:
                    changed["tabs"][0]["documentTab"]["inlineObjects"]["img-1"][
                        "inlineObjectProperties"]["embeddedObject"]["size"]["width"][
                            "magnitude"] = 51
                (self.root / "submitted").unlink()
                self.fixture(before, changed)
                self.refused(self.apply(), "ambiguous")

    def test_post_write_mismatch_and_missing_revision_are_not_success(self):
        self.success(self.plan())
        original = (self.root / "plan.json").read_bytes()
        missing = document("Hello reader.\n")
        missing.pop("revisionId")
        changed_style = document("Hello reader.\n", "rev-2")
        body(changed_style)[1]["paragraph"]["elements"][0]["textRun"][
            "textStyle"] = {"bold": True}
        for doc in [document("Hello incorrect.\n", "rev-2"), missing,
                    changed_style, document("Hello reader.\n", "rev-unexpected")]:
            with self.subTest(doc=doc):
                (self.root / "submitted").unlink(missing_ok=True)
                self.fixture(document(), doc)
                self.refused(self.apply(), "ambiguous")
                self.assertEqual((self.root / "plan.json").read_bytes(), original)

    def test_failed_write_or_verification_never_retries_and_keeps_plan(self):
        self.success(self.plan())
        original = (self.root / "plan.json").read_bytes()
        for mode in ["conflict", "write-error", "zero", "two", "missing-reply",
                     "bad-response", "missing-write-revision", "verify-error"]:
            with self.subTest(mode=mode):
                (self.root / "submitted").unlink(missing_ok=True)
                self.put("mode", mode)
                calls = len(self.calls())
                self.refused(self.apply(), "ambiguous")
                writes = [c for c in self.calls()[calls:] if c[2] == "batchUpdate"]
                self.assertEqual(len(writes), 1)
                self.assertEqual((self.root / "plan.json").read_bytes(), original)

    def test_timeout_after_submission_reports_possible_application(self):
        self.success(self.plan())
        original = (self.root / "plan.json").read_bytes()
        self.put("mode", "timeout")
        result = self.apply("--timeout", "0.3")
        self.refused(result, "ambiguous")
        self.assertIn("may have", json.loads(result.stderr)["message"])
        self.assertEqual(len([c for c in self.calls() if c[2] == "batchUpdate"]), 1)
        self.assertEqual((self.root / "plan.json").read_bytes(), original)

    def test_paths_are_relative_confined_and_never_overwrite(self):
        self.success(self.plan())
        original = (self.root / "plan.json").read_bytes()
        self.refused(self.plan())
        self.assertEqual((self.root / "plan.json").read_bytes(), original)
        for path in ["../escape", "/tmp/escape", "sub/../../escape", "bad\nname"]:
            with self.subTest(path=path):
                self.refused(self.cli("apply", "--plan", path))
                self.refused(self.cli("plan", "--document", "synthetic-doc",
                                     "--find", path, "--replacement", "replacement.txt",
                                     "--out", "new-plan.json"))
                self.refused(self.cli("plan", "--document", "synthetic-doc",
                                     "--find", "find.txt", "--replacement", "replacement.txt",
                                     "--out", path))
        with tempfile.TemporaryDirectory() as outside:
            (self.root / "escape").symlink_to(outside, target_is_directory=True)
            Path(outside, "plan.json").write_bytes(original)
            self.refused(self.cli("apply", "--plan", "escape/plan.json"))
            self.refused(self.cli("plan", "--document", "synthetic-doc",
                                 "--find", "find.txt", "--replacement", "replacement.txt",
                                 "--out", "escape/new.json"))
            self.assertFalse(Path(outside, "new.json").exists())

    def test_invalid_document_id_and_gws_failure_are_redacted(self):
        result = self.cli("plan", "--document", "../secret?token",
                          "--find", "find.txt", "--replacement", "replacement.txt",
                          "--out", "plan.json")
        self.refused(result)
        self.assertEqual(self.calls(), [])
        self.put("mode", "read-error")
        self.refused(self.plan())

    def test_model_armor_diagnostics_remain_visible_without_leaking_content(self):
        self.env["GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE"] = "synthetic-template"
        self.put("mode", "armor-block")
        self.refused(self.plan())
        self.assertFalse((self.root / "plan.json").exists())
        self.put("mode", "armor-warn")
        result = self.success(self.plan())
        self.assertTrue(result.get("gws_diagnostics"))
        self.assertNotIn("secret-token", json.dumps(result))
        self.assertNotIn("\x1b", json.dumps(result))

    def test_unknown_regions_and_malformed_structures_fail_before_plan(self):
        unknown = document()
        unknown["tabs"][0]["documentTab"]["futureRegion"] = {"text": "world"}
        malformed = document()
        body(malformed)[0]["futureBlock"] = {}
        for doc in [unknown, malformed]:
            with self.subTest(doc=doc):
                self.fixture(doc)
                self.refused(self.plan())
                self.assertFalse((self.root / "plan.json").exists())

    def test_omitted_empty_text_style_does_not_fail_verification(self):
        before = document()
        after = document("Hello reader.\n", "rev-2")
        body(after)[1]["paragraph"]["elements"][0]["textRun"].pop("textStyle")
        self.fixture(before, after)
        self.success(self.plan())
        self.success(self.apply())

    def test_c1_control_characters_in_paths_are_rejected_before_read(self):
        self.put("unsafe\u0085.txt", "world")
        self.refused(self.cli(
            "plan", "--document", "synthetic-doc", "--find", "unsafe\u0085.txt",
            "--replacement", "replacement.txt", "--out", "plan.json",
        ))
        self.assertEqual(self.calls(), [])

    def test_nonregular_files_and_leaf_symlinks_refuse(self):
        os.mkfifo(self.root / "pipe")
        (self.root / "link.txt").symlink_to(self.root / "find.txt")
        for path in ["pipe", "link.txt"]:
            with self.subTest(path=path):
                self.refused(self.cli(
                    "plan", "--document", "synthetic-doc", "--find", path,
                    "--replacement", "replacement.txt", "--out", "plan.json",
                ))
        self.assertEqual(self.calls(), [])

    def test_invalid_utf8_oversized_text_and_timeout_values_refuse(self):
        for data in [b"\xff", b"x" * (16 * 1024 + 1)]:
            with self.subTest(size=len(data)):
                (self.root / "find.txt").write_bytes(data)
                self.refused(self.plan())
        for timeout in ["nan", "inf", "0", "-1", "601"]:
            with self.subTest(timeout=timeout):
                self.refused(self.plan("--timeout", timeout))
        self.assertEqual(self.calls(), [])

    def test_malformed_source_and_unsupported_anchors_refuse(self):
        for value in ["[]", "{", '{"documentId":NaN}']:
            with self.subTest(value=value):
                self.put("before.json", value)
                self.refused(self.plan())
        for key in ["namedRanges", "bookmarks"]:
            doc = document()
            doc["tabs"][0]["documentTab"][key] = {"synthetic-anchor": {}}
            self.fixture(doc)
            self.refused(self.plan())
        self.assertFalse((self.root / "submitted").exists())

    def test_large_expanded_style_payload_refuses_before_plan(self):
        doc = document("world" + "a" * 20000 + "\n")
        body(doc)[1]["paragraph"]["elements"][0]["textRun"]["textStyle"] = {
            "link": {"url": "https://example.invalid/" + "x" * 2000}}
        self.fixture(doc)
        self.refused(self.plan())
        self.assertFalse((self.root / "plan.json").exists())

    def test_default_zero_indices_in_untouched_header_are_supported(self):
        before = document()
        header = paragraph([("Header\n", {})], start=0)
        del header["startIndex"]
        del header["paragraph"]["elements"][0]["startIndex"]
        before["tabs"][0]["documentTab"]["headers"] = {"h.1": {"content": [header]}}
        after = copy.deepcopy(before)
        after["revisionId"] = "rev-2"
        body(after)[1] = paragraph([("Hello reader.\n", {})])
        self.fixture(before, after)
        self.success(self.plan())
        self.success(self.apply())


if __name__ == "__main__":
    unittest.main()
