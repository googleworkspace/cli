---
"@googleworkspace/cli": patch
---

fix: use block-style YAML sequences in generated SKILL.md frontmatter

Replace flow sequences (`bins: ["gws"]`, `skills: [...]`) with block-style
sequences (`bins:\n  - gws`) in all generated SKILL.md frontmatter templates.

Flow sequences are valid YAML but rejected by `strictyaml`, which the
Agent Skills reference implementation (`agentskills validate`) uses to parse
frontmatter. This caused all 93 generated skills to fail validation.

Fixes #521
