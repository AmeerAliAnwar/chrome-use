---
"agent-browser": patch
---

### New Features

- **Auto-dismissal for alert and beforeunload dialogs** - JavaScript `alert()` and `beforeunload` dialogs are now automatically accepted to prevent the agent from blocking indefinitely. `confirm` and `prompt` dialogs still require explicit `dialog accept/dismiss` commands. Disable with `--no-auto-dialog` flag or `AGENT_BROWSER_NO_AUTO_DIALOG` environment variable (#1075)
- **Puppeteer browser cache fallback** - Chrome discovery now searches `~/.cache/puppeteer/chrome/` (or `PUPPETEER_CACHE_DIR`) for Chrome binaries, so users with an existing Puppeteer installation can use agent-browser without a separate install step (#1088)
- **Console output improvements** - `console.log` of objects now shows the actual object preview (e.g. `{userId: "abc", count: 42}`) instead of `"Object"`. JSON output includes a raw `args` array for programmatic access (#1040)

### Bug Fixes

- Fixed **same-document navigation** (e.g. SPA hash routing) hanging forever because `wait_for_lifecycle` waited for a `Page.loadEventFired` that never fires on same-document navigations (#1059)
- Fixed **save_state** only capturing cookies and localStorage for the current origin, silently dropping cross-domain data (e.g. SSO/CAS auth cookies). Now uses `Network.getAllCookies` and collects localStorage from all visited origins (#1064)
- Fixed **externally opened tabs** not appearing in `tab list` when using `--cdp` mode. Tabs opened by the user or another CDP client are now detected and tracked (#1042)
- Fixed **dashboard server** not picking up installed files without a restart. `dashboard install` now takes effect immediately on a running server (#1066)
- Fixed **Windows Chrome extraction** failing because zip path normalization used forward slashes while the extraction code expected backslashes (#1088)
