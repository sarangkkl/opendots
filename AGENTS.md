# OpenDots

## Product direction

We are building a desktop app where users can create specialized AI agents and chat with them. Each agent can come with selected plugins and skills already loaded, remember useful information across conversations, and schedule and run jobs for the user.

The core experience is:

1. Create an agent with a name, purpose, and instructions.
2. Choose the plugins and skills available to that agent.
3. Chat with the agent using its configured capabilities.
4. Let the agent retain useful context across conversations.
5. Give the agent a job to run now or on a schedule.
6. Review job status, results, and failures.
7. Create groups of agents and inspect what each agent is working on independently.

## Provider, group, and execution requirements

- Support installed Claude Code, Codex app-server, and Cursor ACP as agent execution providers. All OpenDots backend logic stays in Rust; no bundled sidecars or Node/Python backend.
- Groups share a conversation. Structured agent handoffs create separate executions; ordinary mentions do not trigger work. Users can review all messages and control autonomy.
- Each agent has its own execution calls and execution history, including when it belongs to a group.
- Users can open an agent to see its current task, execution status, available tool activity and output, and completed or failed runs. Show the activity exposed by the provider; do not imply access to hidden model reasoning.
- Keep executions attributable to their agent and, when applicable, the group and job that triggered them.

## Core concepts

- **Agent:** A persistent configuration with its own purpose, instructions, selected capabilities, conversations, and memory.
- **Skill:** Reusable instructions or workflows that guide how an agent performs a specialized task.
- **Plugin:** An integration that gives an agent access to tools or external services.
- **Memory:** Persisted information the agent can use in later conversations and jobs. Users should be able to inspect, edit, and delete it.
- **Job:** A task assigned to an agent, with execution status and a recorded result.
- **Schedule:** A one-time or recurring trigger for a job, with an explicit timezone and controls to pause or cancel it.

The desktop platform uses SQLite in the application-data directory, Rust-owned provider processes and an in-process MCP bridge. Credentials belong in the OS credential store. Memory proposals require user approval. Provider-native plugins are not automatically portable between providers.

Schedules run while OpenDots is in the tray. Explicit Quit stops execution. Missed schedules are recorded, not replayed. No cloud/background daemon is included.

See docs/VALIDATION.md for verified behavior and remaining platform/provider limitations. The project license is AGPL-3.0-only.

## Current stack

- Tauri 2 desktop application with a Rust backend.
- React and TypeScript frontend, built with Vite.
- npm for frontend dependencies and scripts.

## Development guidance

- Use this file as the shared source of project instructions. Keep `CLAUDE.md` as a reference to it rather than duplicating these instructions.
- Build small, working slices of the core experience. Avoid speculative infrastructure or unrelated features.
- Inspect existing code and reuse established patterns before adding new abstractions or dependencies.
- Keep each agent's configuration, memory, and enabled capabilities clearly scoped to that agent. Make sharing explicit.
- Keep credentials out of source control, chat history, and ordinary memory records.
- Make job progress and failures visible. Distinguish a scheduled job from a job that actually ran successfully.
- Validate changes with relevant checks and state any limitations honestly.
- Update these instructions when the user establishes new product or architecture decisions.

## Commands

- `npm install` — install frontend dependencies.
- `npm run tauri dev` — run the desktop app in development.
- `npm run dev` — run the frontend development server.
- `npm run build` — type-check and build the frontend.
- `npm run tauri build` — build the desktop application.

<!-- postman-cli:installation:start -->
Postman CLI is installed on this device, and can be used for API Engineering work.
<!-- postman-cli:installation:end -->

## Validation

- `npm run test:rust` — deterministic Rust tests, including local fixture MCP servers.
- `npm run test:ui` — Playwright UI workflows with mocked Tauri IPC.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — Rust lint checks.
- Live provider tests are opt-in; they use installed CLIs and existing accounts. Never substitute live publishing for a fixture test.
- Use the scripts in `scripts/` for source builds; forward `--locked` to Cargo after Tauri's `--` separator.

## Built-in browser capabilities

- Chrome/Chromium is separately installed; Rust owns CDP, profiles, approvals, screenshots, downloads, replay and recording. No Python/Node/FFmpeg service or bundled browser sidecar.
- Each agent has one dedicated saved profile under application data. Never attach to or copy an everyday browser profile; never reassign saved logins to another agent.
- Browser interactions default to `ask`, with per-agent `browser:<action>` allow/ask/deny policies. Observation is automatic. Approval must bind the exact action to a fresh snapshot and be revalidated before dispatch.
- Manual login/takeover suspends capture and automation. Credentials/cookies must not enter tool results or ordinary memory. Group sharing uses explicit artifact references, never shared browser sessions.
- Native video encoding uses statically linked libvpx/libwebm from Rust. Source builders need libvpx development files and libclang; React only presents frames and artifacts.
- Validate browser behavior against local fixtures, not real social-media publishing. Replay is explicit, respects current policies, stops on ambiguous targets, and never retries uncertain submissions.

- Appearance supports persisted Light/Dark/System preferences. Use shared CSS color tokens for all surfaces and synchronize the native window theme.
- Agents automatically launch/relaunch stopped or exited configured browser profiles. Preserve saved login data and live manual ownership; invalidate old snapshots/approvals and never replay actions as part of recovery.
