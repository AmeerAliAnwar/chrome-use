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

## CLI model inspection

The explicit agentInspect operation queries Codex model/list and account/read
without token refresh, or Claude initialization metadata. It sends no thread or
prompt and transports no account identity/credential fields. Codex pagination
is bounded and partial catalogs are labeled; Claude suggestions use returned
value identifiers and supportedEffortLevels rather than Tiller's fixed aliases.
Magpie's FellBack/ListError distinction informed the explicit unavailable state:
no default list is presented as a successful CLI read. Late UI replies cannot
populate another agent/program, and typed model IDs remain unchanged.

A live local Claude 2.1.296 initialization returned models/value/displayName/
resolvedModel/supportedEffortLevels and account metadata keys. Only key shapes
were inspected; no prompt or model generation was sent. Native synthetic
fixtures verify both protocols, Codex pagination, no conversation creation and
identity/credential stripping. A real Codex 0.161.0 on the SSH build host returned 7 models, a complete
catalog and reportedSignedIn through the native operation. Probe processes and
the isolated service exited with confirmation. No prompt/model generation was
sent. The model work passes 66 Rust tests, 283 extension tests and 3 native
integration tests; remote fmt/clippy pass. These do not establish model execution
access or actual rendered picker acceptance.
Windows ownership support, persistent catalog caches, custom providers, other
runtime adapters and rendered model-picker acceptance remain required.

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

The packaged HTML/CSS/controller and native toolbar behavior are implemented.
Node controller tests cover late transcript/control responses, draft isolation,
uncertain sends, reopen correlation IDs and entry fallback. They do not prove rendered layout, actual
Chrome toolbar opening or native sender compatibility. The new native message
status query reports stored acceptance; an absent ID remains unconfirmed.
Remote native tests pass for stored-message status, including unknown IDs remaining
unconfirmed. Remote fmt/clippy and 276 extension tests pass. Headless screenshot
windows clamp the DOM viewport to 500px; cropped screenshots do not establish
320px layout acceptance. Isolated iframe rendering confirms a real 320px viewport
and no horizontal body/transcript/composer overflow in the dark chat/request
fixture. Screenshot review shortened the queued-send label to keep controls
compact. Light mode, other screens, keyboard/scroll behavior and actual extension
entry/sender acceptance still require verification.
Queue edit/remove and bounded previews are now implemented, with fresh
replacement IDs, original-text conflict checks and accepted-ID tombstones.
Tiller AgentChat.swift distinguishes queue resume from pendingDirect (Ctrl+Enter
stops first); our direct-send flow now persists a single priority intent and waits for
terminal/owned-process-exit confirmation. Ordinary queued messages remain
paused. Duplicate/second intents and unconfirmed recovery are refused. The sidebar keeps
edits inside the queue row rather than merging them into the composer draft.
Remote native fixtures verify both adapters execute edited text, do not execute
removed text and retain exact pre-restart user IDs/texts. Queue checks pass in
59 Rust tests, 3 native integration tests and 280 extension tests; remote fmt and
clippy pass. These do not establish rendered queue controls or real-model use.
Direct-send protocol fixtures cover both backends, including confirmed exit
and process replacement, priority ordering, duplicate intent, paused ordinary
queues and failed startup retaining its message identity. Restart recovery is
covered by state tests. The direct work passes 64 Rust state/configuration tests,
3 native integration tests and 281 extension tests, plus remote fmt/clippy. Real CLI/account, rendered direct/queue controls,
attachments, catalogs and remaining items still require implementation/acceptance.

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

## Release scope and packaged Chrome fixture

The maintainer authorized release closure for existing functionality, with advanced
features deferred. Windows panel writes/processes are gated until private ACL/job
ownership acceptance; discovery/config reads remain available. Native hello
advertises panel capabilities so old relays cannot falsely enable conversations.

Chrome for Testing 155.0.8059.39 on the SSH host loaded the actual extension in an
isolated profile/native-host registry. A trusted extension-page click opened the
native side panel; toolbar behavior readback confirms openPanelOnActionClick and
an empty popup. Real sender metadata reports the panel URL with no sender.tab.
Discovery (14 agents), synthetic model metadata and a complete synthetic Codex
conversation passed through Chrome/native transport. Width/height were 360/870
with no horizontal overflow. This uncovered and repaired late-connect metadata
loading and browser timer receiver defects. This fixture uses a remote-debugging
port and is not stealth evidence, a physical-toolbar-click test, a real model
browser task or final post-main-sync acceptance. Evidence is in the task-owned
SSH fixture directory and bench/panel-extension-check.mjs describes the check.


## Release acceptance follow-up

The real extension opened its native side panel on Chrome for Testing
155.0.8059.39. Real Codex 0.161.0 and Claude Code 2.1.296 each used the existing
local login and chrome_use MCP to open a loopback fixture and click its button.
The independent HTTP counter confirmed one click per task. Codex browser calls
required single-call MCP approvals with visible arguments. Claude required stdio
permission prompts; approval resolution is correlated with tool_use_id.

Relay detector checks passed with the panel closed, open/idle, holding a synthetic
running turn, and after confirmed interruption. Sannysoft, CreepJS (0% headless,
0% stealth), incolumitas, BrowserScan (19 normal rows), worker languages and the
Cloudflare managed challenge passed in all four states. These browsers had no
remote-debugging port. Test-only extension-page helpers opened the panel and
started/stopped the synthetic turn; they are absent from the release ZIP. This is
not a physical-toolbar-click measurement or a claim that panel resizing is
unobservable. Functional CDP tests are separate from this detector evidence.

Evidence: task-owned /tmp/chrome-use-panel-browser.gyx3ja on the SSH host,
panel-stealth-matrix.json, stealth-relay-three.log, stealth-panel-open.log,
stealth-panel-executing.log, stealth-panel-finished.log and real-result.json.
The original full specification remains unfinished: attachments, skills,
schedules, binding the visible tab and Windows write/process support are deferred.
