---
"@googleworkspace/cli": patch
---

ci(release): add Windows (.zip), Debian (.deb), and RPM (.rpm) packages to release artifacts. Windows binary is packaged as a zip archive. Linux targets additionally generate .deb (via cargo-deb) and .rpm (via cargo-generate-rpm) packages alongside the existing .tar.gz archives.
