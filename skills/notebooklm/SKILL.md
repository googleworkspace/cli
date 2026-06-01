---
name: notebooklm
version: 1.0.0
description: "Create and query Google NotebookLM notebooks: add sources (URLs, docs, YouTube), run AI analysis, and generate deliverables (flashcards, infographics, mindmaps, audio overviews)."
metadata:
  openclaw:
    category: "service"
    domain: "research"
    requires:
      bins: ["notebooklm"]
      skills: []
---

# notebooklm

Create and query Google NotebookLM notebooks for AI-powered research analysis.

## Subcommands

### `notebook create`

Create a new notebook.

```bash
notebooklm notebook create --title "<title>" [--description "<desc>"]
# Returns: { "notebook_id": "...", "url": "https://notebooklm.google.com/..." }
```

### `source add`

Add a URL or file as a source to an existing notebook.

```bash
notebooklm source add --notebook-id <id> --url "<url>" [--label "<label>"]
```

Supported source types: YouTube video URLs, web pages, Google Docs/Drive links, plain text.

### `query`

Ask a question or request analysis against the notebook's ingested sources.

```bash
notebooklm query --notebook-id <id> --prompt "<question or analysis request>"
# Returns: markdown-formatted response with citations
```

### `deliverable create`

Generate a structured output artifact from the notebook.

```bash
notebooklm deliverable create --notebook-id <id> --type <TYPE>
```

| `--type` | Description |
|---|---|
| `flashcards` | Q&A flashcard deck covering key concepts |
| `infographic` | Visual summary of main themes |
| `mindmap` | Connected concept map |
| `audio` | Conversational audio overview (two-host format) |

Returns a URL or local file path for the generated artifact.

## Example

```bash
# Create a notebook and add two YouTube sources
NB=$(notebooklm notebook create --title "AI Research" | jq -r .notebook_id)
notebooklm source add --notebook-id "$NB" --url "https://youtube.com/watch?v=abc123" --label "Intro to LLMs"
notebooklm source add --notebook-id "$NB" --url "https://youtube.com/watch?v=def456" --label "Transformer Architecture"

# Query the notebook
notebooklm query --notebook-id "$NB" --prompt "What are the key differences between encoder-only and decoder-only transformers?"

# Generate flashcards
notebooklm deliverable create --notebook-id "$NB" --type flashcards
```
