---
"agent-browser": patch
---

### Bug Fixes

- **WebSocket keepalive for remote browsers** - Added WebSocket Ping frames and TCP `SO_KEEPALIVE` to prevent CDP connections from being silently dropped by intermediate proxies (reverse proxies, load balancers, service meshes) during idle periods (#936)
- **XPath selector support** - Fixed element resolution to correctly handle the `xpath=` selector prefix (#908)

### Performance

- **Fast-path for identical snapshots** - Short-circuits the Myers diff algorithm when comparing a snapshot to itself, avoiding unnecessary computation in retry and loop workloads where repeated identical snapshots are common (#922)

### Documentation

- Migrated page metadata from MDX files to `layout.tsx` (#904)
- Added search functionality and color improvements to docs (#927)
- Fixed desktop browser list in the iOS comparison table (#926)
- Created a new `providers/` section with dedicated provider pages (#928)
