# Extension agent panel design

The chrome-use toolbar button opens a persistent Chrome side panel for local
coding agents. The finished product combines Tiller's browser-adjacent agent
workflow with Magpie's agent discovery and configuration management. Codex and
Claude Code use the user's installed CLI and login. Browser actions go through
the existing chrome-use execution path. Opening the panel or configuring an
agent must not add page scripts, debugger attachments, or Runtime-domain capture.

This is the full implementation contract, not a reduced release scope. The
implementation may land in dependency order; no partial component establishes
that the panel, its configuration manager, or stealth acceptance is complete.

## Source baseline

Study the implementation at these revisions, rather than treating README
promises as proven runtime behavior. Upstream changes require a new comparison.

| Project | Revision | Implementation sources |
| --- | --- | --- |
| Tiller | `17f5cd14bb20092d30430fac978cf69b5b2334b9` | `app/Sources/Tiller/AgentSession.swift`, `AgentChat.swift`, `AgentHistory.swift`, `AgentModels.swift`, `AgentProviders.swift`, `AgentSkills.swift`, `AgentSchedules.swift`, `AgentAttachment.swift`, `mcp/src/browser.rs` |
| Magpie | `e9b16ca184cc6164c477e68dc4fe1be5d466e228` | `internal/agent/agent.go`, `agents.go`, `applied.go`, `plugged.go`, `codex.go`, `claude.go`, `qoder.go`, `pi.go`, `internal/edit/`, `internal/provider/discovery.go`, `internal/proc/find.go` |
| chrome-use | `c9c8a524` base | `cli/src/connect.rs`, `cli/src/mcp.rs`, `cli/src/native/browser.rs`, `cli/src/native/stealth.rs`, `extensions/ab-connect/background.js` |

