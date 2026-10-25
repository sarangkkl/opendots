# Validation status

Validation performed on macOS Apple Silicon. This file distinguishes tests that actually ran from the configured cross-platform CI matrix.

| Check                                                   | Result                                                         |
| ------------------------------------------------------- | -------------------------------------------------------------- |
| Frontend TypeScript and Vite production build           | Passed                                                         |
| Rust compilation and strict Clippy                      | Passed                                                         |
| Deterministic Rust suite                                | 24 passed; native browser and live provider tests opt-in               |
| Playwright UI suite with mocked Tauri IPC               | 7 passed                                                       |
| Claude Code real streaming/tool/resume test             | Passed, 2.1.86                                                 |
| Codex real streaming/tool/resume test                   | Passed, 0.164.0-alpha.11.1                                     |
| Cursor real execution                                   | Blocked: installed CLI requires login                          |
| macOS Apple Silicon release `.app` bundle               | Built successfully                                             |
| Native UI walkthrough                                   | Not verified: computer-use access to OpenDots was not approved |
| macOS Intel, Windows x64, Linux x64 packages/installers | CI configured; not run locally                                 |
| Live studio generation or Instagram publishing          | Outside this release; fixtures only                            |

The macOS source-build script passed both `--check` and `--build-only`. The installation/copy step was not run, so no existing installation was replaced.

## What the deterministic tests prove

- PoloDB persistence, verified SQLite import/retry, cursor pagination with equal timestamps, recovery, and thread-scoped provider sessions.
- Research → creation → clarification → revised creation → publisher handoffs, visible conversation messages, duplicate prevention, and chain limits.
- Proposed memories remain inactive until approved and private conversation content is not shared into unrelated contexts.
- A real loopback fixture MCP server receives no call after rejection or deny; approving once forwards exactly one call.
- Cancelling a waiting handoff expires its approval without dispatching a recipient. Agent calls to local stdio integrations are rejected before launching the integration.
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
5. Decide release signing/notarization before distributing prebuilt binaries.

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

## Threaded chat and document-storage update (2026-10-25)

- Deterministic coverage now includes structured mention deduplication, informational group posts, addressed replies/thread isolation, human request persistence and continuation, rejected/approved job proposals, private path traversal/symlink/hard-link rejection, and readback from private work folders.
- Legacy SQLite fixtures verify preservation, session invalidation, retained snapshots, and retry after an invalid source record without activating an empty store. No destructive production-database conversion was run as a test.
- Seven mocked-IPC UI workflows pass, including safe Markdown (no images/scripts), explicit mention selection and threaded replies. Screenshots were reviewed for the channel/thread layout.
- All three native Chrome fixtures pass with browser media under private folders: profiles/cookies, uploads/downloads, approvals, recordings/replay and exited-browser recovery.
- Real Claude Code and Codex each wrote/read `proof.txt` through the managed `files` tool and posted a persisted `waiting_user` question. These runs used temporary workspaces and existing CLI authentication. Cursor remains unverified because its login was not available in the earlier provider check.
- Native macOS Apple Silicon packaging succeeds with embedded PoloDB/RocksDB. Windows, macOS Intel and Ubuntu remain CI-configured but not executed on this Mac. Do not describe those installers as locally verified.

Limitations: document history cursors bound returned data and retained page memory, but currently scan matching records because PoloDB 5.3.0 indexed compound/sort queries failed regression tests. Full-scale performance is not benchmarked. Private files enforce managed operations; this is not an OS sandbox for every third-party process or remote MCP service. Existing workspace-root relocation is blocked; select the intended root initially. External stdio integrations remain visible in configuration but cannot execute for managed agents. Native drag/dialog edge cases and malicious concurrent host-process interference with Chrome-owned paths require additional platform validation.
