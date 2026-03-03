---
name: recipe-deploy-apps-script
version: 1.0.0
description: "Push local files to a Google Apps Script project."
metadata:
  openclaw:
    category: "recipe"
    domain: "engineering"
    requires:
      bins: ["gws"]
      skills: ["gws-apps-script"]
---

# Deploy an Apps Script Project

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-apps-script`

Push local files to a Google Apps Script project.

## Steps

1. List existing projects: `gws apps-script projects list --format table`
2. Get project content: `gws apps-script projects getContent --params '{"scriptId": "SCRIPT_ID"}'`
3. Update content: `gws apps-script projects updateContent --params '{"scriptId": "SCRIPT_ID"}' --json '{"files": [{"name": "Code", "type": "SERVER_JS", "source": "function main() { ... }"}]}'`
4. Create a new version: `gws apps-script projects versions create --params '{"scriptId": "SCRIPT_ID"}' --json '{"description": "v2 release"}'`

