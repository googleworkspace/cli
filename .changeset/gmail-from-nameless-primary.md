---
"@googleworkspace/cli": patch
---

fix(gmail): omit the From header when the default send-as identity is the primary address with no display name, so Gmail stamps the account's name instead of sending a bare address