Sources: [Tiller session implementation](https://github.com/sorrycc/tiller/blob/17f5cd14bb20092d30430fac978cf69b5b2334b9/app/Sources/Tiller/AgentSession.swift),
[Magpie agent implementation](https://github.com/yetone/magpie/blob/e9b16ca184cc6164c477e68dc4fe1be5d466e228/internal/agent/agent.go),
[Codex app-server protocol](https://learn.chatgpt.com/docs/app-server).
Both reference projects use MIT licenses. Any source adaptation retains the
required attribution; architecture comparisons do not imply code reuse.

The inspected local interfaces are Codex CLI 0.162.1 and Claude Code 2.1.296.
Codex's generated JSON schemas require threadId and turnId for interruption;
user-input requests include isBlocking and questions, and the answer is a map
keyed by question id. Permission-profile approvals and MCP elicitation are
separate from command/file approvals. The Claude CLI exposes permission-prompts
host/none; its actual host control protocol must be tested with synthetic events
before assuming the SDK callback's shape is the wire format. Current official
[Claude input documentation](https://code.claude.com/docs/en/agent-sdk/user-input)
requires handling both permission requests and AskUserQuestion. Protocol schemas
and CLI help do not establish successful login or model access.

## Tiller mechanisms to preserve or improve

### Agent processes and conversations

`AgentSession` adapts two-way stream JSON for Claude/Qoder, JSON-RPC app-server
for Codex, and separate adapters for Antigravity and Grok. Different CLIs do not
share a pretend universal protocol. A process generation tags asynchronous
output, termination, and writes. An obsolete process cannot update a resumed
conversation. Stdin writes are serialized; failed writes terminate the process
with an explicit error. Its line parser buffers bytes until a newline, avoiding
UTF-8 splits and repeated scanning of partial large lines.

Codex thread startup/resume and turn startup/interrupt are distinct operations.
An interrupt before a turn id exists waits for that id. Tiller retries a resume
refused by an active writer, with a bounded count, and reads actual terminal
turn status. Our adapter must additionally wait for successful initialization
before sending initialized and thread calls, and reject unknown terminal status
as a protocol error rather than interpreting it as completion. Retry a specific
transient resume collision only; never replay a browser mutation or user turn.

Tiller unconditionally declines Codex approval, user-input, and elicitation
requests. Its Claude launch uses dontAsk. The extension instead renders actual
approval and question requests with stable request ids, replies to that request,
and rejects stale replies after disconnect or cancellation. The panel must not
silently enable bypass permissions to hide unsupported interaction.

### Queue and cancellation

`AgentChatView` keeps queued messages separate from the executing turn. A failed
or interrupted turn pauses the queue. Direct steering interrupts before sending
the replacement message. Slash-command boundaries are preserved when batching.
Our queue needs persisted entries, edit/remove/send-now controls, visible pause
reason, and a deliberate resume action. A UI timeout is not evidence that a
process stopped. A stop action stays pending until a terminal event or confirmed
process-tree exit; interruption and forced termination have different labels.

### History and profile isolation

`AgentHistoryStore` stores a panel transcript separately from the CLI's session,
including conversation id, backend session id, working directory, agent/provider,
tools, model options, and timestamps. Pending writes are versioned and transcript
loading is lazy. We need atomic, versioned persistence, corruption reporting,
crash recovery and deletion that never touches another conversation's resources.
A terminated turn cannot resume a queued send on restart without user intent.

Chrome profile, extension installation identity, conversation id, backend thread
id, and chrome-use browser session are separate identities. A conversation pins
the intended relay profile and tab ownership. Switching the user's active tab
while a turn runs does not silently move the agent's target. The UI shows its
actual target; switching target is an explicit operation with live identity
readback. Missing profile/tab fails instead of choosing another logged-in one.

### Models, providers, skills and schedules

`AgentModelCatalog` keeps cached model entries while refreshing off the main
thread. Codex uses model/list; Claude uses named aliases. Model capabilities
control effort/context/fast controls. We distinguish catalog availability from
successful model access; cache age, refresh failure, custom ids and unavailable
features remain visible. Model changes apply to a new turn/session according to
the backend's actual protocol, with its effective model reported back.

`AgentProvider` separates metadata from Keychain secrets and applies endpoint
and environment overrides to the child process. We preserve native CLI login;
new keys remain in a native credential store, not extension localStorage or
Chrome sync. The panel gets opaque credential handles and masked metadata.
Endpoint compatibility must be checked per backend protocol. An existing Magpie
gateway may be used as a configured provider, without making it a prerequisite.
Do not implement subscription token sharing by copying undocumented auth flows.

`AgentSkillStore` separates the panel-owned library from user skills. It validates
names and paths and refuses to overwrite a user skill not imported into the
library. Our panel supports discovery, enable/disable, import, slash completion,
and backend-specific skill exposure. Files and archives cannot escape the
library, including through symlinks. A listed skill and a skill actually loaded
by the backend are different states.

Tiller's scheduled runner records due times, running/finished/failed/skipped
states, and skips overlap with a still-busy prior run. Our schedule execution
belongs to the native service, so closing the sidebar or suspending the extension
worker does not lose it. Persist timezone, next run, outcome and conversation;
report offline/missed/overlap behavior. A scheduled run cannot expand the tool
permissions of the conversation that created it. Attachments need size/count
limits, normalization, ownership and cleanup, with text and image input adapted
to each backend's actual capabilities.

## Magpie mechanisms to preserve or improve

### Evidence based discovery

Magpie represents agent binaries, config paths, fields and options separately.
Its detection excludes directories containing only skills or uninstall remnants.
Windows discovery searches installer directories and updated registry PATH,
including exe and cmd shims. Tests isolate system search paths so installed
programs on the test machine cannot make missing-agent fixtures pass.
Magpie also resolves npm-prefix, nvm-default, fnm/mise/asdf and pnpm installations;
lexically choosing the newest-looking Node directory is not an equivalent
resolution. The extension's native host must find the same executable as the
user's installation while preserving an explicit custom executable choice.

Our discovery returns separate installation, configuration, authentication,
protocol and runtime evidence. A config file is not a working CLI. A login file
is not successful authentication. Version probes have a deadline, bounded output
and a real executable identity; same-named unrelated programs are refused by the
runtime handshake. Respect backend config-root variables and XDG/Windows paths.
Support custom executable paths and custom agent adapters with collision checks.
Discovery must be refreshable after install or removal and expose inspection
errors instead of presenting an unreadable path as a missing installation.

Magpie's account-discovery gate prevents reads and refreshes of login credentials
at the sources and sinks, not just by hiding a provider in the UI. Our filesystem
installation discovery does not open credentials at all. Login status comes from
explicit backend probes; opening a model picker does not require a token scan.

### Configuration planning and application

Magpie's format editors preserve comments, ordering, indentation, BOM and CRLF;
its tests cover JSONC, TOML shapes, literal/multiline strings, YAML, symlinks and
hard links. WriteAtomic resolves symlinks, preserves file mode/group, writes a
temporary file, flushes it and renames. Its hard-link path writes in place with a
recovery file, so it has a different crash guarantee. We must state that tradeoff
or refuse hard-linked writes explicitly; never silently split linked config.

Configuration is a transaction: read and hash the actual source, compute a
format-preserving patch, display a redacted field diff, apply only if the source
still matches, persist backup/journal, read back and validate the changed fields.
Invalid or unreadable source is an error, not an empty default. Multi-file changes
have a recovery journal and rollback of only our writes. Atomic writes alone do
not solve another switcher changing the file between read and commit.

Plugin-session configuration and global CLI configuration are separate choices.
Session overrides leave global defaults intact. Applying globally identifies
which files/fields will change. Restore reverts a field only while it still equals
our last applied value; later user edits are retained and reported as conflicts.
Backend, provider, model, effort, tier aliases, environment, tool permissions,
working directory and MCP/skill settings use per-agent capability declarations.
An unsupported control is unavailable with an explanation, never a fake save.

### Drift and effective behavior

Magpie records applied fields and checks config changes, broken wiring, stale
processes and observed requests bypassing its gateway. Our UI similarly keeps
saved configuration separate from the effective configuration of a running
conversation. A completed write can require restart. Process start records the
actual executable/version and non-secret effective settings. A test turn proves
model access for that request, not perpetual entitlement. Health checks report
what answered and when; do not infer why a provider failed without evidence.

### Additional agents and provider management

Magpie's agent plugins declare detection/config fields and adapters; built-in id
or alias collisions fail. Discovery/configuration adapters and conversational
runtime adapters are separate extension points. Implement installed local agents
with advertised capabilities and add declarative custom configuration adapters;
never promise conversational support from a file-only integration. Core scope
includes Codex/Claude runtime, additional installed agents, model/provider picker,
local config import, configuration profiles, health/readback, backups/restore,
and compatibility with existing Magpie endpoints. Existing provider imports are
previews and explicit selections, not automatic secret transfers.

## Native architecture

The extension page talks only to its MV3 worker. The worker validates the
packaged page's sender identity, correlates native requests, enforces bounded
in-flight work and rejects all pending requests on disconnect. Native replies
must not enter the CDP dispatcher. Unknown operations fail explicitly.

The existing native host can serve read-only discovery and small configuration
operations, independently of relay command dispatch. Long-lived conversations,
queues and schedules require a profile-scoped native agent service: an extension
worker or native-messaging port can disappear. Its owner identity, lock,
reconnect handshake, event cursor, journal and bounded replay are part of the
implementation, not UI-only reconnection logic. A reconnect replays events;
it never resubmits a turn. Keep chrome-use's binary distribution and avoid a
mandatory Node/Go runtime for users.

Native configuration requests use `configRead`, `configPlan`, `configApply`, and
`configRestore`. The host supplies the Chrome profile identity from `hello`, not
from caller parameters. Paths and public fields come from the agent registry;
requests cannot select an arbitrary file or secret field. Journals live in the
profile's private configuration store, optionally scoped by
`CHROME_USE_RELAY_DIR`. That override does not move the agents' own settings.

Plans retain source bytes and resolved identities, show changed fields, and do
not apply settings. Apply validates every source before committing any file and
reads back the complete result. An interrupted apply either confirms that all
planned writes already landed or recovers only files still equal to its own
output. Restore preserves user changes and returns `restoreConflicts` when a
field has changed. That state must render as a partial restore with the affected
fields, not a successful full-restore toast. A running session remains on its
own configuration; saved file values alone cannot establish a live model change.

The format layer uses pinned native CST editors for JSON/JSONC, TOML and YAML;
env files use literal assignment edits without evaluating code. Edits must also
pass whole-document semantic equality against the requested changes. Duplicate
keys, ambiguous parents, unsafe alias writes and invalid configuration are
refused. Interpolated or multiline env values require a native session override,
rather than a file edit that might have a different runtime value. Structured
values inside string-only public fields are flagged without exposing their
contents. Credential-file targets are never accepted through config symlinks.

Browser tools use the existing CLI/MCP execution surface with explicit browser
and session identity. The backend cannot silently switch to another browser
extension, a launched browser, or its own generic browser tools. MCP tool results
and UI event rows preserve observed outcome and uncertainty. Page text is data,
not authority to change local configuration, tool permissions or credentials.

## Stealth acceptance

Native side-panel UI is not part of the webpage DOM, but opening it can resize
the viewport and move focus. Do not claim the panel is unobservable. Test the
allowed resize separately from added fingerprints or instrumentation. Opening,
closing, discovery and configuration add no content scripts, page resources,
main-world properties, cursor overlays, attach calls, Runtime.enable or console
capture. Actual tasks reuse current domain and idle-detach policies. Secrets,
agent events and UI controls never travel through website postMessage bridges.

Run acceptance on the configured SSH GUI host. Compare the same fresh binary,
profile and detector conditions with panel closed, open/idle, task running and
finished, including close/reopen during a turn. Record raw detector results,
page property/DOM deltas, debugger command counts and target identity. Include
CreepJS, Sannysoft, incolumitas, BrowserScan, a managed challenge and challenges
embedded in frames. Public detector variability is reported; a regression cannot
be waived because unit tests pass. Do not operate the maintainer's browser for
fixture tests. Tests that isolate agents do not consume the user's subscriptions.

## Completion evidence

Completion requires the following, all against current source and runtime state:

| Requirement | Authoritative evidence |
| --- | --- |
| Toolbar to persistent side panel | Packaged extension opened on the SSH host; actual click/open/close behavior |
| Real Codex and Claude conversations | Handshake, streaming text/tool events, follow-up/resume, failure, approvals/questions and interruption |
| Other-agent discovery and configuration | Isolated fixture matrix plus real installed-binary/path readback; missing and malformed cases |
| Effective model/provider/settings | Config transaction readback and backend-reported settings; custom-provider compatibility checks |
| Queue, history, attachments and skills | Crash/reopen, edits, queue pause/resume, attachment limits and actual backend-loaded skill evidence |
| Schedules | Native-service restart and sidebar-closed runs; overlap, timezone and missed-run records |
| Profile/tab isolation | Multi-profile/tab tests, no silent target drift, no unrelated window/tab changes |
| Config apply and restore | Conflict, comment/BOM/CRLF, symlink/hard-link, permissions, partial failure and recovery tests |
| Native lifecycle | Host/worker/process exit and restart, bounded events, no duplicate turns or orphan process trees |
| UI quality | Rendered narrow/wide side-panel acceptance, keyboard navigation, readable errors, stable scroll and accessibility |
| Stealth | Four-state baseline comparison and raw detector results on the SSH GUI host |
| Distribution | Remote fmt/tests/clippy/build receipts; embedded skills, both READMEs and Chinese/English site docs in the same release |

A source change, passing unit test, successful CLI exit or an agent's final
message does not alone satisfy these requirements. Track gaps in the existing
contract and continue implementation rather than renaming partial work complete.
