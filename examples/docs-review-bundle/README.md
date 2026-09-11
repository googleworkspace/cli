# Visual Docs review bundle

This standalone companion combines native Docs JSON, Drive PDF/DOCX/Markdown
exports, DOCX raster assets, and a local HTML review index. It uses only the
Python standard library and existing `gws` commands. It does not change `gws`,
authenticate separately, or download document hyperlinks or image `contentUri`s.

## Run

Requires Python 3.10+ on Linux or macOS and an authenticated `gws` executable
with read access to the document through both Docs and Drive.

From the repository root:

```sh
python3 examples/docs-review-bundle/docs_review_bundle.py \
  --document-id DOCUMENT_ID review-bundle
```

`review-bundle` must be a **new relative directory inside the current working
directory**. Existing directories, absolute paths, `..`, symlink components,
control characters, and names outside the portable ASCII subset are refused.
Nested paths work when their parents already exist. The final directory is
created with mode `0700`; keep its parents under your control.

Optional orchestration flags:

```sh
python3 examples/docs-review-bundle/docs_review_bundle.py \
  --document-id DOCUMENT_ID --include-comments --render-pages \
  --timeout 120 --gws /trusted/path/to/gws another-review-bundle
```

- `--include-comments`: collect every returned Drive comments page. The entire
  optional artifact is omitted and marked unavailable if retrieval is incomplete
  or fails, or if the final serialized artifact exceeds 20 MiB. Size is checked
  before publication, so an oversized optional result does not fail the required
  bundle. Comments are a separate observation, not revision-bound or mapped to
  PDF coordinates. Deleted comments are not requested.
- `--render-pages`: use a trusted `pdftoppm` on `PATH` to generate 96 DPI PNGs.
  Its path is resolved before running inside the bundle, including relative
  `PATH` entries. Rendering is opt-in. Missing tools, failures, timeouts, invalid
  output and noncontiguous page numbers retain the PDF and report no available
  raster previews. Outputs from these detected failures are discarded. After a
  successful process exit and validation, previews are labeled `available`
  with `coverage: "unverified"`; a missing page suffix cannot be detected.
- `--timeout`: positive finite seconds per subprocess; default 60. This is not
  a total workflow deadline.
- `--gws`: trusted executable, resolved before changing subprocess working
  directories. Existing `gws` authentication and Model Armor environment
  settings are inherited. No credential values or raw process diagnostics
  are inserted into HTML or error messages.

The companion requests `docs documents get` with `includeTabsContent: true`
and uses `drive files export` with `--format json` and fixed relative
`--output` filenames. Every `gws` subprocess runs inside the new bundle.
Export success, MIME type, destination and byte count are checked against the
written artifact. The receipt must name the exact canonical absolute destination,
as returned by `gws`; a matching basename alone is insufficient. This requires
the existing `gws` binary-export receipt format.

Open `index.html` locally. The index has no JavaScript or remote dependencies.
It contains a sandboxed PDF frame, optional page images, a paragraph/table
outline grouped by native tabs, DOCX figures and nearby text, native object
metadata, and escaped Markdown source. Some browsers block local PDF frames;
use the PDF artifact link in that case. Markdown is readable escaped source,
not rendered Markdown.

## Artifacts and completion

| File | Meaning |
| --- | --- |
| `source.json` | Original Docs JSON response, including all returned tabs |
| `revision-after.json` | Final revision observation, when available |
| `document.pdf`, `document.docx`, `document.md` | Required native Drive exports |
| `comments.json` | Optional fully retrieved comments result |
| `assets/` | Recognized DOCX PNG, JPEG, GIF and WebP media |
| `pages/` | Optional successful local page-rendering output |
| `index.html` | Local review index |
| `manifest.json` | State, versions, capabilities, limitations, mappings and hashes |

The manifest begins as `in-progress` and becomes `complete` only after all
required exports, validation, index generation and artifact hashing succeed.
`complete` means the required bundle files were produced; it does **not** mean
an atomic snapshot, successful optional rendering, or verified export tab
coverage. Check `revisions`, `comments`, and `rendering` independently.

Page previews are never labeled `complete`. `rendering.status: "available"`
means that local PNG files passed validation, while
`rendering.coverage: "unverified"` means the original PDF page count was not
independently checked. Even a contiguous list beginning with page 1 may omit
later pages. The HTML displays this same coverage limitation.

Revision status is `mixed` for differing observed Docs revision IDs, `unchanged`
for equal nonempty IDs, and `unknown` if either is missing. Even `unchanged`
does not prove atomicity or that every export represents the same revision.
The manifest always records `atomic_snapshot: false`.

Required failures return exit code 1 and leave a `failed` manifest with a safe
error code and stage. If writing the failure state also fails (for example,
a full disk), the earlier `in-progress` state can remain. Process termination
can also leave that state. No such bundle should be treated as complete.
Usage errors return 2. Optional failures and mixed/unknown revisions return 0
when the required bundle completes. Existing output directories are never
overwritten; retries need a new name.

