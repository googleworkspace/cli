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

"""Build a local visual review bundle using Python's standard library and gws."""

import argparse
import hashlib
import html
import io
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile


VERSION = "1.0"
MIB = 1024 * 1024
FILE_LIMIT = 20 * MIB
TOTAL_LIMIT = 100 * MIB
MEMBER_COUNT = 2000
MAX_PAGES = 500
EXPORTS = {
    "document.pdf": "application/pdf",
    "document.docx": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "document.md": "text/markdown",
}
W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
A = "{http://schemas.openxmlformats.org/drawingml/2006/main}"
WP = "{http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing}"
R = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"
REL = "{http://schemas.openxmlformats.org/package/2006/relationships}"
LIMITATIONS = [
    "Exports are sequential, not an atomic snapshot; unchanged revisions are observations only.",
    "Native outline includes body paragraphs, tables and nested tabs, not full layout or styling.",
    "Drive export tab coverage is not verified; pages and DOCX figures have no reliable tab mapping.",
    "Page preview coverage is unverified; available previews may omit pages.",
    "DOCX relationships provide document order and nearby text, "
    "not exact captions or native Docs IDs.",
    "Only recognized PNG, JPEG, GIF and WebP media are previewed; "
    "signatures are not full validation.",
    "Headers, footers, notes, charts, vectors and unsupported drawings "
    "may be absent from the outline or figures.",
    "Content URIs and document hyperlinks are never fetched; URLs are redacted from display text.",
    "Raw exports and JSON are sensitive, unsanitized source artifacts "
    "and may contain temporary URLs.",
    "The companion is not a document sanitizer or a sandbox for PDF/image viewers or pdftoppm.",
]


class BundleError(Exception):
    """A fixed, non-sensitive failure code safe for manifests and terminals."""


def relative_parts(value):
    """Conservative portable path subset; do not normalize away traversal."""
    parts = value.split("/")
    if not parts or any(
        part in ("", ".", "..") or not re.fullmatch(r"[A-Za-z0-9_. -]+", part)
        for part in parts
    ):
        raise BundleError("unsafe-relative-path")
    return parts


def directory(value, *, create=False):
    """Walk existing parents with no-follow descriptors (Linux/macOS)."""
    parts = relative_parts(value)
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    fd = os.open(".", flags)
    try:
        for index, part in enumerate(parts):
            if create and index == len(parts) - 1:
                os.mkdir(part, mode=0o700, dir_fd=fd)
            next_fd = os.open(part, flags, dir_fd=fd)
            os.close(fd)
            fd = next_fd
    except OSError:
        raise BundleError("directory-exists-or-unsafe") from None
    finally:
        os.close(fd)
    return Path.cwd().joinpath(*parts)


def read_bytes(path, limit=FILE_LIMIT):
    """Bound reads and reject special files, including symlinks."""
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, "rb") as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
                raise BundleError("file-type-or-size-limit")
            data = stream.read(limit + 1)
    except OSError:
        raise BundleError("missing-or-unsafe-file") from None
    if len(data) > limit:
        raise BundleError("file-size-limit")
    return data


def write_bytes(path, data):
    # Files are new, inside the newly created private bundle directory.
    with path.open("xb") as stream:
        stream.write(data)


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=True, indent=2) + "\n").encode("utf-8")


def parse_json(data):
    try:
        value = json.loads(data)
    except (ValueError, UnicodeError):
        raise BundleError("invalid-json") from None
    if not isinstance(value, dict) or "error" in value:
        raise BundleError("invalid-json-response")
    return value


def publish_manifest(bundle, manifest):
    temporary = bundle / ".manifest.tmp"
    write_bytes(temporary, json_bytes(manifest))
    os.replace(temporary, bundle / "manifest.json")


def local_uri(value):
    """HTML references are generated file names, never document-supplied URIs."""
    if not re.fullmatch(r"[A-Za-z0-9_-]+(?:[./][A-Za-z0-9_-]+)*", value):
        raise BundleError("unsafe-local-uri")
    return value


