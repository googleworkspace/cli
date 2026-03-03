---
name: recipe-share-job-posting
version: 1.0.0
description: "USE WHEN the user needs to distribute a job posting internally."
metadata:
  openclaw:
    category: "recipe"
    domain: "hiring"
    requires:
      bins: ["gws"]
      skills: ["gws-drive", "gws-chat", "gws-gmail"]
---

# Share a Job Posting with Team

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`, `gws-chat`, `gws-gmail`

USE WHEN the user needs to distribute a job posting internally.

## Steps

1. Upload the job description to Drive: `gws drive +upload job-description.pdf`
2. Announce in the team Chat space: `gws workflow +file-announce --file-id FILE_ID --space spaces/SPACE_ID --message 'New opening: [Position]'`
3. Email to hiring managers: `gws gmail +send --to hiring-team@company.com --subject 'New Job Posting: [Position]'`

