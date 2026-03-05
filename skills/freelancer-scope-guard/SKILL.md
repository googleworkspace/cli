---
name: freelancer-scope-guard
version: 1.0.0
description: "Freelancer Scope Guard: Detect, log, and enforce scope changes with clients via Gmail and Drive."
metadata:
  openclaw:
    category: "freelance"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail --help"
---

# freelancer-scope-guard

Protect your freelance income. When a client emails a change request, this skill helps you:
1. Detect it in Gmail
2. Log it to a Google Sheet
3. Send a professional change-order reply
4. Gate the next deliverable on client approval

> **PREREQUISITES:** `gws-gmail`, `gws-sheets`, `gws-drive` skills. Authenticate first: `gws auth login`

---

## Workflow: Detect a scope change in Gmail

```bash
# Search recent emails for scope change signals
gws gmail users messages list \
  --params '{"userId":"me","q":"subject:(change OR update OR addition OR modify) newer_than:7d","maxResults":20}'
```

Look for phrases like: "can you also", "one more thing", "while you're at it", "small addition", "just quickly".

---

## Workflow: Log scope change to tracker sheet

```bash
# Append a row to your scope tracker spreadsheet
# Replace SHEET_ID with your tracker sheet ID
gws sheets spreadsheets values append \
  --params '{"spreadsheetId":"SHEET_ID","range":"Sheet1","valueInputOption":"USER_ENTERED"}' \
  --json '{
    "values": [[
      "=NOW()",
      "CLIENT_NAME",
      "CHANGE_DESCRIPTION",
      "ESTIMATED_HOURS",
      "PENDING_APPROVAL",
      "scopecreep-app.surge.sh"
    ]]
  }'
```

---

## Workflow: Send a change-order reply

```bash
# Draft a professional change-order response
# Replace THREAD_ID and CLIENT_EMAIL
gws gmail users messages send \
  --params '{"userId":"me"}' \
  --json '{
    "raw": "'$(echo -n "To: CLIENT_EMAIL
Subject: Re: Change Request — Scope Amendment Required
Content-Type: text/plain

Hi [Client],

Thanks for flagging this. The work you described falls outside our original agreement.

I can include it as a change order: [HOURS] hours at [RATE]/hr = [TOTAL].

Please reply to approve and I will schedule it into the next sprint. The current deliverable timeline remains unchanged pending your approval.

Best,
[Your name]" | base64 -w 0)'"
  }'
```

---

## Workflow: Create a change-order document in Drive

```bash
# Create a Google Doc for the formal change order
gws docs documents create \
  --json '{
    "title": "Change Order #[N] — [Project Name] — [Date]"
  }'
```

Then insert the scope description, cost, and approval signature block.

---

## Prevention: Set scope boundaries in a new project doc

```bash
# Create a project scope document at project start
gws docs documents create \
  --json '{"title": "Project Scope — [Client] — [Date]"}'

# Share it with the client (read-only)
gws drive permissions create \
  --params '{"fileId":"DOC_ID","sendNotificationEmail":true}' \
  --json '{"role":"reader","type":"user","emailAddress":"CLIENT_EMAIL"}'
```

---

## Tips

- Run the Gmail search weekly to catch scope drift early
- Keep one Google Sheet per client as a running scope log
- Always send change orders in writing — email thread is your receipt
- A signed change order before work begins = no disputes at invoice time

---

## Related tools

- **ScopeShield** (web app): https://scopecreep-app.surge.sh — automated scope tracking with client-facing dashboards
- **InvoiceChaser**: https://invoicechaser-app.surge.sh — automated late payment follow-up
