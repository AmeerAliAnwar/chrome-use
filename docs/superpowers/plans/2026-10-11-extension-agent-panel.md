# Extension agent panel implementation

The acceptance contract is
[the full design](../specs/2026-10-11-extension-agent-panel-design.md).
All unchecked items remain required. Passing the discovery tests does not
establish a completed side panel or unchanged stealth.
Use English Conventional Commits for this work, as requested by the maintainer.

## Implemented foundation

- [x] Pin and inspect Tiller and Magpie source revisions and record mechanism-level comparisons.
- [x] Native discovery without reading credentials or executing agents.
- [x] Separate executable, config-file and directory evidence from runtime/authentication.
- [x] Native panel request correlation, bounds, disconnect rejection and stale-reply isolation.
- [x] Packaged-extension sender checks and explicit unsupported/malformed replies.
- [x] Fixture coverage for skills-only remnants, non-executables, root variables, Kimi migration, Qoder CN, Cline data roots, Droid home override and Windows Goose/shims.
- [x] FIFO fixtures prove discovery does not open config or credential streams.
- [x] Remote real native-host framed-protocol acceptance with discovery, errors and heartbeat, without Chrome.

## Native service and conversational adapters

The native service and both protocol adapters are now implemented. SSH-host
fixtures verify multiple turns on the same CLI process, native-host disconnects,
service restart with history retained, duplicate message suppression, queued
failure/interruption, questions/approvals, profile isolation and IPC capability
rejection. The fixtures invoke synthetic protocol programs, not real models or
Chrome. Real account, page, startup/version compatibility, Windows ownership and
rendered side-panel acceptance remain below. Full completion is still unproven.

- [ ] Profile-scoped persistent agent service, ownership lock and authenticated local transport.
- [ ] Event journal/cursors, bounded replay and reconnection without duplicate turns.
- [ ] Codex initialize/start/resume/turn lifecycle with correlated requests and strict terminal statuses.
- [ ] Claude stream/control protocol with partial text, tools, approvals, questions and interruption.
- [ ] Backend process-tree cleanup, write failures, startup/exit deadlines and late-event generation checks.
- [ ] Effective executable/version/model/settings readback and model-catalog caching/refresh.
- [ ] Additional runtime adapters with their real capabilities, and custom adapter registration.
- [ ] Browser MCP integration through the existing chrome-use binary, pinned profile/session/tab identity.

## Discovery and configuration

The native implementation now includes public-field readback for the built-in
agent registry and profile-bound `configPlan`, `configApply`, and `configRestore`
requests. `config_edit.rs` checks full-document semantics after lossless edits;
`config_transaction.rs` stores private journals, verifies source identity/hash,
recovers interrupted application and restores only unchanged fields. The native
wire tests use synthetic Claude settings and a temporary relay/journal registry.
This does not establish live model access, provider import, session overlays,
Windows ACL/file-identity acceptance or the side-panel configuration UI.

- [ ] Expand installation resolution to npm-prefix, nvm-default, fnm/mise/asdf, pnpm, Windows registry and custom paths.
- [ ] Refresh/invalidation and bounded version/protocol/authentication probes.
- [ ] Read allow-listed agent fields and report parse/permission errors without exposing credentials.
- [ ] Format-preserving JSON/JSONC/TOML/YAML/env patch planners.
- [ ] Redacted diffs, source hashes, apply/readback, atomic persistence and transaction journal.
- [ ] Symlink/hard-link behavior, permissions, BOM/CRLF and multi-file failure recovery.
- [ ] Windows private-journal ACLs and real file identity; Unix behavior is covered by native fixtures.
- [ ] Conflict-aware restore that preserves later user edits.
- [ ] Session/global configuration choices, presets, provider import previews and secret handles.
- [ ] Custom providers and existing Magpie gateway compatibility, tested per backend protocol.
- [ ] Capability-aware agent/model/effort/tier/tool/environment/working-directory/MCP/skill controls.
- [ ] Saved versus running configuration, drift and restart notices with observed health evidence.

## Side panel experience

- [ ] Toolbar opens the native side panel; retain connection/setup access.
- [ ] Persistent chats, stream rendering, tool outcomes and readable failures.
- [ ] Actual target display and explicit target switching without silent tab/profile drift.
- [ ] Queue edit/remove/send-now, deliberate resume and failure/interruption pause.
- [ ] History, search, rename/delete, crash recovery and lazy transcript loading.
- [ ] Image attachments with limits, normalization, backend input adaptation and cleanup.
- [ ] Skills discovery/library/import, slash picker and actual backend exposure.
- [ ] Native schedules with timezone, overlap, missed-run and restart behavior.
- [ ] Approval/question UI with stable ids and stale-response refusal.
- [ ] Keyboard navigation, accessible controls, narrow/wide layout, scroll retention and rendered acceptance.

## Completion gates

- [ ] Remote Cargo compilation, automated tests, fmt and clippy for final source.
- [ ] Real packaged extension and both backend workflows on the SSH host.
- [ ] Worker/native-service/backend restarts, concurrent conversations and multi-profile isolation.
- [ ] Stealth baseline comparison with panel closed, open/idle, executing and finished.
- [ ] Detector raw results and DOM/property/debugger-command deltas; no added page instrumentation.
- [ ] Both READMEs, help, embedded core skills/references, and Chinese/English site pages.
- [ ] Review final diff and verify actual commit/PR/build state before declaring completion.

Do not install the development extension into the maintainer's Chrome for
fixture acceptance. Do not replace the installed CLI or mutate live agent
configuration merely to test the implementation. Keep test sources, source
hashes and remote receipts tied to the exact revision under examination.