def display(value):
    text = str(value or "")
    text = re.sub(r"(?i)\b(?:https?|ftp|file|data|javascript):[^\s<>\"']+", "[URL omitted]", text)
    text = re.sub(r"(?i)\bBearer\s+\S+", "[credential omitted]", text)
    text = re.sub(
        r"(?i)\b(?:access_token|authorization|token)\s*[:=]\s*[^\s<>\"']+",
        "[credential omitted]",
        text,
    )
    text = "".join(char for char in text if char in "\n\t" or ord(char) >= 32)
    return html.escape(text, quote=True)


def safe_xml(data):
    # Reject UTF-16/32 (including declaration smuggling via NUL bytes), DTDs and
    # entities before giving XML to the stdlib parser. DOCX normally uses UTF-8.
    if b"\x00" in data or re.search(br"<!\s*(?:DOCTYPE|ENTITY)\b", data, re.I):
        raise BundleError("unsafe-xml")
    try:
        text = data.decode("utf-8-sig")
        declaration = re.search(r"<\?xml[^>]*\bencoding\s*=\s*['\"]([^'\"]+)", text, re.I)
        if declaration and declaration[1].lower() not in ("utf-8", "utf8", "us-ascii"):
            raise BundleError("unsupported-xml-encoding")
        root = ET.fromstring(text)
    except (ET.ParseError, UnicodeError):
        raise BundleError("invalid-xml") from None
    pending = [(root, 0)]
    count = 0
    while pending:
        node, depth = pending.pop()
        count += 1
        if depth > 100 or count > 100_000:
            raise BundleError("xml-complexity-limit")
        pending.extend((child, depth + 1) for child in node)
    return root


def raster_extension(data):
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        return "png"
    if data.startswith(b"\xff\xd8\xff"):
        return "jpg"
    if data.startswith((b"GIF87a", b"GIF89a")):
        return "gif"
    if data.startswith(b"RIFF") and data[8:12] == b"WEBP":
        return "webp"
    return None


def zip_members(source, member_limit, total_limit, member_count):
    """Read a bounded archive without ever extracting its member paths."""
    try:
        with zipfile.ZipFile(io.BytesIO(read_bytes(source))) as archive:
            infos = archive.infolist()
            if len(infos) > member_count:
                raise BundleError("zip-member-count-limit")
            seen = set()
            declared_total = 0
            for info in infos:
                name = info.orig_filename
                parts = name.rstrip("/").split("/")
                kind = stat.S_IFMT(info.external_attr >> 16)
                if (
                    name != info.filename
                    or name.startswith("/")
                    or "\\" in name
                    or ":" in name
                    or any(part in ("", ".", "..") for part in parts)
                    or any(ord(char) < 32 or ord(char) > 126 for char in name)
                    or kind not in (0, stat.S_IFREG, stat.S_IFDIR)
                    or info.flag_bits & 1
                    or info.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)
                    or name.casefold() in seen
                ):
                    raise BundleError("unsafe-zip-member")
                seen.add(name.casefold())
                declared_total += info.file_size
                if info.file_size > member_limit or declared_total > total_limit:
                    raise BundleError("zip-size-limit")
            members = {}
            actual_total = 0
            for info in infos:
                if info.is_dir():
                    continue
                with archive.open(info) as stream:
                    data = stream.read(member_limit + 1)
                actual_total += len(data)
                if (
                    len(data) > member_limit
                    or actual_total > total_limit
                    or len(data) != info.file_size
                ):
                    raise BundleError("zip-size-limit")
                members[info.filename] = data
            return members
    except (zipfile.BadZipFile, RuntimeError, NotImplementedError, OSError, EOFError):
        raise BundleError("invalid-zip") from None


