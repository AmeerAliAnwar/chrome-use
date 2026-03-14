---
"agent-browser": minor
---

### Full Native Rust
- **100% native Rust** -- Removed the entire Node.js/Playwright daemon. The Rust native daemon is now the only implementation. No Node.js runtime or Playwright dependency required. (#754)
- **99x smaller install** -- Install size reduced from 710 MB to 7 MB by eliminating the Node.js dependency tree.
- **18x less memory** -- Daemon memory usage reduced from 143 MB to 8 MB.
- **1.6x faster cold start** -- Cold start time reduced from 1002ms to 617ms.
- **Benchmarks** -- Added benchmark suite comparing native vs Node.js daemon performance.
- **Chromium installer hardened** -- Fixed zip path traversal vulnerability in Chrome for Testing installer.

### Bug Fixes
- Fixed `--headed false` flag not being respected in CLI (#757)
- Fixed "not found" error pattern in `to_ai_friendly_error` incorrectly catching non-element errors (#759)
- Fixed storage local key lookup parsing and text output (#761)
- Fixed Lightpanda engine launch with release binaries (#760)
- Hardened Lightpanda startup timeouts (#762)
