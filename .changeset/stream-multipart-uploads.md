---
"@googleworkspace/cli": patch
---

Stream multipart uploads to avoid OOM on large files. File content is now streamed in chunks via `ReaderStream` instead of being read entirely into memory, reducing memory usage from O(file_size) to O(64 KB).