def docx_image_occurrences(document):
    """Visit each blip once, retaining its nearest paragraph and drawing."""
    paragraph_text = {}
    paragraph_index = {}
    drawing_alt = {}
    occurrences = []
    pending = [(document, None, None)]
    while pending:
        node, paragraph, drawing = pending.pop()
        if node.tag == W + "p":
            paragraph = node
            paragraph_index[node] = len(paragraph_text)
            paragraph_text[node] = []
        elif node.tag == W + "drawing":
            drawing = node
        elif node.tag == W + "t" and paragraph is not None:
            paragraph_text[paragraph].append(node.text or "")
        elif node.tag == WP + "docPr" and drawing is not None:
            drawing_alt.setdefault(
                drawing, " ".join(node.get(field, "") for field in ("title", "descr")).strip()
            )
        elif node.tag == A + "blip" and paragraph is not None and drawing is not None:
            occurrences.append((node, paragraph, drawing))
        pending.extend((child, paragraph, drawing) for child in reversed(node))

    texts = ["".join(parts) for parts in paragraph_text.values()]
    for blip, paragraph, drawing in occurrences:
        index = paragraph_index[paragraph]
        nearby = texts[index] or " ".join(
            texts[max(0, index - 1):index] + texts[index + 1:index + 2]
        )
        yield blip, drawing_alt.get(drawing, ""), nearby


def extract_docx(
    source, output, *,
    member_limit=FILE_LIMIT, total_limit=TOTAL_LIMIT, member_count=MEMBER_COUNT,
):
    members = zip_members(source, member_limit, total_limit, member_count)
    # Validate even unused XML before any asset writes.
    trees = {
        name: safe_xml(data)
        for name, data in members.items()
        if name.lower().endswith((".xml", ".rels"))
    }
    document = trees.get("word/document.xml")
    if document is None or document.tag != W + "document":
        raise BundleError("missing-docx-document")
    relationships = {}
    rels = trees.get("word/_rels/document.xml.rels")
    if rels is not None:
        for rel in rels.findall(REL + "Relationship"):
            identity = rel.get("Id")
            if not identity or identity in relationships:
                raise BundleError("ambiguous-docx-relationship")
            relationships[identity] = rel.attrib
    output.mkdir(mode=0o700)
    assets = {}
    for name, data in members.items():
        extension = raster_extension(data)
        if name.startswith("word/media/") and extension:
            filename = f"image-{len(assets) + 1}.{extension}"
            write_bytes(output / filename, data)
            assets[name] = local_uri(f"{output.name}/{filename}")
    figures = []
    for blip, alt, nearby in docx_image_occurrences(document):
        identity = blip.get(R + "embed") or blip.get(R + "link")
        relationship = relationships.get(identity, {})
        asset = None
        availability = "missing-or-unsupported"
        if relationship.get("TargetMode", "").lower() == "external":
            availability = "external-not-fetched"
        elif relationship.get("Type") == R[1:-1] + "/image":
            target = relationship.get("Target", "")
            # Allow only relative media targets in the document's part.
            if (
                target.startswith("media/")
                and not any(p in ("", ".", "..") for p in target.split("/"))
                and not any(char in target for char in "\\:%?#")
            ):
                asset = assets.get(str(PurePosixPath("word") / target))
            if asset:
                availability = "available"
        figures.append({
            "order": len(figures) + 1,
            "relationship_id": identity,
            "asset": asset,
            "alt": alt,
            "nearby_text": nearby[:1000],
            "availability": availability,
            "mapping_confidence": "docx-relationship-only",
            "native_object_id": None,
            "source_tab_id": None,
        })
    return {"figures": figures, "assets": list(assets.values())}


def native_view(source):
    if not isinstance(source.get("body"), dict) and not isinstance(source.get("tabs"), list):
        raise BundleError("missing-document-content")
    tabs = []
    images = []

    def add_tab(tab, depth):
        if depth > 50 or len(tabs) >= 1000:
            raise BundleError("native-tab-limit")
        properties = tab.get("tabProperties", {})
        document = tab.get("documentTab", {})
        tab_id = properties.get("tabId")
        tabs.append({
            "id": tab_id,
            "title": properties.get("title", "Document"),
            "depth": depth,
            "blocks": document.get("body", {}).get("content", []),
        })
        for collection, property_name in (
            ("inlineObjects", "inlineObjectProperties"),
            ("positionedObjects", "positionedObjectProperties"),
        ):
            for object_id, obj in document.get(collection, {}).items():
                embedded = obj.get(property_name, {}).get("embeddedObject", {})
                images.append({
                    "object_id": object_id,
                    "tab_id": tab_id,
                    "title": embedded.get("title", ""),
                    "description": embedded.get("description", ""),
                    "content_uri_available": bool(
                        embedded.get("imageProperties", {}).get("contentUri")
                    ),
                    "asset": None,
                    "mapping_confidence": "unmapped",
                })
        for child in tab.get("childTabs", []):
            add_tab(child, depth + 1)

    if source.get("tabs"):
        for tab in source["tabs"]:
            add_tab(tab, 0)
    else:
        add_tab({"documentTab": source}, 0)
    return tabs, images


