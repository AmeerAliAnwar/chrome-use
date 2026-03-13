---
"agent-browser": minor
---

### New Features
- **Browserless.io provider** -- Added browserless.io as a browser provider, supported in both Node.js and native daemon paths. Connect to remote Browserless instances with `--provider browserless` or `AGENT_BROWSER_PROVIDER=browserless`. Configurable via `BROWSERLESS_API_KEY`, `BROWSERLESS_API_URL`, and `BROWSERLESS_BROWSER_TYPE` environment variables. (#502, #746)
- **`clipboard` command** -- Read from and write to the browser clipboard. Supports `read`, `write <text>`, `copy` (simulates Ctrl+C), and `paste` (simulates Ctrl+V) operations. (#749)
- **Screenshot output configuration** -- New global flags `--screenshot-dir`, `--screenshot-quality`, `--screenshot-format` and corresponding `AGENT_BROWSER_SCREENSHOT_DIR`, `AGENT_BROWSER_SCREENSHOT_QUALITY`, `AGENT_BROWSER_SCREENSHOT_FORMAT` environment variables for persistent screenshot settings. (#749)

### Bug Fixes
- Fixed `wait --text` not working in native daemon path (#749)
- Fixed `BrowserManager.navigate()` and package entry point (#748)
- Fixed extensions not being loaded from `config.json` (#750)
- Fixed scroll on page load (#747)
- Fixed HTML retrieval by using `browser.getLocator()` for selector operations (#745)
