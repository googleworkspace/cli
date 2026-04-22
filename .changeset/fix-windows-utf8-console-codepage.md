---
"@googleworkspace/cli": patch
---

Force UTF-8 console output on Windows by calling `SetConsoleOutputCP(65001)` at startup, preventing mojibake when the system default codepage is CP-1252