SHA-256 and byte counts cover each produced artifact, including the index and
assets. The manifest does not hash itself and is not an authenticity signature.

## Scope and safety limits

- Native JSON traversal includes nested tabs, body paragraphs, tables and
  tables of contents. The outline identifies paragraph styles but does not
  recreate full layout. Headers, footers, notes, lists, equations, charts,
  suggestions and other document features are not fully represented.
- Google determines the PDF/DOCX/Markdown export layout and tab coverage.
  This companion cannot verify that every tab appears in those formats or
  associate a PDF page/DOCX figure with an exact native tab.
- DOCX figure order follows individual image occurrences in the main document,
  including drawings in tables and nested text boxes. Each occurrence uses its
  nearest drawing's alt text and nearest paragraph's text; legitimate repeated
  uses of an asset remain separate figures. Alt text and nearby paragraphs are
  context, **not exact captions**. Native object IDs are recorded separately;
  the companion never fabricates a matching native ID or source tab ID.
- Media basenames are replaced with distinct generated local filenames.
  External, missing, traversing and unsupported relationships remain visibly
  unavailable. Unreferenced recognized rasters are retained as artifacts.
- ZIP validation rejects traversal, absolute/Windows/control-character paths,
  duplicate names (case-insensitive), symlinks and other special files,
  encryption and unsupported compression. It never calls `extractall`.
  Limits are 2,000 members, 20 MiB per member and 100 MiB total uncompressed.
  ZIP paths are restricted to printable ASCII. Only stored/deflate compression
  is accepted.
- Every XML/relationship member is parsed after rejecting DTDs, entities,
  UTF-16/32 and non-UTF-8 encodings. XML trees are limited to 100 levels and
  100,000 nodes per part. The supported DOCX vocabulary is transitional OOXML
  main-document drawings; unsupported content can remain unmapped.
- Each source/export/JSON artifact is capped at 20 MiB. Page output is limited
  to 500 files and 100 MiB total. PDF header/EOF and raster signatures are
  checked; these are **not full format validation**. Renderer exit success and
  contiguous numbering do not independently prove the original PDF page count,
  so preview coverage is always labeled unverified.
- Subprocess capture is file-backed, with size checks after exit. Timeouts and
  post-render limits do not enforce disk or memory quotas on external tools.
  `pdftoppm`, `gws`, local viewers and the operator-controlled parent directory
  are trusted. This is not a sandbox against a concurrent local attacker.
- Display text is HTML/attribute escaped; URI references use generated local
  allowlisted names. The index has a restrictive content-security policy and
  no scripts. Source URLs and recognizable bearer/token strings are redacted
  from display text, but this is not a general secret scanner or Model Armor
  replacement. Existing `gws` sanitization behavior is preserved; binary
  exports are not made safe by JSON sanitization.
- Raw artifacts deliberately retain original content, including any temporary
  URLs or sensitive text. Treat the whole directory as sensitive. Review
  external links and active content in native viewers separately; the
  companion never follows them automatically.

## Offline fixtures and visual QA

Offline mode performs no `gws` calls. Supply a relative directory containing
`source.json`, `document.pdf`, `document.docx`, and UTF-8 `document.md`.
`revision-after.json` and `comments.json` are optional; a missing revision
observation remains unknown. Files and path components must not be symlinks.

Generate entirely synthetic fixtures using the test utility, then create a
review bundle suitable for inspecting the HTML:

```sh
python3 -B - <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, "examples/docs-review-bundle")
from test_docs_review_bundle import fixtures
fixtures(Path("synthetic-docs-fixture"))
PY

python3 -B examples/docs-review-bundle/docs_review_bundle.py \
  --from-fixture synthetic-docs-fixture --render-pages synthetic-docs-review
```

These generated fixtures intentionally contain HTML injection strings, an
untrusted synthetic URL, duplicate image basenames, missing image URIs,
nested tabs and table content. They are hand-built test data, not an actual
Google export or proof of cross-format fidelity. No real documents,
credentials or network access are needed. Omit `--render-pages` to avoid
running external software.

## Tests

```sh
python3 -B -m unittest discover \
  -s examples/docs-review-bundle -p 'test_*.py' -v
```

Tests use generated JSON/PDF/DOCX files and explicit `gws`/renderer executables
as stubs. Their subprocess environment omits real authentication settings;
no installed `gws`, real document, or network request is needed for those tests.

To include the real CLI export-contract regression:

```sh
cargo build --locked
GWS_TEST_BINARY="$PWD/target/debug/gws" python3 -B -m unittest discover \
  -s examples/docs-review-bundle -p 'test_*.py' -v
```

This additional test uses cached synthetic Discovery, a dummy token, isolated
configuration and ADC paths, and a loopback HTTP server. It downloads all three
generated exports through the real CLI and checks that the bundle completes.
It never contacts Google or uses real credentials. The dedicated Linux/macOS CI
job builds `gws` and always enables this test; local runs without
`GWS_TEST_BINARY` explicitly skip it.
