// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Generates SKILL.md files from the CLI's own clap metadata.
//!
//! Usage: `gws generate-skills [--output-dir skills/]`

use crate::commands;
use crate::discovery;
use crate::error::GwsError;
use crate::services;
use clap::Command;
use std::path::Path;

/// Entry point for `gws generate-skills`.
pub async fn handle_generate_skills(args: &[String]) -> Result<(), GwsError> {
    let output_dir = parse_output_dir(args);
    let output_path = Path::new(&output_dir);
    let filter = parse_filter(args);

    // Generate gws-shared skill if no filter or "shared" is in the filter
    if filter.as_ref().is_none_or(|f| "shared".contains(f.as_str())) {
        generate_shared_skill(output_path)?;
    }

    for entry in services::SERVICES {
        let alias = entry.aliases[0];

        let skill_name = format!("gws-{alias}");

        eprintln!("Generating skills for {alias} ({}/{})...", entry.api_name, entry.version);

        // Fetch discovery doc
        let doc = match discovery::fetch_discovery_document(entry.api_name, entry.version).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  WARNING: Failed to fetch discovery doc for {alias}: {e}");
                continue;
            }
        };

        // Build the CLI tree (includes helpers)
        let cli = commands::build_cli(&doc);

        // Collect helper commands (start with '+') and resource commands
        let mut helpers = Vec::new();
        let mut resources = Vec::new();

        for sub in cli.get_subcommands() {
            let name = sub.get_name();
            if name.starts_with('+') {
                helpers.push(sub);
            } else {
                resources.push(sub);
            }
        }

        // Generate service-level skill (only if service itself is in the filter, or no filter)
        let emit_service = match filter {
            Some(ref f) => alias.contains(f.as_str()),
            None => true,
        };
        if emit_service {
            let service_md = render_service_skill(alias, entry, &helpers, &resources);
            write_skill(output_path, &skill_name, &service_md)?;
        }

        // Generate per-helper skills
        for helper in &helpers {
            let helper_name = helper.get_name();
            // +triage -> triage
            let short = helper_name.trim_start_matches('+');
            let helper_key = format!("{alias}-{short}");

            let emit_helper = match filter {
                Some(ref f) => helper_key.contains(f.as_str()),
                None => true,
            };
            if emit_helper {
                let helper_skill_name = format!("gws-{helper_key}");
                let helper_md = render_helper_skill(alias, helper_name, helper, entry);
                write_skill(output_path, &helper_skill_name, &helper_md)?;
            }
        }
    }

    eprintln!("\nDone. Skills written to {output_dir}/");
    Ok(())
}

fn parse_output_dir(args: &[String]) -> String {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--output-dir" {
            if let Some(val) = args.get(i + 1) {
                return val.clone();
            }
        }
    }
    "skills".to_string()
}

/// Parse `--filter <match>` into a substring filter.
fn parse_filter(args: &[String]) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--filter" {
            if let Some(val) = args.get(i + 1) {
                return Some(val.trim().to_string());
            }
        }
    }
    None
}

