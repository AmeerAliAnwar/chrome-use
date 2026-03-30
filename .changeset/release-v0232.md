---
"agent-browser": patch
---

### New Features

- **Dashboard session creation** - Sessions can now be created directly from the dashboard UI. A new session dialog provides a unified selector grid for local engines (Chrome, Lightpanda) and cloud providers (Browserbase, Browserless, Browser Use, Kernel) with async creation, loading state, and error display (#1092)
- **Dashboard provider icons** - The session sidebar now shows the provider or engine icon for each session, making it easy to identify which backend a session is using (#1092)

### Bug Fixes

- Fixed **Browser Use** provider using an intermediate API call instead of connecting directly via WSS (`wss://connect.browser-use.com`), which caused connection failures (#1092)
- Fixed **Browserbase** provider not sending an explicit JSON body and `Content-Type` header, causing session creation to fail (#1092)
- Fixed **provider navigation** hanging because `wait_for_lifecycle` waited for page load events that remote providers may not emit. Navigation with `--provider` now automatically sets `waitUntil=none` (#1092)
- Fixed **remote CDP connections** timing out by increasing the CDP connect timeout from 10s to 25s for cloud providers (#1092)
- Fixed **zombie daemon processes** not being cleaned up when a provider connection fails during session creation from the dashboard (#1092)
