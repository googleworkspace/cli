---
"@googleworkspace/cli": patch
---

fix: extract CLA label job into dedicated workflow to prevent feedback loop

The Automation workflow's `check_run: [completed]` trigger caused a feedback
loop — every workflow completion fired a check_run event, re-triggering
Automation, which produced another check_run event, and so on. Moving the
CLA label job to its own `cla.yml` workflow eliminates the trigger from
Automation entirely.
