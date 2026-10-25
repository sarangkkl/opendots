# Contributing

Read `AGENTS.md` before changing the project. `CLAUDE.md` imports that file.

Keep all backend behavior in Rust. Do not introduce a Node/Python backend or bundled sidecar. React should render state and invoke backend commands. Installed provider CLIs and explicitly configured MCP servers are allowed external processes.

## Local checks

```sh
npm ci
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
npx playwright install chromium
npm run test:ui
```

Rust fixture tests bind localhost. UI tests use mocked Tauri IPC to check browser interactions; they do not prove native IPC or provider behavior. Provider smoke tests are opt-in and require a separately installed, authenticated CLI. Never use live publishing accounts in automated tests.

Add regression tests at the boundary that owns behavior: queueing and approvals in Rust, provider event translation at the protocol boundary, and user workflows in Playwright. Do not place credentials, provider session files, real conversation exports, or generated bundles in commits.

When changing schemas, add a migration and preserve existing data. When changing provider flags, validate against the installed CLI and update the compatibility guide. Report actual platform validation; do not equate a CI configuration with a passing build.

Contributions are made under AGPL-3.0-only. The public repository and issue tracker will be linked when published.
