---
name: gws-youtube
description: "Google YouTube Data API v3: Upload and manage YouTube videos, playlists and channels."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws youtube --help"
---

# youtube (v3)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws youtube <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+upload`](../gws-youtube-upload/SKILL.md) | Upload a video (resumable, with progress); private by default |
| [`+videos`](../gws-youtube-videos/SKILL.md) | List the channel's most recent uploads with views and status |

## API Resources

### abuseReports

  - `insert` — Inserts a new resource into this collection.

### activities

  - `list` — Retrieves a list of resources, possibly filtered.

### captions

  - `delete` — Deletes a resource.
  - `download` — Downloads a caption track.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.
  - `update` — Updates an existing resource.

### channelBanners

  - `insert` — Inserts a new resource into this collection.

### channelSections

  - `delete` — Deletes a resource.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.
  - `update` — Updates an existing resource.

### channels

  - `list` — Retrieves a list of resources, possibly filtered.
  - `update` — Updates an existing resource.

### commentThreads

  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.

### comments

  - `delete` — Deletes a resource.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.
  - `markAsSpam` — Expresses the caller's opinion that one or more comments should be flagged as spam.
  - `setModerationStatus` — Sets the moderation status of one or more comments.
  - `update` — Updates an existing resource.

### i18nLanguages

  - `list` — Retrieves a list of resources, possibly filtered.

### i18nRegions

  - `list` — Retrieves a list of resources, possibly filtered.

### liveBroadcasts

  - `bind` — Bind a broadcast to a stream.
  - `delete` — Delete a given broadcast.
  - `insert` — Inserts a new stream for the authenticated user.
  - `insertCuepoint` — Insert cuepoints in a broadcast
  - `list` — Retrieve the list of broadcasts associated with the given channel.
  - `transition` — Transition a broadcast to a given status.
  - `update` — Updates an existing broadcast for the authenticated user.

### liveChatBans

  - `delete` — Deletes a chat ban.
  - `insert` — Inserts a new resource into this collection.

### liveChatMessages

  - `delete` — Deletes a chat message. A call to this method has a quota cost of 20 units.
  - `insert` — Inserts a new resource into this collection. A call to this method has a quota cost of 20 units.
  - `list` — Retrieves a list of resources, possibly filtered. A call to this method has a quota cost of 5 units.
  - `transition` — Transitions a durable chat event. A call to this method has a quota cost of 20 units.

### liveChatModerators

  - `delete` — Deletes a chat moderator.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.

### liveStreams

  - `delete` — Deletes an existing stream for the authenticated user.
  - `insert` — Inserts a new stream for the authenticated user.
  - `list` — Retrieve the list of streams associated with the given channel. --
  - `update` — Updates an existing stream for the authenticated user.

### members

  - `list` — Retrieves a list of members that match the request criteria for a channel.

### membershipsLevels

  - `list` — Retrieves a list of all pricing levels offered by a creator to the fans.

### playlistImages

  - `delete` — Deletes a resource.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.
  - `update` — Updates an existing resource.

### playlistItems

  - `delete` — Deletes a resource.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.
  - `update` — Updates an existing resource.

### playlists

  - `delete` — Deletes a resource.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.
  - `update` — Updates an existing resource.

### search

  - `list` — Retrieves a list of search resources

### subscriptions

  - `delete` — Deletes a resource.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.

### superChatEvents

  - `list` — Retrieves a list of resources, possibly filtered.

### tests

  - `insert` — POST method.

### thirdPartyLinks

  - `delete` — Deletes a resource.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.
  - `update` — Updates an existing resource.

### thumbnails

  - `set` — As this is not an insert in a strict sense (it supports uploading/setting of a thumbnail for multiple videos, which doesn't result in creation of a single resource), I use a custom verb here.

### videoAbuseReportReasons

  - `list` — Retrieves a list of resources, possibly filtered.

### videoCategories

  - `list` — Retrieves a list of resources, possibly filtered.

### videoTrainability

  - `get` — Returns the trainability status of a video.

### videos

  - `batchGetStats` — Retrieves a batch of VideoStat resources, possibly filtered. BatchGetStats is intentionally not atomic to provide a better user experience.
  - `delete` — Deletes a resource.
  - `getRating` — Retrieves the ratings that the authorized user gave to a list of specified videos.
  - `insert` — Inserts a new resource into this collection.
  - `list` — Retrieves a list of resources, possibly filtered.
  - `rate` — Adds a like or dislike rating to a video or removes a rating from a video.
  - `reportAbuse` — Report abuse for a video.
  - `update` — Updates an existing resource.

### watermarks

  - `set` — Allows upload of watermark image and setting it for a channel.
  - `unset` — Allows removal of channel watermark.

### youtube

  - `v3` — Operations on the 'v3' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws youtube --help

# Inspect a method's required params, types, and defaults
gws schema youtube.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

