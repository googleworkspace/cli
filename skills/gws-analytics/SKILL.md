---
name: gws-analytics
description: "Google Analytics Data: Google Analytics 4 reports and audience data."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws analytics --help"
---

# analytics (v1beta)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws analytics <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+report`](../gws-analytics-report/SKILL.md) | GA4 audience report: users, sessions and views by a dimension |
| [`+properties`](../gws-analytics-properties/SKILL.md) | List the Analytics accounts and GA4 properties you can access |

## API Resources

### properties

  - `batchRunPivotReports` — Returns multiple pivot reports in a batch. All reports must be for the same Google Analytics property.
  - `batchRunReports` — Returns multiple reports in a batch. All reports must be for the same Google Analytics property.
  - `checkCompatibility` — This compatibility method lists dimensions and metrics that can be added to a report request and maintain compatibility. This method fails if the request's dimensions and metrics are incompatible. In Google Analytics, reports fail if they request incompatible dimensions and/or metrics; in that case, you will need to remove dimensions and/or metrics from the incompatible report until the report is compatible. The Realtime and Core reports have different compatibility rules.
  - `getMetadata` — Returns metadata for dimensions and metrics available in reporting methods. Used to explore the dimensions and metrics. In this method, a Google Analytics property identifier is specified in the request, and the metadata response includes Custom dimensions and metrics as well as Universal metadata. For example if a custom metric with parameter name `levels_unlocked` is registered to a property, the Metadata response will contain `customEvent:levels_unlocked`.
  - `runPivotReport` — Returns a customized pivot report of your Google Analytics event data. Pivot reports are more advanced and expressive formats than regular reports. In a pivot report, dimensions are only visible if they are included in a pivot. Multiple pivots can be specified to further dissect your data.
  - `runRealtimeReport` — Returns a customized report of realtime event data for your property. Events appear in realtime reports seconds after they have been sent to the Google Analytics. Realtime reports show events and usage data for the periods of time ranging from the present moment to 30 minutes ago (up to 60 minutes for Google Analytics 360 properties).
  - `runReport` — Returns a customized report of your Google Analytics event data. Reports contain statistics derived from data collected by the Google Analytics tracking code. The data returned from the API is as a table with columns for the requested dimensions and metrics. Metrics are individual measurements of user activity on your property, such as active users or event count. Dimensions break down metrics across some common criteria, such as country or event name.
  - `audienceExports` — Operations on the 'audienceExports' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws analytics --help

# Inspect a method's required params, types, and defaults
gws schema analytics.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

