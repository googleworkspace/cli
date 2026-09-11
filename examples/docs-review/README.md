# Review one Google Docs text patch

`docs_review.py` is a standalone Python standard-library companion to `gws`.
It creates a reviewable plan, checks that the source has not changed, applies
one literal replacement, and verifies the result. It adds no authentication
implementation or Rust commands and needs no other examples.

Requirements: Python 3.10+ on a POSIX system supporting `O_NOFOLLOW` and
directory-relative file access (Linux/macOS), with `gws` on `PATH` for live
reads/apply. Existing `gws` authentication and Model Armor environment settings
are inherited. Offline preview and tests need neither credentials nor `gws`.

## Workflow

All file arguments must be relative to your current working directory, with
existing parent directories. Absolute paths, `..`, symlinks (including inward
symlinks), devices and overwriting an existing plan are rejected. Plan files
are created with mode `0600`. Keep both the plan and its digest from review.

These commands use synthetic placeholder IDs/text. The `plan` command performs
one document read, including every tab and inline suggestions. It never sends
a document mutation. Replace the placeholder IDs only when working on a
document you are authorized to edit.

```sh
# From the repository root. printf deliberately does not append a newline.
printf '%s' 'world' > find.txt
printf '%s' 'reader' > replacement.txt

python3 examples/docs-review/docs_review.py plan \
  --document SYNTHETIC_DOCUMENT_ID --tab t.synthetic \
  --find find.txt --replacement replacement.txt --out reviewed-plan.json

# Inspect the full JSON plan: IDs, revision, literal text, diff, target, digest.
cat reviewed-plan.json

# Local request/diff preview: validates the plan, calls no gws, writes no files.
python3 examples/docs-review/docs_review.py apply \
  --plan reviewed-plan.json --dry-run

# After review: reread, compare, submit once, reread and verify.
python3 examples/docs-review/docs_review.py apply --plan reviewed-plan.json
```

Omit `--tab` only for a document containing exactly one tab. Child tabs count;
titles are not selectors. Use a new `--out` path when regenerating a plan.
An empty replacement file deletes the matched text. Empty finds and no-op
replacements are rejected. Do not use `echo` to prepare text files: it commonly
adds a newline, which this workflow deliberately rejects.

The versioned plan contains the document/tab IDs, exact source revision, source
fingerprint, find/replacement text, expected count `1`, UTF-16 target offsets,
diff, and deterministic SHA-256 digest. It does not contain document titles,
the complete source, surrounding text, credentials, or command strings.
The diff is the exact removed/inserted text, not a full-document preview.
Plans still contain sensitive text if your inputs do; treat them accordingly.

The digest detects accidental edits and binds the review fields together; it
is **not a signature or an authorization mechanism**. Anyone who can replace
the entire plan can recompute it. Protect the reviewed file and compare its
digest with your separately retained review record before applying.

## Supported text and verification

- Exactly one case-sensitive literal occurrence in one tab. Regex is disabled.
  Overlapping occurrences also count as ambiguous.
- The match must fit inside one top-level body paragraph. It may cross text
  style runs. Unicode offsets use UTF-16 code units, including emoji.
- Tables, images, headers, footers, footnotes, other paragraphs and other tabs
  remain in the document. Text in non-body regions still counts toward
  uniqueness: a duplicate in a header or table causes refusal.
- Apply reconstructs the plan from the fresh source and compares every field.
  A changed revision, source, target or digest stops before submission.
- Exactly one `replaceAllText` request is sent with
  `writeControl.requiredRevisionId` and `tabsCriteria.tabIds`. No block deletion,
  full-document reupload, or arbitrary request from a plan is allowed.
- Success requires `occurrencesChanged == 1`, a new returned revision, and a
  reread at that revision. The resulting text, paragraph/structure metadata,
  shifted body indices, image metadata, other tabs, and untouched character
  formatting must match the expected result.

Google controls formatting inheritance **inside the replaced span**. This
example does not set or promise the replacement's character styles. It checks
the replacement text and all formatting outside that span, accepting changes
in text-run splitting that leave character formatting unchanged. Temporary
image `contentUri` values and gws `_sanitization` annotations are excluded from
fingerprints; other image/style metadata remains checked. The verification is
an API snapshot, not a visual rendering or a guarantee against later edits.