def outline_html(blocks, depth=0):
    if depth > 50:
        raise BundleError("native-outline-depth-limit")
    parts = []
    for block in blocks:
        if "paragraph" in block:
            paragraph = block["paragraph"]
            text = "".join(
                element.get("textRun", {}).get("content", "")
                for element in paragraph.get("elements", [])
            )
            style = paragraph.get("paragraphStyle", {}).get("namedStyleType", "NORMAL_TEXT")
            parts.append(f"<p><small>{display(style)}</small> {display(text)}</p>")
        elif "table" in block:
            parts.append("<table><tbody>")
            for row in block["table"].get("tableRows", []):
                parts.append("<tr>")
                for cell in row.get("tableCells", []):
                    parts.append(
                        "<td>" + outline_html(cell.get("content", []), depth + 1) + "</td>"
                    )
                parts.append("</tr>")
            parts.append("</tbody></table>")
        elif "tableOfContents" in block:
            parts.append(outline_html(block["tableOfContents"].get("content", []), depth + 1))
    return "".join(parts)


def index_html(source, markdown, tabs, manifest, artifacts):
    parts = [
        '<!doctype html><html lang="en"><head><meta charset="utf-8">',
        '<meta name="viewport" content="width=device-width,initial-scale=1">',
        '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; '
        "img-src 'self'; style-src 'unsafe-inline'; frame-src 'self'; "
        "base-uri 'none'; form-action 'none'\">",
        "<title>Docs review bundle</title>",
        "<style>body{font:16px/1.5 system-ui,sans-serif;max-width:1100px;margin:2rem auto;"
        "padding:0 1rem;color:#182334;background:#fafbfd}h1,h2,h3{line-height:1.2}"
        "section,figure{background:white;border:1px solid #ccd4df;border-radius:8px;"
        "padding:1rem;margin:1rem 0}small{color:#526073}img{max-width:100%;max-height:650px;"
        "object-fit:contain}pre{white-space:pre-wrap;overflow-wrap:anywhere}"
        "td{border:1px solid #ccd4df;padding:.5rem}table{border-collapse:collapse}"
        "iframe{width:100%;height:700px;border:1px solid #ccd4df}"
        "a{color:#184e99}</style></head><body>",
        f"<h1>{display(source.get('title', 'Docs review bundle'))}</h1>",
        f"<p>Revision observation: <strong>{display(manifest['revisions']['status'])}</strong>. "
        "Sequential exports; not an atomic snapshot.</p>",
        "<section><h2>Artifacts</h2><ul>",
    ]
    for name in [*artifacts, "manifest.json"]:
        parts.append(f'<li><a download href="{local_uri(name)}">{display(name)}</a></li>')
    parts.extend(
        [
            "</ul><p>Raw files may contain sensitive content and temporary URLs.</p></section>",
            "<section><h2>Native PDF</h2>",
            '<iframe sandbox title="Native PDF preview" src="document.pdf"></iframe>',
            "<p>If your browser blocks the embedded viewer, open the local PDF artifact.</p>",
            f"<p>Raster previews: {display(manifest['rendering']['status'])}; "
            f"{display(manifest['rendering'].get('reason', ''))}</p>",
        ]
    )
    if manifest["rendering"]["pages"]:
        parts.append(
            f"<p>Page coverage: {display(manifest['rendering']['coverage'])}. "
            "The PDF page count has not been verified; previews may omit pages.</p>"
        )
    for page in manifest["rendering"]["pages"]:
        parts.append(f'<img loading="lazy" src="{local_uri(page)}" alt="{display(page)}">')
    parts.append("</section><section><h2>Native outline and tabs</h2>")
    for tab in tabs:
        parts.append(
            f"<h3>{display(tab['title'])}</h3>"
            f"<p>Tab: {display(tab['id'] or 'legacy body')}; "
            f"depth: {tab['depth']}</p>{outline_html(tab['blocks'])}"
        )
    parts.append("</section><section><h2>DOCX figures</h2>")
    for figure in manifest["figures"]:
        parts.append(f"<figure><h3>Figure {figure['order']}</h3>")
        if figure["asset"]:
            parts.append(
                f'<img loading="lazy" src="{local_uri(figure["asset"])}" '
                f'alt="{display(figure["alt"])}">'
            )
        parts.append(
            f"<figcaption>{display(figure['alt'])}</figcaption>"
            f"<p>Availability: {display(figure['availability'])}</p>"
            f"<p>Nearby text (not an exact caption): {display(figure['nearby_text'])}</p>"
            "<p>DOCX relationship only; native object and source tab mapping unknown.</p></figure>"
        )
    parts.append("</section><section><h2>Native image metadata</h2>")
    for image in manifest["native_images"]:
        parts.append(
            f"<p>Object {display(image['object_id'])}, tab {display(image['tab_id'])}: "
            f"{display(image['title'])} {display(image['description'])}. "
            f"contentUri available: {image['content_uri_available']}; asset mapping: unmapped.</p>"
        )
    parts.append(
        "</section><section><h2>Readable Markdown source</h2>"
        f"<pre>{display(markdown)}</pre></section>"
    )
    parts.append(
        f"<section><h2>Comments</h2><p>{display(manifest['comments']['status'])}</p></section>"
    )
    parts.append("<section><h2>Capabilities and limitations</h2><ul>")
    parts.extend(f"<li>{display(item)}</li>" for item in LIMITATIONS)
    parts.append("</ul></section></body></html>")
    return "".join(parts).encode("utf-8")


