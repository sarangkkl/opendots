# Architecture

## Runtime boundary

React invokes typed Tauri commands. Rust owns persistence, validation, scheduling, queueing, process management, provider protocol handling, MCP forwarding, and approvals. The loopback MCP bridge runs inside the same application process; it is not a sidecar.

`commands` validates UI input. `runtime` owns queue and lifecycle decisions. `providers` translates the three provider protocols. `mcp` exposes narrowly scoped collaboration and integration tools. `db` persists application state; `schedule` handles wall-clock recurrence.

## Storage

Embedded `polodb_core = 5.3.0` stores BSON documents in `opendots.polodb` under Tauri application data (`com.gaurav.opendots`). RocksDB is statically compiled into the Rust binary, not launched as a service. Each resource kind has a collection. Documents preserve stable IDs and include a format version, typed data and indexed lookup fields.

Startup copies an existing SQLite database using `VACUUM INTO`, imports every entity, message, event, execution, legacy session and handoff into a staging store, verifies record round trips and historical execution references, writes an import manifest, and renames the verified directory into place. SQLite and its timestamped snapshot remain untouched. Failed stages remain available for diagnosis; retry starts a fresh stage from the preserved source. An activated store without an import manifest is an error, never an empty fallback. `rusqlite` exists only for legacy import and test fixtures.

Message/dispatch/deduplication records, handoffs, schedule application, scheduled occurrence claims, and human questions/continuations use transactions. Message positions are monotonic counters, independent of timestamps. History returns 50 messages; activity returns 200 events with explicit cursors. PoloDB 5.3.0 has an indexed filter/sort regression: the adapter streams equality-filtered cursors and retains a bounded newest page in Rust. Payloads and retained page memory are bounded, but reading older pages still scans the conversation. Benchmark before promising large-history performance.

Provider sessions use agent, conversation, thread, provider and managed-tool-version keys. Imported sessions remain historical and are never resumed. Credentials stay in the OS credential store. Document defaults and explicit format upgrades replace SQL schema migrations; compatibility changes still require upgrades.

Settings chooses one working root. `workspace` mediates private `agents/<agent-id>/` directories with capability handles and no-follow path traversal. File tools reject absolute paths, traversal, symbolic links and hard-linked reads; writes replace an inode rather than truncate an existing linked file. Browser artifacts use `artifacts/<execution-id>`, uploads use `inputs`, and shared copies use `shared`. Agents cannot mutate those runtime-owned subdirectories through file tools. Browser/profile databases and provider configuration remain application runtime data.

Existing browser output is copied and verified before a transaction switches the working-root and upload/grant references. Original files remain. Workspace relocation after initial selection is deliberately blocked until a dedicated move workflow exists. Missing roots block execution rather than silently selecting a replacement. Provider-native shell/filesystem capabilities are disabled; local stdio integration calls are blocked. Remote MCP services remain separately configured, approval-controlled services, not OS-sandboxed processes.

## Chat routing

`chat::post_chat` validates conversation members and commits a message plus one execution per unique selected agent. Reusing its request ID returns the original dispatch. Plain `@name` strings are inert. Replies inherit their thread and address an agent author by default. Explicit mentions override that default; unaddressed group posts stay informational. Direct chats implicitly address their agent.

The UI has safe Markdown (no raw HTML or automatic remote images), mention chips/autocomplete, a fixed composer, optional thread/activity panels, reply counts and an inbox. `post_message` shares the existing handoff path for agent recipients. Mentioning `you` creates an unread notification; `request_answer` atomically records a waiting request and ends provider work. Replying creates a continuation, closes the waiting request and retains the job/task-chain association. Approval cards remain separate.

## Execution lifecycle

A user message, scheduled occurrence, or structured handoff creates a queued execution. The dispatcher allows one active execution per agent and four across the app. A run streams events and finishes as completed, failed, or cancelled. Approvals temporarily move it to waiting_approval. On restart, formerly running/queued runs become interrupted; human requests remain waiting and pending approvals expire.

Every conversation exposes its executions. Tool activity is what the provider or managed MCP bridge actually reports; hidden reasoning is not presented as observable work.

## Group collaboration

The provider receives the selected agent's instructions, imported skill content, approved memories, and recent messages from the current thread. Group context includes member IDs and roles. Private chats are excluded.

`send_to_agent(agent_id, message, request_id, artifacts?)` creates a separate recipient execution in the same task chain. Reusing request_id within the originating execution returns the existing handoff. Ordinary text mentions do not execute anything. The task-chain handoff limit counts branches as well as depth.

`propose_memory(content)` creates an unapproved lesson. Only user-approved lessons are included in later prompts.

## MCP and approval boundary

Each running execution gets an unpredictable, short-lived bridge URL bound to loopback. Browser Origin requests are rejected. Tokens are removed when a run ends. The bridge exposes collaboration operations plus assigned-integration discovery and invocation.

External tools use a fresh initialized MCP connection for a call. HTTP Streamable MCP (JSON or SSE responses) and installed stdio servers are supported. Sampling and elicitation requests from external servers are rejected. Stateful workflows must put durable state in the server or explicit arguments; connections are not shared between agent executions.

`call_tool(integration_id, tool, arguments)` checks assignment and per-tool allow/ask/deny policy before forwarding. Approval refers to the concrete call and is checked again against revoked access. Unknown tools ask. A rejected call never reaches the server. There is no automatic retry of external operations.

Native provider tools and inherited integrations are restricted to avoid bypassing this gate. Unsupported configurations fail closed. See provider-specific compatibility details before enabling a workflow.

## Scheduling

One-time, daily, and weekly schedules store an IANA timezone and local wall-clock anchor. An ambiguous daylight-saving time selects the earlier occurrence. A nonexistent initial time is rejected; subsequent nonexistent daily occurrences are skipped. Missed occurrences older than 60 seconds, including startup catch-up, are recorded for user review. A long downtime is coalesced into one missed record before advancing to the next future occurrence.

An active job chain prevents overlap. Single-instance handling prevents multiple desktop instances from dispatching simultaneously. Window close hides to the tray; explicit Quit stops the runtime. No jobs execute while the app is quit or the computer is asleep.

## Browser runtime

`browser` owns the Rust CDP connection, supervised Chrome processes, profile leases, observation snapshots, managed files, and capture. Document collections hold `browser_profile`, `browser_upload`, explicit copied `browser_share_copy` references and `browser_replay` requests. These additive entity kinds preserve existing records without rewriting their schema. Executions retain browser action start/finish and screenshot-reference events; large artifacts are under the owning agent’s `artifacts/<execution-id>`.

The execution-scoped tool bridge returns typed text/image content. Claude/Cursor receive MCP image content; Codex receives `inputImage` dynamic-tool items. No screenshot base64 is written to execution events. UI live frames are polled from bounded in-memory capture state and are shown only for the owning execution.

A profile is permanently scoped to an agent. The profile session mutex serializes observations/actions; ownership spans an execution and yields to manual takeover. Approval waits release that mutex. Dispatch validates ownership, cancellation, snapshot token, document identity, URL, DOM revision, viewport and element geometry. Interaction policy keys are `browser:<action>` and default to ask. Every action invalidates its snapshot before dispatch, including failed or uncertain actions.

One CDP reader multiplexes request IDs and events. The screencast consumer acknowledges frames and sends them to a bounded encoding queue. A blocking Rust worker converts RGB to I420, encodes VP8 through statically linked libvpx, and muxes WebM through statically linked libwebm. Completed segments are atomically renamed from `.partial`; the render thread does not encode media. Chrome receives a graceful close before forced cleanup so saved cookies can flush.
