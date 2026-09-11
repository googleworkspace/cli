#!/usr/bin/env python3
# Copyright 2026 Google LLC
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy at https://www.apache.org/licenses/LICENSE-2.0
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Hermetic behavior tests: generated documents and executable process stubs only."""

import base64
import contextlib
import hashlib
import http.server
from html.parser import HTMLParser
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock
import urllib.parse
import zipfile


SCRIPT = Path(__file__).with_name("docs_review_bundle.py")
PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8"
    "/x8AAwMCAO+jRZkAAAAASUVORK5CYII="
)
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
WP = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
REL = "http://schemas.openxmlformats.org/package/2006/relationships"
REMOTE = "https://untrusted.invalid/image?token=SIGNED_SECRET"


def paragraph(text, style="NORMAL_TEXT"):
    return {
        "paragraph": {
            "paragraphStyle": {"namedStyleType": style},
            "elements": [
                {"textRun": {"content": text, "textStyle": {"link": {"url": REMOTE}}}}
            ],
        }
    }


def native_document():
    return {
        "documentId": "synthetic-doc",
        "title": 'Review <script>alert("title")</script>',
        "revisionId": "revision-one",
        "tabs": [
            {
                "tabProperties": {"tabId": "tab-main", "title": "Main"},
                "documentTab": {
                    "body": {
                        "content": [
                            paragraph("Heading <script>bad()</script>", "HEADING_1"),
                            {
                                "table": {
                                    "tableRows": [
                                        {
                                            "tableCells": [
                                                {"content": [paragraph("Table cell")]}
                                            ]
                                        }
                                    ]
                                }
                            },
                            {
                                "paragraph": {
                                    "elements": [
                                        {
                                            "inlineObjectElement": {
                                                "inlineObjectId": "native-image"
                                            }
                                        }
                                    ]
                                }
                            },
                        ]
                    },
                    "inlineObjects": {
                        "native-image": {
                            "inlineObjectProperties": {
                                "embeddedObject": {
                                    "title": "Native figure",
                                    "description": "No exact DOCX mapping",
                                    "imageProperties": {"contentUri": REMOTE},
                                }
                            }
                        },
                        "missing-uri": {
                            "inlineObjectProperties": {
                                "embeddedObject": {
                                    "description": "No downloadable URI",
                                    "imageProperties": {},
                                }
                            }
                        },
                    },
                },
                "childTabs": [
                    {
                        "tabProperties": {"tabId": "tab-child", "title": "Child"},
                        "documentTab": {
                            "body": {"content": [paragraph("Nested tab paragraph")]}
                        },
                    }
                ],
            }
        ],
    }


def pdf_bytes(page_count=1):
    """A complete synthetic PDF, also usable by real pdftoppm."""
    content = (
        b"BT /F1 22 Tf 48 720 Td (Synthetic Docs review) Tj ET\n"
        b"BT /F1 12 Tf 48 687 Td (Local fixture - no Google document) Tj ET\n"
        b"0.85 0.92 1 rg 48 435 516 210 re f\n"
        b"0.08 0.25 0.5 rg 72 459 120 162 re f\n"
        b"0.12 0.45 0.6 rg 216 459 120 105 re f\n"
        b"0.1 0.6 0.45 rg 360 459 120 140 re f\n"
        b"0 0 0 rg BT /F1 12 Tf 48 402 Td (Synthetic figure and table context) Tj ET\n"
        b"0.5 G 48 270 516 90 re S 48 315 m 564 315 l S\n"
        b"306 270 m 306 360 l S\n"
        b"BT /F1 12 Tf 60 333 Td (Column A) Tj 258 0 Td (Column B) Tj ET\n"
        b"BT /F1 12 Tf 60 288 Td (Cell one) Tj 258 0 Td (Cell two) Tj ET\n"
    )
    content_id = page_count + 3
    font_id = page_count + 4
    kids = " ".join(f"{number} 0 R" for number in range(3, page_count + 3))
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        f"<< /Type /Pages /Kids [{kids}] /Count {page_count} >>".encode(),
    ]
    objects.extend(
        (
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            f"/Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>"
        ).encode()
        for _ in range(page_count)
    )
    objects.extend([
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"endstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ])
    data = b"%PDF-1.4\n"
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data += f"{index} 0 obj\n".encode() + obj + b"\nendobj\n"
    startxref = len(data)
    object_count = len(objects) + 1
    data += f"xref\n0 {object_count}\n0000000000 65535 f \n".encode()
    for offset in offsets[1:]:
        data += f"{offset:010d} 00000 n \n".encode()
    data += (
        f"trailer\n<< /Size {object_count} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n"
    ).encode()
    return data


def docx_members():
    xml = f"""<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:a="{A}" xmlns:wp="{WP}">