def run_process(argv, bundle, timeout):
    # File-backed capture bounds memory. No shell and no untrusted diagnostics
    # echoed to the terminal or copied into the manifest/display HTML.
    with tempfile.TemporaryFile(dir=bundle) as output:
        try:
            process = subprocess.run(
                argv,
                cwd=bundle,
                stdout=output,
                stderr=subprocess.DEVNULL,
                stdin=subprocess.DEVNULL,
                timeout=timeout,
                check=False,
            )
        except subprocess.TimeoutExpired:
            raise BundleError("timeout") from None
        except OSError:
            raise BundleError("process-unavailable") from None
        if process.returncode:
            raise BundleError("process-failed")
        if output.tell() > FILE_LIMIT:
            raise BundleError("process-output-limit")
        output.seek(0)
        return output.read(FILE_LIMIT + 1)


def gws_json(executable, bundle, timeout, command, params, output=None):
    argv = [executable, *command, "--params", json.dumps(params), "--format", "json"]
    if output:
        argv.extend(["--output", local_uri(output)])
    data = run_process(argv, bundle, timeout)
    return data, parse_json(data)


def collect_comments(executable, bundle, timeout, document_id):
    comments = []
    seen = set()
    token = None
    total = 0
    for _ in range(100):
        params = {"fileId": document_id, "fields": "nextPageToken,comments", "pageSize": 100}
        if token:
            params["pageToken"] = token
        raw, page = gws_json(executable, bundle, timeout, ["drive", "comments", "list"], params)
        total += len(raw)
        if total > FILE_LIMIT or not isinstance(page.get("comments", []), list):
            raise BundleError("invalid-or-oversized-comments")
        comments.extend(page.get("comments", []))
        token = page.get("nextPageToken")
        if not token:
            return json_bytes({"comments": comments})
        if not isinstance(token, str) or token in seen:
            raise BundleError("incomplete-comments")
        seen.add(token)
    raise BundleError("incomplete-comments")


