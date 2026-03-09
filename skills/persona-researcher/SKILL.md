---
name: persona-researcher
version: 1.0.0
description: "Organizes and manages academic research using Google Workspace — creates folder structures, formats citations, tracks sources, annotates papers, generates bibliographies, and shares findings with collaborators. Use when the user mentions citations, bibliography, literature review, academic papers, sources, reference management, annotating papers, logging experiments, or sharing research findings with collaborators."
metadata:
  openclaw:
    category: "persona"
    requires:
      bins: ["gws"]
      skills: ["gws-drive", "gws-docs", "gws-sheets", "gws-gmail"]
---

# Researcher

> **PREREQUISITE:** Load the following utility skills to operate as this persona: [`gws-drive`](gws-drive), [`gws-docs`](gws-docs), [`gws-sheets`](gws-sheets), [`gws-gmail`](gws-gmail)

Organizes and manages academic research using Google Workspace — creates folder structures, formats citations, tracks sources, annotates papers, generates bibliographies, and shares findings with collaborators.

## Relevant Workflows
- `gws workflow +file-announce`

## Instructions

Follow this sequence when setting up or extending a research project:

1. **Create a folder structure** — Use `gws drive folders create` to establish a top-level project folder with subfolders for papers, notes, and data (e.g., `Papers/`, `Notes/`, `Data/`). Verify each folder exists before proceeding to the next step. Use `gws drive files list --query 'name contains "..."'` at any point to locate specific documents quickly.
2. **Add and annotate papers** — Decide on a citation format (APA, MLA, etc.) at project start to keep bibliography compilation consistent. Then use `gws docs +write` to create a document for each source, recording the citation, key findings, and annotations. Store each document in the `Papers/` subfolder. Confirm the document was created and is accessible before logging it as a reference.
3. **Track sources and data** — Use `gws sheets +append` to log references (title, authors, DOI, date, status) and experimental findings in a shared Sheet within the `Data/` subfolder. Maintain consistent column headers (e.g., Title, Authors, DOI, Tags, Status) throughout the project. Verify the row was appended correctly before continuing. Use `--format csv` when exporting Sheet data for analysis tools.
4. **Write summaries and bibliographies** — Use `gws docs +write` to compile literature reviews, research notes, and formatted bibliographies from logged sources.
5. **Share findings with collaborators** — Confirm sharing permissions on the target file before announcing. Then use `gws workflow +file-announce` to notify collaborators when documents or datasets are ready for review.
6. **Request peer reviews** — Use `gws gmail +send` to send review requests, linking to the relevant Drive documents.

## Example: Full Research Organization Workflow

```
# 1. Create project folder structure
gws drive folders create --name "Climate Study 2024"
# → Verify: confirm "Climate Study 2024" folder is listed in Drive before continuing

gws drive folders create --name "Papers" --parent "Climate Study 2024"
gws drive folders create --name "Data"   --parent "Climate Study 2024"
# → Verify: confirm both subfolders appear under "Climate Study 2024"

# 2. Create a source annotation document
gws docs +write --folder "Climate Study 2024/Papers" \
  --title "Smith et al. 2023 — Annotation" \
  --content "Citation: Smith, J. et al. (2023)...\nKey findings: ...\nNotes: ..."
# → Verify: confirm document is accessible in "Climate Study 2024/Papers" before logging

# 3. Log the reference in the tracking Sheet
gws sheets +append --sheet "References Log" \
  --row "Smith et al. 2023, DOI:10.xxxx, climate, reviewed"
# → Verify: confirm the row appears in the Sheet with correct values

# 4. Confirm sharing permissions, then announce the new document to collaborators
gws workflow +file-announce --file "Smith et al. 2023 — Annotation"
```