fn write_skill(base: &Path, name: &str, content: &str) -> Result<(), GwsError> {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).map_err(|e| {
        GwsError::Validation(format!("Failed to create dir {}: {e}", dir.display()))
    })?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, content).map_err(|e| {
        GwsError::Validation(format!("Failed to write {}: {e}", path.display()))
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

fn render_service_skill(
    alias: &str,
    entry: &services::ServiceEntry,
    helpers: &[&Command],
    resources: &[&Command],
) -> String {
    let mut out = String::new();

    // Frontmatter
    out.push_str(&format!(
        r#"---
name: gws-{alias}
version: 1.0.0
description: "USE WHEN the user wants to {description} via the `gws` CLI."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws {alias} --help"
---

"#,
        description = entry.description.to_lowercase(),
    ));

    // Title
    let api_version = entry.version;
    out.push_str(&format!(
        "# {alias} ({api_version})\n\n"
    ));

    out.push_str(
        "> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.\n\n",
    );

    out.push_str(&format!(
        "```bash\ngws {alias} <resource> <method> [flags]\n```\n\n",
    ));

    // Helper commands
    if !helpers.is_empty() {
        out.push_str("## Helper Commands\n\n");
        out.push_str("| Command | Description |\n");
        out.push_str("|---------|-------------|\n");
        for h in helpers {
            let name = h.get_name();
            let short = name.trim_start_matches('+');
            let about = h.get_about().map(|s| s.to_string()).unwrap_or_default();
            // Strip the "[Helper] " prefix if present
            let about = about.strip_prefix("[Helper] ").unwrap_or(&about);
            out.push_str(&format!(
                "| [`{name}`](../gws-{alias}-{short}/SKILL.md) | {about} |\n"
            ));
        }
        out.push('\n');
    }

    // API resources
    if !resources.is_empty() {
        out.push_str("## API Resources\n\n");
        for res in resources {
            let res_name = res.get_name();
            let methods: Vec<String> = res
                .get_subcommands()
                .map(|m| {
                    let mname = m.get_name().to_string();
                    let mabout = m.get_about().map(|s| s.to_string()).unwrap_or_default();
                    format!("  - `{mname}` — {mabout}")
                })
                .collect();

            if methods.is_empty() {
                // Might have sub-resources, list them
                let subs: Vec<String> = res
                    .get_subcommands()
                    .filter(|s| s.get_subcommands().next().is_some())
                    .map(|s| format!("  - `{}`", s.get_name()))
                    .collect();
                if !subs.is_empty() {
                    out.push_str(&format!("### {res_name}\n\n"));
                    for s in subs {
                        out.push_str(&s);
                        out.push('\n');
                    }
                    out.push('\n');
                }
            } else {
                out.push_str(&format!("### {res_name}\n\n"));
                for m in &methods {
                    out.push_str(m);
                    out.push('\n');
                }
                out.push('\n');
            }
        }
    }

    // Discovering commands section
    out.push_str("## Discovering Commands\n\n");
    out.push_str("Before calling any API method, inspect it:\n\n");
    out.push_str(&format!("```bash\n# Browse resources and methods\ngws {alias} --help\n\n# Inspect a method's required params, types, and defaults\ngws schema {alias}.<resource>.<method>\n```\n\n"));
    out.push_str("Use `gws schema` output to build your `--params` and `--json` flags.\n\n");

    out
}

fn render_helper_skill(
    alias: &str,
    cmd_name: &str,
    cmd: &Command,
    entry: &services::ServiceEntry,
) -> String {
    let mut out = String::new();

    let about_raw = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
    let about = about_raw
        .strip_prefix("[Helper] ")
        .unwrap_or(&about_raw);

    let short = cmd_name.trim_start_matches('+');

    // Determine if write command
    let is_write = matches!(
        short,
        "send" | "write" | "upload" | "push" | "insert" | "append"
            | "create-template" | "subscribe"
    );
    let category = if alias == "modelarmor" {
        "security"
    } else {
        "productivity"
    };

    // Frontmatter
    out.push_str(&format!(
        r#"---
name: gws-{alias}-{short}
version: 1.0.0
description: "{about}"
metadata:
  openclaw:
    category: "{category}"
    requires:
      bins: ["gws"]
    cliHelp: "gws {alias} {cmd_name} --help"
---

"#,
    ));

    // Title
    out.push_str(&format!("# {alias} {cmd_name}\n\n"));

    out.push_str(
        "> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.\n\n",
    );

    out.push_str(&format!("{about}\n\n"));

    // Usage
    out.push_str("## Usage\n\n");
    out.push_str(&format!("```bash\ngws {alias} {cmd_name}"));

    // Show required args inline
    let args: Vec<_> = cmd.get_arguments().filter(|a| a.get_id() != "help").collect();
    for arg in &args {
        if arg.is_required_set() {
            if let Some(long) = arg.get_long() {
                let val_name = arg
                    .get_value_names()
                    .and_then(|v| v.first())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "VALUE".to_string());
                out.push_str(&format!(" --{long} <{val_name}>"));
            } else {
                let id = arg.get_id().as_str();
                out.push_str(&format!(" <{id}>"));
            }
        }
    }

    out.push_str("\n```\n\n");

    // Flags table
    if !args.is_empty() {
        out.push_str("## Flags\n\n");
        out.push_str("| Flag | Required | Default | Description |\n");
        out.push_str("|------|----------|---------|-------------|\n");

        for arg in &args {
            let flag = if let Some(long) = arg.get_long() {
                format!("`--{long}`")
            } else {
                format!("`<{}>`", arg.get_id().as_str())
            };

            let required = if arg.is_required_set() { "✓" } else { "—" };

            // Get default value
            let default = arg
                .get_default_values()
                .first()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_else(|| "—".to_string());

            let help = arg
                .get_help()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "—".to_string());

            out.push_str(&format!("| {flag} | {required} | {default} | {help} |\n"));
        }
        out.push('\n');
    }

    // After-help (examples, tips) — format as proper markdown
    if let Some(after) = cmd.get_after_help() {
        let after_str = after.to_string();
        if !after_str.is_empty() {
            let mut in_examples = false;
            let mut in_tips = false;
            let mut examples = Vec::new();
            let mut tips = Vec::new();

            for line in after_str.lines() {
                let trimmed = line.trim();
                if trimmed == "EXAMPLES:" {
                    in_examples = true;
                    in_tips = false;
                    continue;
                }
                if trimmed == "TIPS:" {
                    in_tips = true;
                    in_examples = false;
                    continue;
                }
                if in_examples && !trimmed.is_empty() {
                    examples.push(trimmed.to_string());
                }
                if in_tips && !trimmed.is_empty() {
                    tips.push(trimmed.to_string());
                }
            }

            if !examples.is_empty() {
                out.push_str("## Examples\n\n```bash\n");
                for ex in &examples {
                    out.push_str(ex);
                    out.push('\n');
                }
                out.push_str("```\n\n");
            }

            if !tips.is_empty() {
                out.push_str("## Tips\n\n");
                for tip in &tips {
                    out.push_str(&format!("- {tip}\n"));
                }
                out.push('\n');
            }
        }
    }

    // Write warning
    if is_write {
        out.push_str("> [!CAUTION]\n");
        out.push_str("> This is a **write** command — confirm with the user before executing.\n\n");
    }

    // Cross-reference
    out.push_str(&format!(
        "## See Also\n\n- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth\n- [gws-{alias}](../gws-{alias}/SKILL.md) — All {} commands\n",
        entry.description.to_lowercase(),
    ));

    out
}