def render_pages(bundle, timeout, requested):
    result = {"status": "not-requested", "pages": []}
    if not requested:
        return result
    renderer = shutil.which("pdftoppm")
    if not renderer:
        return {"status": "unavailable", "reason": "pdftoppm-not-found; PDF retained", "pages": []}
    try:
        renderer = str(Path(renderer).resolve())
        with tempfile.TemporaryDirectory(prefix=".render-", dir=bundle) as temp:
            staging = Path(temp)
            prefix = str(staging.relative_to(bundle) / "page")
            run_process([renderer, "-png", "-r", "96", "document.pdf", prefix], bundle, timeout)
            pages = {}
            total = 0
            for path in staging.iterdir():
                match = re.fullmatch(r"page-([0-9]+)\.png", path.name)
                if not match:
                    raise BundleError("invalid-render")
                number = int(match[1])
                data = read_bytes(path)
                total += len(data)
                if number in pages or raster_extension(data) != "png" or total > TOTAL_LIMIT:
                    raise BundleError("invalid-render")
                pages[number] = path.name
            if (
                not pages
                or len(pages) > MAX_PAGES
                or sorted(pages) != list(range(1, len(pages) + 1))
            ):
                raise BundleError("invalid-render")
            staging.rename(bundle / "pages")
            return {
                "status": "available",
                "coverage": "unverified",
                "pages": [local_uri("pages/" + pages[number]) for number in sorted(pages)],
            }
    except (BundleError, OSError) as error:
        return {
            "status": "failed", "pages": [],
            "reason": "timeout" if str(error) == "timeout" else "invalid-or-failed-render",
        }


def revision_observation(before, after):
    before_id = before.get("revisionId")
    after_id = after.get("revisionId")
    before_id = before_id if isinstance(before_id, str) and before_id else None
    after_id = after_id if isinstance(after_id, str) and after_id else None
    status = "unknown"
    if before_id and after_id:
        status = "unchanged" if before_id == after_id else "mixed"
    return {"before": before_id, "after": after_id, "status": status, "atomic_snapshot": False}


def build_bundle(args, bundle, manifest):
    artifacts = ["source.json", *EXPORTS]
    manifest["stage"] = "exports"
    if args.from_fixture:
        fixture = directory(args.from_fixture)
        for name in artifacts:
            write_bytes(bundle / name, read_bytes(fixture / name))
        source = parse_json(read_bytes(bundle / "source.json"))
        after_path = fixture / "revision-after.json"
        after = parse_json(read_bytes(after_path)) if after_path.exists() else {}
        if after_path.exists():
            write_bytes(bundle / "revision-after.json", read_bytes(after_path))
            artifacts.append("revision-after.json")
        executable = None
    else:
        executable = shutil.which(args.gws)
        if not executable:
            raise BundleError("gws-not-found")
        executable = str(Path(executable).resolve())
        try:
            version = run_process([executable, "--version"], bundle, args.timeout).decode("utf-8")
            match = re.fullmatch(r"gws ([0-9][A-Za-z0-9.+-]*)\s*", version)
            manifest["versions"]["gws"] = match[1] if match else "unknown"
        except BundleError:
            manifest["versions"]["gws"] = "unknown"
        params = {"documentId": args.document_id, "includeTabsContent": True}
        raw, source = gws_json(
            executable, bundle, args.timeout, ["docs", "documents", "get"], params
        )
        if source.get("documentId") != args.document_id:
            raise BundleError("document-id-mismatch")
        write_bytes(bundle / "source.json", raw)
        for name, mime in EXPORTS.items():
            _, receipt = gws_json(
                executable, bundle, args.timeout, ["drive", "files", "export"],
                {"fileId": args.document_id, "mimeType": mime}, output=name,
            )
            data = read_bytes(bundle / name)
            if (
                receipt.get("status") != "success"
                or receipt.get("saved_file") != str((bundle / name).resolve())
                or str(receipt.get("mimeType", "")).split(";")[0].strip().lower() != mime
                or type(receipt.get("bytes")) is not int
                or receipt["bytes"] != len(data)
            ):
                raise BundleError("invalid-export-receipt")
        raw, after = gws_json(
            executable, bundle, args.timeout, ["docs", "documents", "get"],
            {**params, "fields": "revisionId"},
        )
        write_bytes(bundle / "revision-after.json", raw)
        artifacts.append("revision-after.json")

    manifest["revisions"] = revision_observation(source, after)
    manifest["comments"] = {"status": "not-requested"}
    if args.include_comments:
        try:
            if executable:
                comments = collect_comments(executable, bundle, args.timeout, args.document_id)
            else:
                comments = read_bytes(fixture / "comments.json")
                value = parse_json(comments)
                if not isinstance(value.get("comments"), list) or value.get("nextPageToken"):
                    raise BundleError("incomplete-comments")
            if len(comments) > FILE_LIMIT:
                raise BundleError("comments-size-limit")
            write_bytes(bundle / "comments.json", comments)
            artifacts.append("comments.json")
            manifest["comments"] = {"status": "available"}
        except (BundleError, OSError) as error:
            (bundle / "comments.json").unlink(missing_ok=True)
            manifest["comments"] = {
                "status": "unavailable",
                "reason": (
                    "comments-size-limit"
                    if isinstance(error, BundleError) and str(error) == "comments-size-limit"
                    else "retrieval-incomplete-or-failed"
                ),
            }

    manifest["stage"] = "validate-and-extract"
    pdf = read_bytes(bundle / "document.pdf")
    if not pdf.startswith(b"%PDF-") or b"%%EOF" not in pdf[-1024:]:
        raise BundleError("invalid-or-truncated-pdf")
    markdown = read_bytes(bundle / "document.md").decode("utf-8")
    tabs, native_images = native_view(source)
    extracted = extract_docx(bundle / "document.docx", bundle / "assets")
    manifest["figures"] = extracted["figures"]
    manifest["native_images"] = native_images
    manifest["tabs"] = [{k: v for k, v in tab.items() if k != "blocks"} for tab in tabs]
    artifacts.extend(extracted["assets"])
    manifest["stage"] = "render"
    manifest["rendering"] = render_pages(bundle, args.timeout, args.render_pages)
    artifacts.extend(manifest["rendering"]["pages"])
    manifest["stage"] = "index"
    write_bytes(bundle / "index.html", index_html(source, markdown, tabs, manifest, artifacts))
    artifacts.append("index.html")
    manifest["artifacts"] = {}
    for name in artifacts:
        data = read_bytes(bundle / local_uri(name))
        manifest["artifacts"][name] = {
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        }
    manifest["status"] = "complete"
    manifest["stage"] = "complete"
    publish_manifest(bundle, manifest)