<w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
<w:r><w:t>Nearby &lt;script&gt;bad()&lt;/script&gt;</w:t></w:r>
<w:r><w:drawing><wp:inline><wp:docPr id="1"
descr="Figure &quot; onerror=&quot;bad()" title="First"/>
<a:graphic><a:blip r:embed="rId1"/></a:graphic></wp:inline></w:drawing></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Table image context</w:t></w:r>
<w:r><w:drawing><wp:inline><wp:docPr id="2" descr="Second"/>
<a:graphic><a:blip r:embed="rId2"/></a:graphic></wp:inline></w:drawing></w:r>
</w:p></w:tc></w:tr></w:tbl>
<w:p><w:r><w:drawing><wp:inline><wp:docPr id="3" descr="External"/>
<a:graphic><a:blip r:link="rId3"/></a:graphic></wp:inline></w:drawing></w:r></w:p>
</w:body></w:document>"""
    rels = f"""<Relationships xmlns="{REL}">
<Relationship Id="rId1" Type="{R}/image" Target="media/image1.png"/>
<Relationship Id="rId2" Type="{R}/image" Target="media/nested/image1.png"/>
<Relationship Id="rId3" Type="{R}/image" Target="{REMOTE.replace('&', '&amp;')}"
TargetMode="External"/></Relationships>"""
    return {
        "word/document.xml": xml.encode(),
        "word/_rels/document.xml.rels": rels.encode(),
        "word/styles.xml": f'<w:styles xmlns:w="{W}"/>'.encode(),
        "word/media/image1.png": PNG,
        "word/media/nested/image1.png": PNG + b"different",
    }


def nested_docx_members(*, outer_image=False):
    members = docx_members()
    outer_blip = '<a:blip r:embed="rId2"/>' if outer_image else ""
    members["word/document.xml"] = f"""