fn generate_shared_skill(base: &Path) -> Result<(), GwsError> {
    let content = r#"---
name: gws-shared
version: 1.0.0
description: "Shared patterns, authentication, and global flags for all gws commands."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
---

# gws — Shared Reference

## Installation

The `gws` binary must be on `$PATH`. See the project README for install options.

## Authentication

```bash
# Browser-based OAuth (interactive)
gws auth login

# Service Account
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/key.json
```

## Global Flags

| Flag | Description |
|------|-------------|
| `--format <FORMAT>` | Output format: `json` (default), `table`, `yaml`, `csv` |
| `--dry-run` | Validate locally without calling the API |
| `--sanitize <TEMPLATE>` | Screen responses through Model Armor |

## CLI Syntax

```bash
gws <service> <resource> [sub-resource] <method> [flags]
```

### Method Flags

| Flag | Description |
|------|-------------|
| `--params '{"key": "val"}'` | URL/query parameters |
| `--json '{"key": "val"}'` | Request body |
| `-o, --output <PATH>` | Save binary responses to file |
| `--upload <PATH>` | Upload file content (multipart) |
| `--page-all` | Auto-paginate (NDJSON output) |
| `--page-limit <N>` | Max pages when using --page-all (default: 10) |
| `--page-delay <MS>` | Delay between pages in ms (default: 100) |

## Security Rules

- **Never** output secrets (API keys, tokens) directly
- **Always** confirm with user before executing write/delete commands
- Prefer `--dry-run` for destructive operations
- Use `--sanitize` for PII/content safety screening
"#;

    write_skill(base, "gws-shared", content)
}
