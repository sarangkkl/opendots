# Validation status

Validation performed on macOS Apple Silicon. This file distinguishes tests that actually ran from the configured cross-platform CI matrix.

| Check                                                   | Result                                                         |
| ------------------------------------------------------- | -------------------------------------------------------------- |
| Frontend TypeScript and Vite production build           | Passed                                                         |
| Rust compilation and strict Clippy                      | Passed                                                         |
| Deterministic Rust suite                                | 16 passed; native browser and live provider tests opt-in               |
| Playwright UI suite with mocked Tauri IPC               | 6 passed                                                       |
| Claude Code real streaming/tool/resume test             | Passed, 2.1.86                                                 |
| Codex real streaming/tool/resume test                   | Passed, 0.164.0-alpha.11.1                                     |
| Cursor real execution                                   | Blocked: installed CLI requires login                          |
| macOS Apple Silicon release `.app` bundle               | Built successfully                                             |
| Native UI walkthrough                                   | Not verified: computer-use access to OpenDots was not approved |
| macOS Intel, Windows x64, Linux x64 packages/installers | CI configured; not run locally                                 |
| Live studio generation or Instagram publishing          | Outside this release; fixtures only                            |

The macOS source-build script passed both `--check` and `--build-only`. The installation/copy step was not run, so no existing installation was replaced.

## What the deterministic tests prove

- SQLite persistence, recovery, and agent/conversation/provider session separation.
- Research → creation → clarification → revised creation → publisher handoffs, visible conversation messages, duplicate prevention, and chain limits.
- Proposed memories remain inactive until approved and private conversation content is not shared into unrelated contexts.
- A real loopback fixture MCP server receives no call after rejection or deny; approving once forwards exactly one call.
- Cancelling a waiting handoff expires its approval without dispatching a recipient. Cancelling an in-flight stdio MCP call terminates the pending request promptly.
- Queue limits enforce one active run per agent and four globally; cancellation releases owned processes.
- Malformed provider output and early process exit become errors.
- Schedules handle startup misses, overlap, wall-clock recurrence, and daylight-saving gaps/ambiguities.
- The MCP bridge rejects invalid execution tokens and browser-origin calls.

UI tests exercise agent configuration, memory editing, persisted mock state, sending/cancelling work, group autonomy controls, visible handoffs, and approval rejection. They do not replace native IPC or platform tests.

## Remaining validation before announcing broad support

1. Authenticate Cursor and run its opt-in live test. Fix any version-specific ACP/MCP differences before claiming verified Cursor support.
2. Run the committed CI workflow on all four OS/architecture runners.
3. Exercise source installation and tray lifecycle manually on each supported OS, including Linux desktop/Secret Service variations and Windows process cleanup.
4. Complete a native UI walkthrough with computer-use permission or manual review.
5. Add the public repository URL and decide release signing/notarization before distributing prebuilt binaries.

Build scripts and CI are provided now; unrun platforms are not represented as passing. No real social account was connected or published to during validation.


## Built-in browser delivery

Verified on this Apple Silicon Mac with dedicated temporary Chrome profiles:

- Two native Chrome fixture tests passed: saved-cookie persistence after browser restart; isolated agent profiles; approval rejection; mutation/stale-snapshot rejection; screenshots; open shadow DOM and same-origin frame discovery; replay with two fresh approvals; upload/download; coordinate click; scrolling; tab switching; manual takeover and cancellation while waiting.
- Typed MCP image and Codex image response serialization passed. A real Claude Code run and a real Codex app-server run both identified a green triangle visible only in the screenshot fixture.
- Cursor's same vision test was attempted and failed at authentication (`agent login` required), before browser-tool use. Cursor screenshot compatibility remains unverified.
- Finalized VP8/WebM output was inspected and decoded with external validation tools. Those tools are not invoked or required by OpenDots. `otool -L` confirms the native application does not dynamically depend on Homebrew libvpx/libwebm.
- Frontend build, strict Clippy, formatting, and all six mocked-IPC UI workflows passed. Browser profile/policy persistence is included in UI coverage.
- The macOS source script passed `--check` and `--build-only`; it produced the native `.app`. No existing application installation was replaced.

The native fixtures do not prove behavior on arbitrary third-party sites. Cross-origin frame interaction and closed shadow DOM remain explicitly limited. Drag gestures, keyboard actions and JavaScript-dialog handling have implementations but lack dedicated real-site coverage. Abrupt crash recovery retains finalized video segments by construction; unfinished `.partial` files are exposed as incomplete, not claimed to be playable. Cross-platform codec builds and source installation still require the CI/manual matrix above.

Appearance/recovery update: Light/Dark/System switching and persisted preferences passed the UI workflow test. An isolated native Chrome regression passed automatic launch, forced process exit, relaunch using the same profile, stale-action rejection, and pending-approval expiration. Native title-bar appearance is wired through Tauri; a native visual walkthrough remains unverified.
