# OpenDots

A desktop workspace for specialized AI agents that learn your process and work together in visible group conversations.

Create agents backed by your installed Claude Code, Codex, or Cursor CLI. Give them instructions, Markdown skills, and MCP integrations. Review their proposed memories, follow individual executions, and control when they hand work to each other or use tools.

The application backend is Rust inside Tauri 2. React and TypeScript render the UI. There is no bundled sidecar or Node/Python backend. Installed provider CLIs and remote MCP services remain separately configured capabilities. Agent calls to arbitrary local stdio MCP commands are blocked under managed file confinement.

## Start developing

Install [platform prerequisites](docs/BUILDING.md), then:

```sh
npm ci
npm run tauri dev
```

`npm run dev` is a browser-only UI preview; it cannot run agents or persist application data.

## Build and install from source

Clone this repository and enter its directory. The public repository URL will be added when the project is published.

| Device                          | Command                                                                    |
| ------------------------------- | -------------------------------------------------------------------------- |
| macOS Apple Silicon or Intel    | `bash scripts/install-from-source.sh`                                      |
| Linux x64 (Ubuntu/Debian first) | `bash scripts/install-from-source.sh`                                      |
| Windows x64, PowerShell         | `powershell -ExecutionPolicy Bypass -File scripts/install-from-source.ps1` |

See [BUILDING](docs/BUILDING.md) for prerequisite installation, build-only commands, package locations, and verification status. Node is a build dependency; OpenDots' packaged backend does not require Node. Individual provider CLIs may have their own requirements.

## First workflow

1. Create your agents, or choose **Start with Sira Production** for Researcher, Creator, and Publisher role templates.
2. Sign in to your chosen provider in a terminal and check its installation in Settings.
3. Connect MCP servers in Settings, then assign them and their tool policies to agents.
4. Teach each agent its job. Approve or edit proposed memories before they affect future work.
5. Create a group, select its members, and send a task to the starting agent.
6. Watch group handoffs and open any execution to inspect its output, tool activity, and errors.

Sira is a configuration example, not a live Instagram integration. No studio or publishing workflow is hard-coded.

## Controls

- Group handoffs can run automatically or require review. Plain text mentions do not dispatch work.
- Unknown external tools ask for approval. Each agent can allow, ask, or deny a tool.
- Pausing a group stops new dispatch and handoffs; use Cancel to stop an active execution.
- A default 20-handoff limit stops runaway task chains. Increase the limit and resume after review.
- One execution runs per agent, with at most four provider executions across the app.
- Closing the window leaves OpenDots in the tray. Quit stops scheduling and owned processes.
- Missed scheduled work is recorded rather than replayed. External actions are never retried automatically.

Cancelling a request cannot undo an action an external server already performed. Inspect results before retrying publishing or other external actions.

## Documentation and tests

- [Architecture and data flow](docs/ARCHITECTURE.md)
- [Provider setup and compatibility](docs/PROVIDERS.md)
- [Build and installation](docs/BUILDING.md)
- [Validation status](docs/VALIDATION.md)
- [Contributing](CONTRIBUTING.md)

```sh
npm run build
npm run test:rust
npx playwright install chromium
npm run test:ui
```

The Rust tests use local fixture MCP servers. Live provider tests are opt-in and use your existing account; see the validation guide.

## License

[GNU Affero General Public License v3.0 only](LICENSE), SPDX `AGPL-3.0-only`. External providers and dependencies retain their own licenses and terms.

Built-in browser setup, saved logins, approvals, capture and replay: [Browser guide](docs/BROWSER.md).

Chats support structured mentions, thread replies, safe Markdown and a Mentions & requests inbox. Agents can propose approved schedules and work in private folders selected through Settings. State is stored in embedded PoloDB; existing SQLite installations import through a verified, non-destructive snapshot transition. See [architecture](docs/ARCHITECTURE.md) and [validation](docs/VALIDATION.md) for boundaries and tested combinations.