## Deliberate limitations

This is a text patch workflow, not a structural document editor. Paragraph
breaks, tabs/control characters, private-use characters and object markers
are rejected in find/replacement files. Moving blocks, inserting tables/images,
editing across images, and targets inside tables, tables of contents, headers,
footers or footnotes are unsupported. Structural requests cannot be supplied
through the plan.

The selected tab must have no unresolved suggestions, named ranges or
bookmarks. Unknown tab regions, unknown structural blocks, equations,
automatic text, rich links and smart chips in the selected tab are refused.
These strict limits avoid interpreting inaccessible text, unstable anchors,
or unsupported element boundaries as a safe replacement. Other tabs are
fingerprinted and verified, not edited. Before normalization, every tab's
paragraph elements must have valid UTF-16 lengths and contiguous ranges.
Recognized regions and structural blocks must have valid object/list shapes,
including table rows and cell content. Malformed snapshots in any tab are
refused before submission or reported as ambiguous after submission.
Unknown metadata is retained for comparison; unknown structural blocks and
extra text-element fields that would be discarded are refused. New API
structures may require an explicit compatibility update.

Each text input is limited to 16 KiB, the plan to 1 MiB, and each gws response
to 16 MiB. Source verification additionally limits total text to 250,000
characters and its estimated expanded style representation to 16 MiB.
Oversized or malformed inputs fail closed. `--timeout SECONDS` bounds each
gws subprocess (default 60, maximum 600); there are no mutation retries.

## Outcomes and recovery

Commands emit JSON. `plan`, offline preview, and a verified apply exit `0`
with status `planned`, `preview`, or `applied`. Preview only validates local
data: it cannot establish that a live revision is still current.

Exit `2` / `refused` means the companion did not submit a document write.
Correct the input or reread the source and review a **new** plan.

Exit `3` / `ambiguous` means a write **may have applied**, or its result could
not be verified or reported successfully. This includes subprocess timeouts,
nonzero write exits (including a concurrent API 400), malformed responses, missing revisions,
unexpected reply counts, failed/mismatching rereads, and interruptions or
output failures after submission. The diagnostic JSON on stderr includes
`mutation_state: "attempted"` or `"confirmed"`. A confirmed mutation followed
by a closed stdout consumer or failed final reporting still exits `3`; it
does not claim that the write was refused or never submitted. Stdout is
flushed before success is returned, so buffered output failures are handled
inside the same outcome check.

The original plan stays unchanged. Inspect the document and revision through
your normal tools; do not blindly rerun apply. Keep the failed plan as your review record and
create a new plan if further work is required. The companion does not persist
a separate attempt journal or prevent an operator from manually rerunning it.

All subprocesses use argument arrays, checked exit codes and timeouts.
Raw gws output is not echoed into errors. Diagnostics produce a fixed,
redacted notice; inspect your gws/Model Armor configuration when it appears.
Model Armor block outcomes stop the workflow; settings are never disabled.
This example uses raw `gws docs documents` methods, whose executor at the
accompanying source revision does not retry requests.

## Tests

```sh
python3 -m unittest discover -s examples/docs-review -p 'test_*.py' -v
```

The suite executes the real companion CLI with a temporary stub `gws` and
synthetic documents. It passes an isolated environment, never reads real
credentials, and never contacts Google. CI runs it independently of Rust
changes on Linux and macOS. Tests cover the request contract, no-write
planning/preview, revision/source checks, Unicode/tabs, paths and plan
tampering, structures/styles, reply counts, ambiguity and plan retention.

## API references

- [Documents.get](https://developers.google.com/workspace/docs/api/reference/rest/v1/documents/get):
  `includeTabsContent` and `suggestionsViewMode`.
- [Document structure](https://developers.google.com/workspace/docs/api/reference/rest/v1/documents):
  indices are measured in UTF-16 code units; revisions are opaque.
- [ReplaceAllTextRequest](https://developers.google.com/workspace/docs/api/reference/rest/v1/documents/request#ReplaceAllTextRequest):
  literal matching and tab criteria.
- [BatchUpdate / WriteControl](https://developers.google.com/workspace/docs/api/reference/rest/v1/documents/batchUpdate):
  stale `requiredRevisionId` requests are rejected with HTTP 400. A required
  revision in the response identifies the revision after application.
