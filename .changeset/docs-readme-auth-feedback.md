---
"@googleworkspace/cli": patch
---

docs: Comprehensive README overhaul addressing user feedback.

Added a Prerequisites section prior to the Quick Start to highlight the optional `gcloud` dependency.
Expanded the Authentication section with a decision matrix to help users choose the correct authentication path.
Added prominent warnings about OAuth "testing mode" limitations (the 25-scope cap) and the strict requirement to explicitly add the authorizing account as a "Test user" (#130).
Added a dedicated Troubleshooting section detailing fixes for common OAuth consent errors, "Access blocked" issues, and `redirect_uri_mismatch` failures.
Included shell escaping examples for Google Sheets A1 notation (`!`).
Clarified the `npm` installation rationale and added explicit links to pre-built native binaries on GitHub Releases.
