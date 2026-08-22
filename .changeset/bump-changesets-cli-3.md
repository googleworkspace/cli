---
"@googleworkspace/cli": patch
---

chore(deps): bump `@changesets/cli` (devDependency) from 2.29.8 to 3.0.1 to resolve GitHub Dependabot alerts against transitive `js-yaml` (3.14.2, 4.1.1) and `picomatch` (2.3.1) versions pulled in by `@changesets/cli`'s own dependency tree (via `@changesets/parse`, `@manypkg/get-packages`, `@changesets/git`). All three are devDependencies with dev/CI-only exposure — no untrusted input, no impact on the published binary.

v3 changed the default behavior for `"private": true` packages: `changeset version` now silently no-ops for them unless `privatePackages.version` is explicitly enabled in `.changeset/config.json` (verified locally — without this option, `changeset version` reports success but makes no changes; with it, it correctly bumps the version and consumes pending changesets). Since this repository's root package is `"private": true`, that option is added here so `changeset version`/`pnpm run version-sync` keep working.
