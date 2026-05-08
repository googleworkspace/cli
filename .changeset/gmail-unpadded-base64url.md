---
"@googleworkspace/cli": patch
---

fix(gmail): accept unpadded base64url from Gmail API. Gmail API returns base64url data without `=` padding in attachment and message body responses (per spec). The previous decoder (`URL_SAFE`) required canonical padding and would silently return an error for real Gmail data with missing padding. Replaced with a custom `URL_SAFE_LENIENT` engine using `DecodePaddingMode::Indifferent`, which accepts both padded and unpadded input without any preprocessing (upstream #774).