def positive_timeout(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("timeout must be a positive finite number")
    return number


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--document-id", help="Google Docs ID (not a URL)")
    source.add_argument(
        "--from-fixture", help="Relative directory containing captured export files"
    )
    parser.add_argument("output_dir", help="New relative directory within CWD; parents must exist")
    parser.add_argument("--include-comments", action="store_true")
    parser.add_argument(
        "--render-pages", action="store_true", help="Opt in to local pdftoppm rendering"
    )
    parser.add_argument(
        "--timeout", type=positive_timeout, default=60.0,
        help="Seconds per process (default: 60)",
    )
    parser.add_argument(
        "--gws", default="gws", help="Trusted gws executable (default: PATH lookup)"
    )
    args = parser.parse_args(argv)
    if args.document_id and not re.fullmatch(r"[A-Za-z0-9_-]+", args.document_id):
        parser.error("document-id must be a Google Docs ID, not a URL or path")
    bundle = None
    manifest = {
        "schema_version": 1, "status": "in-progress", "stage": "initialize",
        "mode": "offline" if args.from_fixture else "gws",
        "versions": {"companion": VERSION, "python": sys.version.split()[0], "gws": None},
        "capabilities": {
            "all_native_tabs": True, "native_pdf": True, "docx_raster_assets": True,
            "native_markdown": True, "network_image_fetch": False,
            "exact_native_asset_mapping": False, "atomic_snapshot": False,
        },
        "limitations": LIMITATIONS,
    }
    try:
        bundle = directory(args.output_dir, create=True)
        publish_manifest(bundle, manifest)
        build_bundle(args, bundle, manifest)
    except (Exception, KeyboardInterrupt) as error:
        code = str(error) if isinstance(error, BundleError) else "invalid-or-incomplete-bundle"
        if bundle is not None:
            manifest["status"] = "failed"
            manifest["error"] = code
            try:
                publish_manifest(bundle, manifest)
            except OSError:
                # An earlier in-progress manifest remains non-complete if storage fails.
                pass
        print(f"Review bundle failed: {code}.", file=sys.stderr)
        return 1
    print("Review bundle complete. Open index.html in the new output directory.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
