# Read structured Google Docs content

```bash
gws docs +read --document DOC_ID
gws docs +read --document DOC_ID --format yaml
gws docs +read --document DOC_ID --dry-run
gws docs +read --document DOC_ID | jq '.outline'
```

`+read` translates the Docs API's nested structural elements into ordered
blocks. It uses the existing authentication, request executor, Model Armor
sanitization, and output formatters. `+write` is unchanged.

The request uses `includeTabsContent=true` and
`suggestionsViewMode=SUGGESTIONS_INLINE`. Pass other API options through
`--params`. To prevent omitted content from looking like an empty document,
`fields` must be absent or exactly `"*"`. `$fields`, other tab/suggestion views,
conflicting document IDs, and non-JSON `alt` responses are rejected before
authentication. Dry-run prints the executor's request plan without acquiring
credentials, fetching document content, or invoking Model Armor.

## Output

- The root retains `documentId`, `title`, `revisionId`, `suggestionsViewMode`,
  and `_sanitization` when returned. No revision is invented when absent.
- `tabs` and recursive `childTabs` preserve API order, IDs, titles, and parents.
  Each tab has ordered `blocks`. `source: "legacyBody"` means the response had
  no populated tabs; that fallback cannot confirm coverage of other tabs.
- Paragraph blocks have `text`, ordered `elements`, and `paragraphStyle`.
  Text elements retain separate runs, text styles, links (including tab-aware
  internal links), and suggested insertion/deletion/style changes. Other
  returned paragraph metadata, such as bullets and positioned object IDs,
  stays on the block.
- Table blocks have `rowCount`, `columns`, and
  `rows[].cells[].blocks`, including nested tables. Row/cell styles and
  suggestion metadata are retained.
- Each tab's `figures` map contains inline and positioned object metadata.
  `embeddedObject` retains alt text, dimensions, and image properties when
  available. Figure elements reference `objectId`. Missing metadata or
  `contentUri` does not remove the reference.
- Headers, footers, and footnotes remain separate maps with their own `blocks`.
  Footnote references and structural markers remain in content order.
  Unsupported elements use `type: "unknown"` with their original `data`.
- `outline` contains titles, subtitles, and headings with text, style level,
  tab ID, heading ID when present, source indices, and a JSON Pointer `path`
  to the normalized paragraph, including headings in tables and segments.

For example, select a heading by its returned ID:

```bash
gws docs +read --document DOC_ID |
  jq '.. | objects | select(.paragraphStyle?.headingId? == "HEADING_ID")'
```

To select the blocks between two top-level headings in a particular tab:

```bash
gws docs +read --document DOC_ID |
  jq --arg tab TAB_ID --argjson start 10 --argjson end 50 \
    '.. | objects | select(.tabId? == $tab and has("blocks")) |
     .blocks[] | select(.startIndex >= $start and .startIndex < $end)'
```

Use indices actually returned for that tab. `startIndex` and `endIndex` are
UTF-16 offsets in the API's tab/segment, **not** byte or character offsets into
the extracted `text`. The text convenience field concatenates text runs only;
figures and other markers remain in `elements`.

## Limits

This is a structured content view, not a visual layout renderer or a lossless
API round trip. It does not resolve inherited styles, render drawings or
equations, download images, or accept/reject suggestions. Proposed deletions
remain inline. Image URIs are included only when returned and may expire.
Use raw `gws docs documents get` for other views or partial field masks.

JSON is the default and retains the entire normalized tree. YAML uses the
existing serializer. Table and CSV use the existing formatter's first
nonempty-array summary (typically the outline, otherwise tabs); table cells
can be truncated. Use JSON for complete downstream processing.

Usage and limitations also live in the command's help, the source consumed by
`gws generate-skills`. Generated skill files are maintained by the repository's
Generate Skills workflow.
