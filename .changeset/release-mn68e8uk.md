---
"agent-browser": patch
---

### Bug Fixes

- **Re-apply download behavior on recording context** - Fixed an issue where downloads were silently dropped in recording contexts because `Browser.setDownloadBehavior` set at launch only applied to the default context. The download behavior is now re-applied when a new recording context is created (#1019)
- **Reap zombie Chrome process and fast-detect crash for auto-restart** - Added a non-blocking process-exit check before attempting CDP connection checks. This prevents a 3-second CDP timeout when Chrome has already crashed or exited, enabling faster detection and auto-restart of the browser (#1023)
- **Route keyboard `type` through text input** - Fixed keyboard `type` subaction to correctly route through the text input handler, and added support for an `insertText` subaction using `Input.insertText` (#1014)
- **Handle `--clear` flag in `console` command** - Fixed the `console` command to accept and process a `clear` parameter, allowing console event history to be cleared (#1015)
