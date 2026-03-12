# Google Docs Cookbook

Hand-written recipes for common Google Docs API patterns. These complement the auto-generated [SKILL.md](./SKILL.md).

## Working with Tabs

Google Docs supports multiple tabs within a single document. The API requires specific patterns for reading, creating, and writing to tabs.

### Reading all tabs

Use `includeTabsContent: true` to get all tabs. Without it, only the first tab's content is returned in the legacy `body` field.

```bash
gws docs documents get \
  --params '{"documentId": "DOC_ID", "includeTabsContent": true}'
```

Response structure:

```text
doc.tabs[]                          # Array of top-level tabs
  .tabProperties.tabId              # Tab identifier (e.g., "t.abc123")
  .tabProperties.title              # Tab display name
  .tabProperties.index              # Position (0-based)
  .documentTab.body.content[]       # Paragraphs, tables, etc.
  .childTabs[]                      # Nested child tabs (same structure)
```

> **Note:** Tabs can be nested. When searching for a tab by title, recursively check `childTabs`.

### Creating a new tab

Use `addDocumentTab` in a batchUpdate request.

```bash
gws docs documents batchUpdate \
  --params '{"documentId": "DOC_ID"}' \
  --json '{"requests": [{"addDocumentTab": {"tabProperties": {"title": "My New Tab"}}}]}'
```

The response includes the new tab's ID:

```text
replies[0].addDocumentTab.tabProperties.tabId → "t.xyz789"
```

> **Gotcha:** The request type is `addDocumentTab`, not `createTab`. Using `createTab` returns a validation error.

### Writing content to a specific tab

Include `tabId` in the `location` object of any content request.

```bash
gws docs documents batchUpdate \
  --params '{"documentId": "DOC_ID"}' \
  --json '{
    "requests": [{
      "insertText": {
        "text": "Hello from a specific tab!\n",
        "location": {"index": 1, "tabId": "TAB_ID"}
      }
    }]
  }'
```

> **Important:** `tabId` must also be included in `range` objects for formatting requests like `updateParagraphStyle`.

### Renaming a tab

```bash
gws docs documents batchUpdate \
  --params '{"documentId": "DOC_ID"}' \
  --json '{
    "requests": [{
      "updateDocumentTabProperties": {
        "tabId": "TAB_ID",
        "documentTabProperties": {"title": "New Title"},
        "fields": "title"
      }
    }]
  }'
```

### Deleting a tab

```bash
gws docs documents batchUpdate \
  --params '{"documentId": "DOC_ID"}' \
  --json '{"requests": [{"deleteTab": {"tabId": "TAB_ID"}}]}'
```

---

## Inserting Formatted Content

The `+write` helper inserts plain text only. For structured content with headings, bold, or styles, use `batchUpdate` with multiple requests.

### Pattern: insert text then apply styles

The key principle: insert all text in a **single request**, then apply formatting using character ranges. This avoids index-shifting issues between requests.

```bash
gws docs documents batchUpdate \
  --params '{"documentId": "DOC_ID"}' \
  --json '{
    "requests": [
      {
        "insertText": {
          "text": "My Heading\nBody paragraph text.\n",
          "location": {"index": 1, "tabId": "TAB_ID"}
        }
      },
      {
        "updateParagraphStyle": {
          "paragraphStyle": {"namedStyleType": "HEADING_1"},
          "range": {"startIndex": 1, "endIndex": 12, "tabId": "TAB_ID"},
          "fields": "namedStyleType"
        }
      }
    ]
  }'
```

### Available paragraph styles

| Named Style | Usage |
|-------------|-------|
| `TITLE` | Document title |
| `HEADING_1` – `HEADING_6` | Section headings |
| `NORMAL_TEXT` | Body text (default) |

### Applying bold or italic

Use `updateTextStyle` with a character range to apply styles like bold or italic.

```bash
gws docs documents batchUpdate \
  --params '{"documentId": "DOC_ID"}' \
  --json '{
    "requests": [{
      "updateTextStyle": {
        "textStyle": {"bold": true},
        "range": {"startIndex": 1, "endIndex": 11, "tabId": "TAB_ID"},
        "fields": "bold"
      }
    }]
  }'
```

Replace `"bold": true` with `"italic": true` for italic, or combine both with `"fields": "bold,italic"`.

### Tips for batch formatting

- **Insert first, format second.** A single `insertText` followed by multiple `updateParagraphStyle`/`updateTextStyle` requests avoids index math headaches.
- **Track positions manually.** Each character (including `\n`) advances the index by 1. Index 1 is the start of the document body.
- **Atomic batches.** All requests in a single `batchUpdate` are atomic — if any request fails, none are applied.
- **Tab targeting.** When writing to a non-default tab, every `location` and `range` must include the `tabId`.

---

## Valid batchUpdate Request Types

Quick reference for all supported request types:

| Category | Requests |
|----------|----------|
| **Tabs** | `addDocumentTab`, `deleteTab`, `updateDocumentTabProperties` |
| **Text** | `insertText`, `deleteContentRange`, `replaceAllText`, `replaceNamedRangeContent` |
| **Formatting** | `updateTextStyle`, `updateParagraphStyle`, `updateDocumentStyle`, `updateSectionStyle` |
| **Lists** | `createParagraphBullets`, `deleteParagraphBullets` |
| **Tables** | `insertTable`, `insertTableRow`, `insertTableColumn`, `deleteTableRow`, `deleteTableColumn`, `mergeTableCells`, `unmergeTableCells`, `updateTableCellStyle`, `updateTableColumnProperties`, `updateTableRowStyle`, `pinTableHeaderRows` |
| **Objects** | `insertInlineImage`, `replaceImage`, `deletePositionedObject`, `insertPerson`, `insertDate` |
| **Structure** | `insertPageBreak`, `insertSectionBreak`, `createHeader`, `deleteHeader`, `createFooter`, `deleteFooter`, `createFootnote` |
| **Named Ranges** | `createNamedRange`, `deleteNamedRange` |

---

## Limitations of `+write`

The `docs +write` helper has two limitations to be aware of:

1. **No tab support** — always appends to the first tab. Use `batchUpdate` with `tabId` in `location` to target a specific tab (see [Writing content to a specific tab](#writing-content-to-a-specific-tab)).
2. **Plain text only** — no formatting. Use `batchUpdate` with `updateParagraphStyle`/`updateTextStyle` for structured content (see [Inserting Formatted Content](#inserting-formatted-content)).
