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

- Support using Claude Code, Codex, and Cursor as agent execution providers. Study the reference implementation before choosing integration details; its repository location is still needed.
- Users can create groups and add agents to them. Group orchestration and shared conversation behavior remain to be defined.
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

These describe the intended product, not features that are already implemented. Model providers, storage, plugin loading, and job execution architecture remain to be decided as we implement them. Do not assume scheduled jobs can run while the desktop app is closed without an execution mechanism that supports it.

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
