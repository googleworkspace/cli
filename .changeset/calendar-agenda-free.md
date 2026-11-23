---
"@googleworkspace/cli": minor
---

`gws calendar +agenda` now shows each event's Google Meet link, and its table and CSV output keep a readable column order. New `gws calendar +free --days 3 --minutes 30 --between 09:00-18:00` lists free slots across your calendars in your account timezone; declined, cancelled and "free" events don't block time, and all-day events only do when out-of-office or with `--count-all-day`.