<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:a="{A}" xmlns:wp="{WP}">
<w:body><w:p><w:r><w:t>Outer paragraph</w:t></w:r>
<w:r><w:drawing><wp:inline><wp:docPr id="1" descr="Outer text box"/>
<a:graphic><w:txbxContent><w:p><w:r><w:t>Inner paragraph</w:t></w:r>
<w:r><w:drawing><wp:inline><wp:docPr id="2" descr="Inner image"/>
<a:graphic><a:blip r:embed="rId1"/></a:graphic>
</wp:inline></w:drawing></w:r></w:p></w:txbxContent>
{outer_blip}</a:graphic></wp:inline></w:drawing></w:r>
</w:p></w:body></w:document>""".encode()
    return members


def write_docx(path, members=None):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, data in (members if members is not None else docx_members()).items():
            archive.writestr(name, data)


def fixtures(path):
    path.mkdir()
    (path / "source.json").write_text(json.dumps(native_document()), encoding="utf-8")
    (path / "revision-after.json").write_text(
        '{"revisionId":"revision-one"}', encoding="utf-8"
    )
    (path / "document.pdf").write_bytes(pdf_bytes())
    (path / "document.md").write_text(
        "# Markdown\n<script>markdown()</script>\n![remote](" + REMOTE + ")\n",
        encoding="utf-8",
    )
    write_docx(path / "document.docx")
    return path


GWS_STUB = r'''
import json
import os
from pathlib import Path
import shutil
import sys
import time

args = sys.argv[1:]
mode = os.environ.get("STUB_MODE", "")
fixture = Path(os.environ["STUB_FIXTURES"])
with open(os.environ["STUB_LOG"], "a", encoding="utf-8") as log:
    log.write(json.dumps({"args": args, "cwd": os.getcwd(),
                         "sanitize": os.environ.get("GOOGLE_WORKSPACE_CLI_SANITIZE_MODE")}) + "\n")
if args == ["--version"]:
    print("gws 0.0.0-synthetic")
    sys.exit(0)
params = json.loads(args[args.index("--params") + 1])
assert args[args.index("--format") + 1] == "json"
if mode == "timeout":
    time.sleep(10)
if args[:3] == ["docs", "documents", "get"]:
    assert params["documentId"] == "synthetic-doc"
    assert params["includeTabsContent"] is True
    if params.get("fields") == "revisionId":
        source = {"revisionId": "revision-two" if mode == "mixed" else "revision-one"}
        if mode == "missing-revision":
            source = {}
    else:
        source = json.loads((fixture / "source.json").read_text())
    print(json.dumps(source))
elif args[:3] == ["drive", "files", "export"]:
    assert params["fileId"] == "synthetic-doc"
    expected = {
        "application/pdf": "document.pdf",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document": "document.docx",
        "text/markdown": "document.md",
    }
    name = args[args.index("--output") + 1]
    assert expected[params["mimeType"]] == name
    assert "/" not in name and "\\" not in name
    if mode == "failed-export" and name == "document.docx":
        Path(name).write_bytes(b"partial")
        print("Bearer PRIVATE_TOKEN " + "\x1b[31m", file=sys.stderr)
        sys.exit(7)
    if mode != "no-export-file":
        shutil.copyfile(fixture / name, name)
    print(json.dumps({
        "status": "error" if mode == "bad-export-status" else "success",
        "saved_file": (
            str(Path.cwd().parent / "other" / name) if mode == "wrong-export-path"
            else name if mode == "relative-export-path"
            else str(Path(name).resolve())
        ),
        "mimeType": params["mimeType"],
        "bytes": 1 if mode == "wrong-export-size" else (fixture / name).stat().st_size,
    }))
elif args[:3] == ["drive", "comments", "list"]:
    assert params["fileId"] == "synthetic-doc"
    assert "nextPageToken" in params["fields"]
    if mode == "expanded-comments":
        # ~8 MiB on the wire; >24 MiB after ensure_ascii=True serialization.
        print(json.dumps(
            {"comments": [{"id": "large", "content": "é" * (4 * 1024 * 1024)}]},
            ensure_ascii=False, separators=(",", ":"),
        ))
        sys.exit(0)
    if mode == "failed-comments" and params.get("pageToken"):
        sys.exit(9)
    if params.get("pageToken"):
        print(json.dumps({"comments": [{"id": "two", "content": "Second comment"}]}))
    else:
        print(json.dumps({"nextPageToken": "page-two", "comments": [{"id": "one"}]}))
else:
    raise AssertionError(args)
'''

RENDER_STUB = r'''
import base64
import os
from pathlib import Path
import sys
import time
assert sys.argv[1:4] == ["-png", "-r", "96"]
assert sys.argv[-2] == "document.pdf"
prefix = Path(sys.argv[-1])
png = base64.b64decode(os.environ["STUB_PNG"])
mode = os.environ.get("RENDER_MODE", "")
Path(str(prefix) + "-1.png").write_bytes(png)
if mode == "failed":
    sys.exit(2)
if mode == "timeout":
    time.sleep(10)
if mode == "gap":
    Path(str(prefix) + "-3.png").write_bytes(png)
'''


class HTMLInspection(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.tags = []
        self.references = []
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        self.tags.append((tag, dict(attrs)))
        for name, value in attrs:
            if name in ("src", "href", "data"):
                self.references.append(value)


class BundleTestCase(unittest.TestCase):
    def setUp(self):
        # All generated files stay inside this assigned worktree and are removed.
        self.temp = tempfile.TemporaryDirectory(dir=SCRIPT.parent)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.fixture = fixtures(self.root / "fixtures")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = {
            "PATH": str(self.bin),
            "PYTHONDONTWRITEBYTECODE": "1",
            "STUB_FIXTURES": str(self.fixture),
            "STUB_LOG": str(self.root / "gws.log"),
            "STUB_PNG": base64.b64encode(PNG).decode(),
            "GOOGLE_WORKSPACE_CLI_CONFIG_DIR": str(self.root / "unused-config"),
            "GOOGLE_WORKSPACE_CLI_SANITIZE_MODE": "block",
        }

    def api(self):
        self.assertTrue(SCRIPT.is_file(), "Missing executable review bundle companion")
        if not hasattr(self, "_api"):
            spec = importlib.util.spec_from_file_location("docs_review_bundle", SCRIPT)
            self._api = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(self._api)
        return self._api

    def executable(self, name, source):
        path = self.bin / name
        path.write_text(f"#!{sys.executable}\n" + source, encoding="utf-8")
        path.chmod(0o700)
        return path

    def run_bundle(self, *args, live=False, **env):
        self.assertTrue(SCRIPT.is_file(), "Missing executable review bundle companion")
        if live:
            self.executable("gws", GWS_STUB)
            source = ["--document-id", "synthetic-doc"]
        else:
            source = ["--from-fixture", "fixtures"]
        return subprocess.run(
            [sys.executable, "-B", str(SCRIPT), *source, *args],
            cwd=self.root,
            env={**self.env, **env},
            text=True,
            capture_output=True,
            timeout=15,
        )

    def manifest(self, directory="review"):
        return json.loads((self.root / directory / "manifest.json").read_text())


class ExportReceiptTests(BundleTestCase):
    def test_canonical_receipts_complete_all_exports(self):
        result = self.run_bundle("review", live=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["status"], "complete")

    def test_wrong_destination_or_relative_receipt_is_refused(self):
        for mode in ["wrong-export-path", "relative-export-path"]:
            with self.subTest(mode=mode):
                result = self.run_bundle(mode, live=True, STUB_MODE=mode)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(self.manifest(mode)["error"], "invalid-export-receipt")


@unittest.skipUnless(os.environ.get("GWS_TEST_BINARY"), "Set GWS_TEST_BINARY for real CLI coverage")
class RealCliExportTests(BundleTestCase):
    def test_real_cli_exports_complete_bundle_with_canonical_receipts(self):
        binary = Path(os.environ["GWS_TEST_BINARY"]).resolve(strict=True)
        fixture = self.fixture
        requests = []
        exports = {
            "application/pdf": "document.pdf",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document": "document.docx",
            "text/markdown": "document.md",
        }

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                parsed = urllib.parse.urlsplit(self.path)
                path = urllib.parse.unquote(parsed.path)
                params = urllib.parse.parse_qs(parsed.query)
                requests.append((path, params, self.headers.get("Authorization")))
                if path == "/documents/synthetic-doc":
                    data = (fixture / "source.json").read_bytes()
                    mime = "application/json"
                elif path == "/files/synthetic-doc/export":
                    mime = params.get("mimeType", [""])[0]
                    if mime not in exports:
                        self.send_error(400)
                        return
                    data = (fixture / exports[mime]).read_bytes()
                else:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Type", mime)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            config = self.root / "real-cli-config"
            cache = config / "cache"
            cache.mkdir(parents=True)
            (self.root / ".env").write_text("")
            for service, version, resource, method, id_field, path in [
                ("docs", "v1", "documents", "get", "documentId", "documents/{documentId}"),
                ("drive", "v3", "files", "export", "fileId", "files/{fileId}/export"),
            ]:
                discovery = {
                    "name": service, "version": version,
                    "rootUrl": f"http://127.0.0.1:{server.server_port}/",
                    "resources": {resource: {"methods": {method: {
                        "httpMethod": "GET", "path": path,
                        "parameters": {id_field: {
                            "type": "string", "location": "path", "required": True,
                        }},
                    }}}},
                }
                (cache / f"{service}_{version}.json").write_text(json.dumps(discovery))
            env = {
                **self.env,
                "GOOGLE_WORKSPACE_CLI_CONFIG_DIR": str(config),
                "GOOGLE_WORKSPACE_CLI_TOKEN": "synthetic-loopback-token",
                "GOOGLE_APPLICATION_CREDENTIALS": str(self.root / "absent-adc.json"),
                "GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND": "file",
                "GOOGLE_WORKSPACE_PROJECT_ID": "synthetic-loopback-project",
                "HTTP_PROXY": "http://127.0.0.1:1",
                "HTTPS_PROXY": "http://127.0.0.1:1",
                "ALL_PROXY": "http://127.0.0.1:1",
                "NO_PROXY": "127.0.0.1,localhost",
            }
            result = subprocess.run(
                [sys.executable, "-B", str(SCRIPT), "--document-id", "synthetic-doc",
                 "--gws", str(binary), "--timeout", "10", "review"],
                cwd=self.root, env=env, text=True, capture_output=True, timeout=25,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            manifest = self.manifest()
            self.assertEqual(manifest["status"], "complete")
            self.assertEqual(manifest["revisions"]["status"], "unchanged")
            for name in exports.values():
                data = (self.root / "review" / name).read_bytes()
                self.assertEqual(data, (fixture / name).read_bytes())
                self.assertEqual(manifest["artifacts"][name]["bytes"], len(data))
            self.assertEqual(len(requests), 5)
            self.assertTrue(all(auth == "Bearer synthetic-loopback-token"
                                for _, _, auth in requests))
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


class ExtractionTests(BundleTestCase):
    def extract(self, **limits):
        return self.api().extract_docx(
            self.fixture / "document.docx", self.root / "assets", **limits
        )

    def test_relationships_preserve_order_alt_text_and_table_context(self):
        result = self.extract()
        figures = result["figures"]
        self.assertEqual([f["order"] for f in figures], [1, 2, 3])
        self.assertIn('Figure " onerror="bad()', figures[0]["alt"])
        self.assertIn("Table image context", figures[1]["nearby_text"])
        self.assertIsNone(figures[2]["asset"])
        self.assertEqual(figures[2]["availability"], "external-not-fetched")
        self.assertIsNone(figures[0]["native_object_id"])
        self.assertEqual(figures[0]["mapping_confidence"], "docx-relationship-only")

    def test_duplicate_basenames_get_distinct_local_assets(self):
        result = self.extract()
        paths = [f["asset"] for f in result["figures"][:2]]
        self.assertNotEqual(*paths)
        self.assertEqual((self.root / paths[0]).read_bytes(), PNG)
        self.assertEqual((self.root / paths[1]).read_bytes(), PNG + b"different")

    def test_nested_text_box_image_has_one_occurrence_with_inner_ownership(self):
        write_docx(self.fixture / "document.docx", nested_docx_members())
        figures = self.extract()["figures"]
        self.assertEqual(len(figures), 1)
        self.assertEqual(figures[0]["order"], 1)
        self.assertEqual(figures[0]["alt"], "Inner image")
        self.assertEqual(figures[0]["nearby_text"], "Inner paragraph")
        self.assertEqual((self.root / figures[0]["asset"]).read_bytes(), PNG)

    def test_nested_image_order_and_context_follow_nearest_owners(self):
        write_docx(self.fixture / "document.docx", nested_docx_members(outer_image=True))
        figures = self.extract()["figures"]
        self.assertEqual([f["relationship_id"] for f in figures], ["rId1", "rId2"])
        self.assertEqual([f["alt"] for f in figures], ["Inner image", "Outer text box"])
        self.assertEqual(
            [f["nearby_text"] for f in figures], ["Inner paragraph", "Outer paragraph"]
        )
        self.assertEqual([f["order"] for f in figures], [1, 2])

    def test_repeated_asset_uses_remain_distinct_figure_occurrences(self):
        members = docx_members()
        members["word/document.xml"] = members["word/document.xml"].replace(
            b'r:embed="rId2"', b'r:embed="rId1"'
        )
        write_docx(self.fixture / "document.docx", members)
        figures = self.extract()["figures"]
        self.assertEqual(len(figures), 3)
        self.assertEqual([f["order"] for f in figures], [1, 2, 3])
        self.assertEqual(figures[0]["asset"], figures[1]["asset"])
        self.assertEqual([f["relationship_id"] for f in figures[:2]], ["rId1", "rId1"])
        self.assertNotEqual(figures[0]["alt"], figures[1]["alt"])
        self.assertEqual(figures[1]["nearby_text"], "Table image context")

    def test_zip_traversal_absolute_windows_and_control_paths_are_rejected(self):
        api = self.api()
        for index, name in enumerate(
            ["../escape", "/escape", "C:/escape", r"..\escape", "word/../escape", "bad\x01"]
        ):
            with self.subTest(name=name):
                members = {**docx_members(), name: b"bad"}
                write_docx(self.fixture / "document.docx", members)
                with self.assertRaises(api.BundleError):
                    api.extract_docx(
                        self.fixture / "document.docx", self.root / f"assets-{index}"
                    )
        self.assertFalse((self.root / "escape").exists())

    def test_zip_symlink_rejected(self):
        api = self.api()
        with zipfile.ZipFile(self.fixture / "document.docx", "a") as archive:
            link = zipfile.ZipInfo("word/media/link.png")
            link.create_system = 3
            link.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(link, "../../../escape")
        with self.assertRaises(api.BundleError):
            self.extract()

    def test_member_total_and_count_limits_reject_before_writing_assets(self):
        api = self.api()
        for limits in (
            {"member_limit": 32},
            {"total_limit": 32},
            {"member_count": 2},
        ):
            with self.subTest(limits=limits), self.assertRaises(api.BundleError):
                self.extract(**limits)
        self.assertFalse((self.root / "assets").exists())

    def test_duplicate_archive_member_rejected(self):
        api = self.api()
        with zipfile.ZipFile(self.fixture / "document.docx", "a") as archive:
            archive.writestr("WORD/MEDIA/IMAGE1.PNG", PNG)
        with self.assertRaises(api.BundleError):
            self.extract()

    def test_doctype_entities_and_utf16_are_rejected_even_in_unused_xml(self):
        api = self.api()
        for data in (
            b'<!DOCTYPE x [<!ENTITY x "boom">]><x>&x;</x>',
            b'<!ENTITY x SYSTEM "file:///etc/passwd"><x/>',
            '<!DOCTYPE x [<!ENTITY x "boom">]><x>&x;</x>'.encode("utf-16"),
        ):
            with self.subTest(data=data[:30]):
                members = {**docx_members(), "word/unused.xml": data}
                write_docx(self.fixture / "document.docx", members)
                with self.assertRaises(api.BundleError):
                    self.extract()
        self.assertFalse((self.root / "assets").exists())

    def test_xml_encoding_allowlist_handles_declaration_whitespace(self):
        api = self.api()
        for encoding in ("UTF-7", "UTF-16", "UTF-32", "ISO-8859-1"):
            for assignment in (f' = "{encoding}"', f"= '{encoding}'", f'\t=\n"{encoding}"'):
                with self.subTest(encoding=encoding, assignment=assignment):
                    data = f'<?xml version="1.0" encoding{assignment}?><x/>'.encode("utf-8")
                    with self.assertRaises(api.BundleError) as raised:
                        api.safe_xml(data)
                    self.assertEqual(str(raised.exception), "unsupported-xml-encoding")

    def test_xml_utf8_bom_is_accepted_and_utf16_utf32_are_rejected(self):
        api = self.api()
        data = '<?xml version="1.0" encoding = "UTF-8"?><x>café</x>'.encode("utf-8-sig")
        root = api.safe_xml(data)
        self.assertEqual(root.tag, "x")
        self.assertEqual(root.text, "café")
        for encoding in ("utf-16", "utf-32"):
            with self.subTest(encoding=encoding), self.assertRaises(api.BundleError):
                api.safe_xml("<x/>".encode(encoding))

    def test_relationship_traversal_and_svg_are_not_local_html_assets(self):
        members = docx_members()
        members["word/_rels/document.xml.rels"] = members[
            "word/_rels/document.xml.rels"
        ].replace(b"media/image1.png", b"../outside.png")
        members["word/media/nested/image1.png"] = b"<svg onload='bad()'/>"
        write_docx(self.fixture / "document.docx", members)
        figures = self.extract()["figures"]
        self.assertIsNone(figures[0]["asset"])
        self.assertIsNone(figures[1]["asset"])
        # The safe, now unreferenced raster is still preserved; the SVG is not.
        files = list((self.root / "assets").iterdir())
        self.assertEqual(len(files), 1)
        self.assertEqual(files[0].read_bytes(), PNG)

    def test_malformed_xml_and_corrupt_zip_fail_closed(self):
        api = self.api()
        for data in (b"not a zip", None):
            if data is None:
                write_docx(
                    self.fixture / "document.docx",
                    {**docx_members(), "word/document.xml": b"<broken"},
                )
            else:
                (self.fixture / "document.docx").write_bytes(data)
            with self.assertRaises(api.BundleError):
                self.extract()

    def test_local_reference_validation_rejects_schemes_encoded_paths_and_attributes(self):
        api = self.api()
        for value in (
            "https://untrusted.invalid/x.png", "//untrusted.invalid/x.png",
            "../x.png", "assets/../x.png", "assets/%2e%2e/x", r"assets\x.png",
            'assets/x.png" onerror="bad()', "assets/x.png#fragment",
        ):
            with self.subTest(value=value), self.assertRaises(api.BundleError):
                api.local_uri(value)
        self.assertEqual(api.local_uri("assets/image-1.png"), "assets/image-1.png")


class OfflineTests(BundleTestCase):
    def test_cli_creates_complete_hashed_bundle_and_preserves_raw_source(self):
        result = self.run_bundle("review")
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.manifest()
        self.assertEqual(manifest["status"], "complete")
        self.assertEqual(manifest["revisions"]["status"], "unchanged")
        self.assertFalse(manifest["revisions"]["atomic_snapshot"])
        self.assertEqual(manifest["mode"], "offline")
        self.assertEqual(manifest["rendering"]["status"], "not-requested")
        self.assertEqual(manifest["comments"]["status"], "not-requested")
        bundle = self.root / "review"
        self.assertEqual(
            (bundle / "source.json").read_bytes(),
            (self.fixture / "source.json").read_bytes(),
        )
        artifacts = manifest["artifacts"]
        for name in ("source.json", "document.pdf", "document.docx", "document.md", "index.html"):
            self.assertIn(name, artifacts)
        for name, details in artifacts.items():
            self.assertEqual(
                hashlib.sha256((bundle / name).read_bytes()).hexdigest(),
                details["sha256"],
            )
            self.assertEqual((bundle / name).stat().st_size, details["bytes"])

    def test_html_escapes_injection_and_has_only_existing_local_references(self):
        result = self.run_bundle("review")
        self.assertEqual(result.returncode, 0, result.stderr)
        html = (self.root / "review/index.html").read_text()
        inspection = HTMLInspection(html)
        self.assertNotIn("<script>", html)
        self.assertIn("&lt;script&gt;", html)
        self.assertNotIn("SIGNED_SECRET", html)
        self.assertNotIn("untrusted.invalid", html)
        self.assertTrue(any(tag == "iframe" for tag, _ in inspection.tags))
        for tag, attrs in inspection.tags:
            self.assertNotEqual(tag, "script")
            self.assertFalse(any(name.startswith("on") for name in attrs))
            if tag == "iframe":
                self.assertIn("sandbox", attrs)
        for ref in inspection.references:
            self.assertNotIn(":", ref)
            self.assertNotIn("..", ref)
            self.assertNotIn("%", ref)
            self.assertTrue((self.root / "review" / ref).is_file(), ref)

    def test_native_outline_includes_tables_child_tabs_styles_and_uncertain_images(self):
        result = self.run_bundle("review")
        self.assertEqual(result.returncode, 0, result.stderr)
        html = (self.root / "review/index.html").read_text()
        for text in ("Table cell", "Nested tab paragraph", "HEADING_1", "tab-child"):
            self.assertIn(text, html)
        self.assertIn("<table", html)
        images = self.manifest()["native_images"]
        self.assertEqual([image["object_id"] for image in images], ["native-image", "missing-uri"])
        self.assertFalse(images[1]["content_uri_available"])
        self.assertTrue(all(image["asset"] is None for image in images))
        self.assertTrue(all(image["mapping_confidence"] == "unmapped" for image in images))

    def test_legacy_body_and_table_of_contents_are_traversed(self):
        source = {
            "documentId": "synthetic-doc",
            "body": {
                "content": [
                    {"tableOfContents": {"content": [paragraph("Contents paragraph")]}},
                    paragraph("Legacy paragraph"),
                ]
            },
        }
        (self.fixture / "source.json").write_text(json.dumps(source))
        result = self.run_bundle("review")
        self.assertEqual(result.returncode, 0, result.stderr)
        html = (self.root / "review/index.html").read_text()
        self.assertIn("Contents paragraph", html)
        self.assertIn("Legacy paragraph", html)
        self.assertEqual(self.manifest()["revisions"]["status"], "unknown")

    def test_missing_revision_never_claims_consistency(self):
        (self.fixture / "revision-after.json").unlink()
        result = self.run_bundle("review")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["revisions"]["status"], "unknown")

    def test_missing_required_export_leaves_failed_manifest(self):
        (self.fixture / "document.md").unlink()
        result = self.run_bundle("review")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.manifest()["status"], "failed")
        self.assertFalse((self.root / "review/index.html").exists())

    def test_truncated_pdf_leaves_failed_manifest(self):
        (self.fixture / "document.pdf").write_bytes(b"%PDF-1.4\npartial")
        result = self.run_bundle("review")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.manifest()["status"], "failed")

    def test_existing_directory_is_never_modified(self):
        target = self.root / "review"
        target.mkdir()
        (target / "keep").write_bytes(b"keep exactly")
        result = self.run_bundle("review")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(list(target.iterdir()), [target / "keep"])
        self.assertEqual((target / "keep").read_bytes(), b"keep exactly")

    def test_output_parent_absolute_and_symlink_escape_are_rejected(self):
        (self.root / "link").symlink_to(self.fixture, target_is_directory=True)
        before = sorted(self.fixture.iterdir())
        for path in ("../escape", str(self.root / "absolute"), "link/review", "bad\nname"):
            with self.subTest(path=path):
                result = self.run_bundle(path)
                self.assertNotEqual(result.returncode, 0)
        self.assertEqual(sorted(self.fixture.iterdir()), before)

    def test_fixture_symlink_is_rejected(self):
        (self.fixture / "document.md").unlink()
        (self.fixture / "document.md").symlink_to(self.fixture / "source.json")
        result = self.run_bundle("review")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.manifest()["status"], "failed")

    def test_optional_offline_comments_only_included_when_valid(self):
        (self.fixture / "comments.json").write_text('{"comments":[{"id":"one"}]}')
        result = self.run_bundle("--include-comments", "review")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["comments"]["status"], "available")
        self.assertTrue((self.root / "review/comments.json").is_file())

    def test_missing_optional_comments_are_marked_unavailable(self):
        result = self.run_bundle("--include-comments", "review")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["comments"]["status"], "unavailable")
        self.assertFalse((self.root / "review/comments.json").exists())

    def test_index_write_failure_cannot_publish_complete_manifest(self):
        api = self.api()
        original = api.write_bytes

        def fail_index(path, data):
            if path.name == "index.html":
                raise OSError("simulated full disk")
            return original(path, data)

        old_cwd = Path.cwd()
        os.chdir(self.root)
        try:
            with (
                mock.patch.object(api, "write_bytes", side_effect=fail_index),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                code = api.main(["--from-fixture", "fixtures", "review"])
        finally:
            os.chdir(old_cwd)
        self.assertNotEqual(code, 0)
        self.assertEqual(self.manifest()["status"], "failed")

    def test_partial_comment_write_cannot_leave_a_published_comments_file(self):
        api = self.api()
        (self.fixture / "comments.json").write_text('{"comments":[{"id":"one"}]}')
        original = api.write_bytes

        def fail_comments(path, data):
            if path.name == "comments.json":
                path.write_bytes(b'{"comments":[')
                raise OSError("simulated interrupted write")
            return original(path, data)

        old_cwd = Path.cwd()
        os.chdir(self.root)
        try:
            with (
                mock.patch.object(api, "write_bytes", side_effect=fail_comments),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                code = api.main(["--from-fixture", "fixtures", "--include-comments", "review"])
        finally:
            os.chdir(old_cwd)
        self.assertEqual(code, 0)
        self.assertEqual(self.manifest()["comments"]["status"], "unavailable")
        self.assertFalse((self.root / "review/comments.json").exists())

    def test_nested_output_with_existing_safe_parent_is_supported(self):
        (self.root / "parent").mkdir()
        result = self.run_bundle("parent/review")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest("parent/review")["status"], "complete")

    def test_invalid_required_json_fails_without_echoing_document_content(self):
        (self.fixture / "source.json").write_text('{"error":"PRIVATE_TOKEN\\u001b[31m"}')
        result = self.run_bundle("review")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.manifest()["status"], "failed")
        self.assertNotIn("PRIVATE_TOKEN", result.stderr)

    def test_incomplete_offline_comments_are_unavailable(self):
        (self.fixture / "comments.json").write_text('{"comments":[],"nextPageToken":"more"}')
        result = self.run_bundle("--include-comments", "review")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["comments"]["status"], "unavailable")
        self.assertFalse((self.root / "review/comments.json").exists())

    def test_untrusted_document_urls_never_trigger_network_access(self):
        api = self.api()
        old_cwd = Path.cwd()
        os.chdir(self.root)
        try:
            with (
                mock.patch("socket.socket", side_effect=AssertionError("network forbidden")),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                code = api.main(["--from-fixture", "fixtures", "review"])
        finally:
            os.chdir(old_cwd)
        self.assertEqual(code, 0)
        self.assertEqual(self.manifest()["status"], "complete")


class RenderingTests(BundleTestCase):
    def test_missing_pdftoppm_keeps_pdf_with_explicit_fallback(self):
        result = self.run_bundle("--render-pages", "review")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["rendering"]["status"], "unavailable")
        self.assertEqual(self.manifest()["rendering"]["pages"], [])
        self.assertIn("PDF", (self.root / "review/index.html").read_text())

    def test_successful_renderer_publishes_local_page_previews(self):
        self.executable("pdftoppm", RENDER_STUB)
        result = self.run_bundle("--render-pages", "review")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["rendering"]["status"], "available")
        self.assertEqual(self.manifest()["rendering"]["coverage"], "unverified")
        self.assertEqual(self.manifest()["rendering"]["pages"], ["pages/page-1.png"])
        self.assertIn("pages/page-1.png", self.manifest()["artifacts"])

    def test_zero_exit_page_prefix_has_unverified_coverage_not_complete(self):
        (self.fixture / "document.pdf").write_bytes(pdf_bytes(page_count=2))
        self.executable("pdftoppm", RENDER_STUB)
        result = self.run_bundle("--render-pages", "review")
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.manifest()
        self.assertEqual(manifest["status"], "complete")
        self.assertEqual(manifest["rendering"]["status"], "available")
        self.assertEqual(manifest["rendering"]["coverage"], "unverified")
        self.assertEqual(manifest["rendering"]["pages"], ["pages/page-1.png"])
        html = (self.root / "review/index.html").read_text()
        self.assertIn("Page coverage: unverified", html)
        self.assertIn("previews may omit pages", html)
        self.assertTrue((self.root / "review/document.pdf").is_file())

    def test_renderer_discovered_on_relative_path_runs_in_bundle(self):
        self.executable("pdftoppm", RENDER_STUB)
        for output, search_path in (("relative-path", "bin"), ("absolute-path", str(self.bin))):
            with self.subTest(search_path=search_path):
                result = self.run_bundle("--render-pages", output, PATH=search_path)
                self.assertEqual(result.returncode, 0, result.stderr)
                rendering = self.manifest(output)["rendering"]
                self.assertEqual(rendering["status"], "available")
                self.assertEqual(rendering["coverage"], "unverified")
                self.assertEqual(rendering["pages"], ["pages/page-1.png"])
                self.assertEqual((self.root / output / "pages/page-1.png").read_bytes(), PNG)

    def test_renderer_failure_timeout_and_gap_publish_no_partial_page_list(self):
        self.executable("pdftoppm", RENDER_STUB)
        for mode in ("failed", "timeout", "gap"):
            with self.subTest(mode=mode):
                result = self.run_bundle(
                    "--render-pages", "--timeout",
                    "0.5" if mode == "timeout" else "3", mode, RENDER_MODE=mode
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                rendering = self.manifest(mode)["rendering"]
                self.assertEqual(rendering["status"], "failed")
                self.assertEqual(rendering["pages"], [])
                self.assertFalse((self.root / mode / "pages").exists())
                self.assertEqual(
                    rendering["reason"],
                    "timeout" if mode == "timeout" else "invalid-or-failed-render",
                )


class GwsIntegrationTests(BundleTestCase):
    def test_gws_exports_use_bundle_cwd_relative_files_and_all_tabs(self):
        result = self.run_bundle("review", live=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.manifest()
        self.assertEqual(manifest["status"], "complete")
        self.assertEqual(manifest["versions"]["gws"], "0.0.0-synthetic")
        records = [json.loads(line) for line in (self.root / "gws.log").read_text().splitlines()]
        self.assertTrue(all(record["cwd"] == str(self.root / "review") for record in records))
        self.assertTrue(all(record["sanitize"] == "block" for record in records))
        self.assertEqual(
            [record["args"][:3] for record in records if record["args"] != ["--version"]],
            [
                ["docs", "documents", "get"],
                ["drive", "files", "export"],
                ["drive", "files", "export"],
                ["drive", "files", "export"],
                ["docs", "documents", "get"],
            ],
        )

    def test_changed_revision_is_labeled_mixed(self):
        result = self.run_bundle("review", live=True, STUB_MODE="mixed")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["revisions"]["status"], "mixed")
        self.assertIn("mixed", (self.root / "review/index.html").read_text())

    def test_missing_live_revision_is_unknown(self):
        result = self.run_bundle("review", live=True, STUB_MODE="missing-revision")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["revisions"]["status"], "unknown")

    def test_export_error_status_and_size_mismatch_fail_even_when_file_exists(self):
        for mode in ("bad-export-status", "wrong-export-size"):
            with self.subTest(mode=mode):
                result = self.run_bundle(mode, live=True, STUB_MODE=mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.manifest(mode)["status"], "failed")

    def test_failed_or_missing_export_never_publishes_complete_bundle(self):
        for mode in ("failed-export", "no-export-file"):
            with self.subTest(mode=mode):
                result = self.run_bundle(mode, live=True, STUB_MODE=mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.manifest(mode)["status"], "failed")
                self.assertFalse((self.root / mode / "index.html").exists())
                self.assertNotIn("PRIVATE_TOKEN", result.stderr)
                self.assertNotIn("\x1b", result.stderr)

    def test_gws_timeout_leaves_failed_state(self):
        result = self.run_bundle("--timeout", "0.2", "review", live=True, STUB_MODE="timeout")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.manifest()["status"], "failed")
        self.assertEqual(self.manifest()["error"], "timeout")

    def test_comments_retrieve_all_pages(self):
        result = self.run_bundle("--include-comments", "review", live=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        comments = json.loads((self.root / "review/comments.json").read_text())
        self.assertEqual([comment["id"] for comment in comments["comments"]], ["one", "two"])
        self.assertEqual(self.manifest()["comments"]["status"], "available")

    def test_partial_comments_are_unavailable_not_published(self):
        result = self.run_bundle(
            "--include-comments", "review", live=True, STUB_MODE="failed-comments"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest()["comments"]["status"], "unavailable")
        self.assertFalse((self.root / "review/comments.json").exists())

    def test_serialized_comments_over_limit_remain_an_optional_failure(self):
        result = self.run_bundle(
            "--include-comments", "review", live=True, STUB_MODE="expanded-comments"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.manifest()
        self.assertEqual(manifest["status"], "complete")
        self.assertEqual(manifest["comments"]["status"], "unavailable")
        self.assertEqual(manifest["comments"]["reason"], "comments-size-limit")
        self.assertNotIn("comments.json", manifest["artifacts"])
        self.assertFalse((self.root / "review/comments.json").exists())
        self.assertTrue((self.root / "review/index.html").is_file())


if __name__ == "__main__":
    unittest.main()
