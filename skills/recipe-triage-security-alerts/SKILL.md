---
name: recipe-triage-security-alerts
version: 1.0.0
description: "List and review Google Workspace security alerts from Alert Center."
metadata:
  openclaw:
    category: "recipe"
    domain: "security"
    requires:
      bins: ["gws"]
      skills: ["gws-alertcenter"]
---

# Triage Google Workspace Security Alerts

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-alertcenter`

List and review Google Workspace security alerts from Alert Center.

## Steps

1. List active alerts: `gws alertcenter alerts list --format table`
2. Get alert details: `gws alertcenter alerts get --params '{"alertId": "ALERT_ID"}'`
3. Acknowledge an alert: `gws alertcenter alerts undelete --params '{"alertId": "ALERT_ID"}'`

