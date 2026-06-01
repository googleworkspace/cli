---
name: yt-search
version: 1.0.0
description: "Search YouTube for videos and return structured metadata including title, URL, channel, view count, likes, engagement ratio, duration, and publish date."
metadata:
  openclaw:
    category: "service"
    domain: "research"
    requires:
      bins: ["yt-search"]
      skills: []
---

# yt-search

Search YouTube for videos and return structured metadata.

## Usage

```
yt-search "<query>" [--limit N] [--fields FIELD,...]
```

### Flags

| Flag | Default | Description |
|---|---|---|
| `--limit N` | `10` | Maximum number of results to return |
| `--fields` | all | Comma-separated list of fields: `title`, `url`, `channel`, `views`, `likes`, `duration`, `published_at`, `description` |

## Examples

```bash
# Search for 10 videos on a topic with all metadata
yt-search "machine learning transformers" --limit 10

# Search with specific fields only
yt-search "rust programming" --limit 5 --fields title,url,channel,views
```

## Output

Returns NDJSON with one object per video:

```json
{
  "title": "Introduction to Transformers",
  "url": "https://youtube.com/watch?v=XXXXXXXXXXX",
  "channel": "AI Explained",
  "views": 1250000,
  "likes": 42000,
  "engagement_ratio": 0.0336,
  "duration": "PT18M42S",
  "published_at": "2024-03-15",
  "description": "In this video we explore..."
}
```

> **Note:** `likes` and `engagement_ratio` are omitted when the channel has hidden like counts.
