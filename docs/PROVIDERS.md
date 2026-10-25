# Provider setup and compatibility

OpenDots launches separately installed CLIs. It does not bundle providers, copy their credentials, or implement its own provider login. Existing subscriptions and provider terms apply.

## Claude Code

Install from [Claude Code's official setup guide](https://code.claude.com/docs/en/setup), then run:

```sh
claude auth login
claude --version
```

OpenDots uses print mode with JSON streaming, a restricted MCP configuration, disabled native tools and hooks, and explicit session IDs for resumption. Agent instructions and imported Markdown skills are supplied by OpenDots; provider-native plugins and slash-command skills are not automatically loaded.

Validated locally with Claude Code 2.1.86: streaming response, managed memory proposal, and session resumption.

## Codex

Install using [the official Codex CLI instructions](https://developers.openai.com/codex/cli/), then:

```sh
codex login
codex --version
```

OpenDots communicates with `codex app-server` over stdio. The installed protocol must support experimental dynamic tools, which route collaboration and managed integration calls directly into the Rust bridge implementation. OpenDots disables inherited MCP/plugin entries for its child process without editing the user's global configuration. Shell, JavaScript execution, multi-agent delegation, and web search are disabled; native approval requests are denied. The provider runs read-only.

Configuration keys containing dots cannot currently be safely overridden by this adapter and cause a visible configuration error. A future protocol change may require an adapter update; OpenDots does not silently downgrade to unrestricted execution.

Validated locally with codex-cli 0.164.0-alpha.11.1: streaming response, managed memory proposal, and session resumption. Older/newer protocol compatibility is not assumed.

## Cursor

Install from [Cursor's CLI instructions](https://cursor.com/docs/cli/installation), then:

```sh
agent login
agent --version
```

OpenDots uses `agent acp`, creates/resumes ACP sessions, and consumes message/tool updates. A managed workspace contains the OpenDots MCP connection and native shell/read/write denial settings. Non-bridge permission requests and unsupported client methods are rejected.

Global Cursor MCP servers are rejected because they could bypass per-agent assignment. Configure these capabilities in OpenDots instead. Cursor's native plugin/hook configuration is not a supported capability source for managed agents.

The locally installed Cursor version is `2026.05.05-84a231c`. Initialization was exercised, but a real chat could not be validated because the CLI returned **Authentication required**. Run `agent login`, then the opt-in test below. Treat Cursor execution and MCP/resume compatibility as unverified until that test passes.

## Discovery and paths

Settings provides provider version checks. Agents can override the executable path when desktop PATH discovery cannot find a CLI. OpenDots searches PATH plus common local installation directories. The account login must be usable from the desktop process; shell-only environment variables may not be inherited when launching from Finder or the Start menu.

## Integration policies

Connect an MCP URL or command in Settings, then assign it to an agent. Discover tool names to configure allow/ask/deny policies. Unknown tool names ask for approval. Optional HTTP bearer tokens are stored in the OS credential store. Stdio servers should manage their own authentication; do not put credentials in command arguments.

Provider processes are trusted third-party software, not an operating-system security boundary. OpenDots enforces managed MCP call policy and restricts native tools, but cannot undo external actions or guarantee that a changed upstream CLI preserves undocumented behavior. Keep provider versions under review.

## Opt-in provider test

These commands make small model requests using your existing account. They propose a test memory in an isolated temporary database and resume the same session; they do not publish or invoke external integrations.

```sh
OPENDOTS_TEST_PROVIDER=claude cargo test --manifest-path src-tauri/Cargo.toml --locked live_provider_smoke -- --ignored --nocapture
OPENDOTS_TEST_PROVIDER=codex cargo test --manifest-path src-tauri/Cargo.toml --locked live_provider_smoke -- --ignored --nocapture
OPENDOTS_TEST_PROVIDER=cursor cargo test --manifest-path src-tauri/Cargo.toml --locked live_provider_smoke -- --ignored --nocapture
```

PowerShell example:

```powershell
$env:OPENDOTS_TEST_PROVIDER = 'cursor'
cargo test --manifest-path src-tauri/Cargo.toml --locked live_provider_smoke -- --ignored --nocapture
```

Set `OPENDOTS_TEST_EXECUTABLE` to an explicit provider path if needed. Test sessions may remain in the provider's own history.

## Built-in browser tools

Browser tools are supplied by OpenDots' Rust bridge, not a provider-native browser plugin. Assign a saved profile in Settings. `browser_observe` returns native image content as well as text. Codex's installed app-server schema exposes `inputImage`/`imageUrl`; Claude and Cursor use MCP `image` content. A configured provider/model still needs image understanding for screenshot-based reasoning. DOM observations remain text. Refer to [browser setup](BROWSER.md) and [verification status](VALIDATION.md).

## Managed workspace and collaboration tools

Choose a working root in Settings before running a provider. Each agent uses its private folder through the Rust `files` tool (`list`, `read`, `write`, `mkdir`, `move`, `delete`, `share`). Modifying actions default to approval. The old free-form workspace reference no longer grants access. Local stdio integrations can still be inspected in Settings, but agent execution rejects them because they could bypass file confinement. Existing remote MCP calls retain their approval policies; those services are not an OS filesystem sandbox.

`post_message` takes structured member IDs and a stable request ID; `you` addresses the user. `request_answer: true` ends the current provider execution as waiting-for-you. A reply starts a continuation in the same thread. `send_to_agent` remains compatible and inherits the thread. File sharing requires approval and copies browser artifacts before recipient dispatch.

`jobs` lists the agent's jobs and proposes create/edit/pause/delete. Provide a stable request ID; changes require explicit approval. Use IANA timezones and once/daily/weekly recurrence, or `immediate`. The app must remain running in its tray. Unavailable work folders produce blocked occurrences.

Sessions created before managed-tool version 2 are not resumed. Configure provider-native plugins only outside these managed sessions. Installed provider login files and runtime configuration remain outside the selected task-file root.
