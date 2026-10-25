import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  active,
  empty,
  type Agent,
  type Group,
  type Integration,
  type Job,
  type Memory,
  type Message,
  type RunEvent,
  type Snapshot,
} from "./types";
import "./App.css";
import { readAppearance, saveAppearance, type Appearance } from "./theme";
import { BrowserSettings, BrowserInspector, BrowserPolicies } from "./Browser";

type Page = "Groups" | "Agents" | "Jobs" | "Settings";
type Editor =
  | { kind: "agent"; value: Agent }
  | { kind: "group"; value: Group }
  | { kind: "integration"; value: Integration }
  | { kind: "job"; value: Job }
  | { kind: "memory"; value: Memory };
const uid = () => crypto.randomUUID();
const date = (s: string) =>
  new Date(s).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
const newAgent = (): Agent => ({
  id: uid(),
  name: "",
  role: "",
  instructions: "",
  provider: "claude",
  model: "",
  executable: "",
  workspace: "",
  skills: [],
  integrations: [],
  policies: {},
});
const newGroup = (): Group => ({
  id: uid(),
  name: "",
  purpose: "",
  members: [],
  automatic: true,
  paused: false,
  handoff_limit: 20,
});
const newJob = (): Job => ({
  id: uid(),
  name: "",
  agent_id: "",
  group_id: null,
  prompt: "",
  timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  local_time: "",
  recurrence: "once",
  next_at: null,
  paused: false,
});
function Field({
  label,
  children,
  hint,
}: {
  label: string;
  children: ReactNode;
  hint?: string;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {hint && <small>{hint}</small>}
    </label>
  );
}
function Badge({ status }: { status: string }) {
  return <span className={`badge ${status}`}>{status.replace(/_/g, " ")}</span>;
}

export default function App() {
  const [appearance, setAppearance] = useState<Appearance>(readAppearance);
  const [data, setData] = useState<Snapshot>(empty);
  const [page, setPage] = useState<Page>("Groups");
  const [selected, setSelected] = useState("");
  const [editor, setEditor] = useState<Editor | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [recipient, setRecipient] = useState("");
  const [inspect, setInspect] = useState("");
  const [events, setEvents] = useState<RunEvent[]>([]);
  const [checks, setChecks] = useState<Record<string, unknown>>({});
  const [toolLists, setToolLists] = useState<
    Record<string, { name: string; description?: string }[]>
  >({});
  const [credential, setCredential] = useState<{
    id: string;
    value: string;
  } | null>(null);
  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setData(await invoke<Snapshot>("snapshot"));
    } catch (e) {
      setError(String(e));
    }
  }, []);
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;
  useEffect(() => {
    void refresh();
    if (!isTauri()) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const off = listen("changed", () => {
      if (!timer)
        timer = setTimeout(() => {
          timer = undefined;
          void refreshRef.current();
        }, 250);
    });
    return () => {
      void off.then((f) => f());
      clearTimeout(timer);
    };
  }, [refresh]);
  const group =
    page === "Groups" ? data.groups.find((g) => g.id === selected) : undefined;
  const agent =
    page === "Agents" ? data.agents.find((a) => a.id === selected) : undefined;
  const conversation = group
    ? `group:${group.id}`
    : agent
      ? `agent:${agent.id}`
      : "";
  useEffect(() => {
    let valid = true;
    if (conversation && isTauri())
      void invoke<Message[]>("messages", { conversationId: conversation })
        .then((v) => {
          if (valid) setMessages(v);
        })
        .catch((e) => setError(String(e)));
    else setMessages([]);
    return () => {
      valid = false;
    };
  }, [conversation, data]);
  useEffect(() => {
    let valid = true;
    if (inspect && isTauri())
      void invoke<RunEvent[]>("execution_events", { executionId: inspect })
        .then((v) => {
          if (valid) setEvents(v);
        })
        .catch((e) => setError(String(e)));
    else setEvents([]);
    return () => {
      valid = false;
    };
  }, [inspect, data]);
  useEffect(() => {
    setRecipient(group?.members[0] || agent?.id || "");
    setDraft("");
  }, [selected, page, group?.members.join(",")]);
  const act = async <T,>(
    command: string,
    args: Record<string, unknown> = {},
  ): Promise<T | undefined> => {
    setBusy(true);
    setError("");
    try {
      const r = await invoke<T>(command, args);
      await refresh();
      return r;
    } catch (e) {
      setError(String(e));
      return undefined;
    } finally {
      setBusy(false);
    }
  };
  const save = async () => {
    if (!editor) return;
    const result = await act<boolean>("save_entity", {
      kind: editor.kind,
      value: editor.value,
    }).then((v) => v !== undefined);
    if (result) setEditor(null);
  };
  // Void Tauri commands resolve null, so undefined exclusively means a failed request.
  const saveDirect = async (kind: string, value: unknown) =>
    act("save_entity", { kind, value });
  const show = (p: Page, id = "") => {
    setPage(p);
    setSelected(id);
    setInspect("");
    setError("");
  };
  const name = (id: string | null) =>
    data.agents.find((a) => a.id === id)?.name ||
    (id ? "Deleted agent" : "You");
  const relevantRuns = data.executions.filter(
    (r) => r.conversation_id === conversation,
  );
  const currentRun = data.executions.find((r) => r.id === inspect);
  const selectedApprovals = data.approvals.filter(
    (a) =>
      a.status === "pending" &&
      (!conversation ||
        data.executions.some(
          (r) => r.id === a.execution_id && r.conversation_id === conversation,
        )),
  );
  const update = (values: Record<string, unknown>) =>
    setEditor((e) =>
      e ? ({ ...e, value: { ...e.value, ...values } } as Editor) : null,
    );
  async function testIntegration(id: string) {
    const result = await act<{
      tools: { name: string; description?: string }[];
    }>("check_integration", { integrationId: id });
    if (result) {
      setToolLists((s) => ({ ...s, [id]: result.tools || [] }));
      setNotice(`${result.tools?.length || 0} tools available`);
    }
  }
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <button className="brand" onClick={() => show("Groups")}>
          <span className="brand-symbol">⠿</span> OpenDots{" "}
          <span className="version">0.1</span>
        </button>
        <p className="eyebrow">YOUR WORKSPACE</p>
        <nav>
          {(["Groups", "Agents", "Jobs", "Settings"] as Page[]).map((p, i) => (
            <button
              key={p}
              className={page === p ? "nav-item selected" : "nav-item"}
              onClick={() => show(p)}
            >
              <span className="nav-icon">{["◈", "◉", "◷", "⚙"][i]}</span>
              {p}
              {p === "Agents" && (
                <span className="count">{data.agents.length}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-groups">
          <p className="eyebrow">GROUPS</p>
          {data.groups.map((g) => (
            <button
              key={g.id}
              className={`group-link ${selected === g.id ? "chosen" : ""}`}
              onClick={() => show("Groups", g.id)}
            >
              <span className="small-dot" />
              {g.name}
              {g.paused && <span title="Paused">Ⅱ</span>}
            </button>
          ))}
        </div>
        <div className="sidebar-footer">
          <span className="status-dot" /> Runs on your device
          <small>Rust backend · no sidecar</small>
        </div>
      </aside>
      <main className="main">
        <header className="topbar">
          <div className="breadcrumbs">
            Workspace <span>/</span> {page}
            {(group || agent) && (
              <>
                <span>/</span>
                {group?.name || agent?.name}
              </>
            )}
          </div>
          <div className="top-status">
            <span className="status-dot" />
            {data.executions.filter((r) => active(r.status)).length} active
            executions
          </div>
        </header>
        {!isTauri() && (
          <div className="banner warning">
            Browser preview only. Launch the desktop app with{" "}
            <code>npm run tauri dev</code> to use the Rust backend.
          </div>
        )}
        {error && (
          <div className="banner error" role="alert">
            {error}
            <button onClick={() => setError("")} aria-label="Dismiss error">
              ×
            </button>
          </div>
        )}
        {notice && (
          <div className="banner" role="status">
            {notice}
            <button onClick={() => setNotice("")} aria-label="Dismiss notice">
              ×
            </button>
          </div>
        )}
        <div className="page-body">
          {!group && !agent && (
            <div className="page-heading">
              <div>
                <p className="eyebrow">
                  {page === "Groups" ? "BETTER TOGETHER" : "OPENDOTS"}
                </p>
                <h1>
                  {page === "Groups"
                    ? "Your agent teams"
                    : page === "Agents"
                      ? "Meet your agents"
                      : page === "Jobs"
                        ? "Work on a schedule"
                        : "Workspace settings"}
                </h1>
                <p className="muted">
                  {page === "Groups"
                    ? "Give each agent a role. Let the conversation move the work forward."
                    : page === "Agents"
                      ? "Teach them your process, connect their tools, and follow their work."
                      : page === "Jobs"
                        ? "Assign a task now or schedule it for later."
                        : "Connect your installed providers and the tools your agents use."}
                </p>
              </div>
              {page !== "Settings" && (
                <button
                  className="primary"
                  onClick={() =>
                    setEditor(
                      page === "Groups"
                        ? { kind: "group", value: newGroup() }
                        : page === "Agents"
                          ? { kind: "agent", value: newAgent() }
                          : { kind: "job", value: newJob() },
                    )
                  }
                >
                  + New{" "}
                  {page === "Groups"
                    ? "group"
                    : page === "Agents"
                      ? "agent"
                      : "job"}
                </button>
              )}
            </div>
          )}
          {page === "Groups" && !group && (
            <>
              <div className="card-grid">
                {data.groups.map((g) => (
                  <button
                    className="team-card"
                    key={g.id}
                    onClick={() => show("Groups", g.id)}
                  >
                    <div className="card-top">
                      <span className="team-mark">◈</span>
                      <Badge status={g.paused ? "paused" : "ready"} />
                    </div>
                    <h2>{g.name}</h2>
                    <p>
                      {g.purpose || "A space for your agents to work together."}
                    </p>
                    <div className="card-bottom">
                      <span className="avatars">
                        {g.members.slice(0, 4).map((id) => (
                          <span key={id} title={name(id)}>
                            {name(id).slice(0, 1)}
                          </span>
                        ))}
                      </span>
                      <span>
                        {g.members.length} agents{" "}
                        <span className="arrow">↗</span>
                      </span>
                    </div>
                  </button>
                ))}
              </div>
              {!data.groups.length && (
                <div className="empty-state">
                  <div className="empty-symbol">◈</div>
                  <h2>A team starts with a purpose</h2>
                  <p>
                    Create a group and give your agents complementary roles.
                    <br />
                    You’ll see their handoffs, questions, and results in one
                    conversation.
                  </p>
                  <button
                    className="primary"
                    disabled={busy || !isTauri()}
                    onClick={async () => {
                      const id = await act<string>("create_example");
                      if (id) show("Groups", id);
                    }}
                  >
                    Start with Sira Production
                  </button>
                  <small>
                    Creates three unconfigured agents. No live publishing.
                  </small>
                </div>
              )}
            </>
          )}
          {page === "Agents" && !agent && (
            <div className="card-grid">
              {data.agents.map((a) => (
                <button
                  className="team-card"
                  key={a.id}
                  onClick={() => show("Agents", a.id)}
                >
                  <div className="card-top">
                    <span className="avatar large">{a.name[0]}</span>
                    <Badge
                      status={
                        data.executions.find(
                          (r) => r.agent_id === a.id && active(r.status),
                        )?.status || "ready"
                      }
                    />
                  </div>
                  <h2>{a.name}</h2>
                  <p>{a.role || "Ready to learn a role"}</p>
                  <div className="card-bottom">
                    <span>{a.provider}</span>
                    <span>{a.integrations.length} integrations ↗</span>
                  </div>
                </button>
              ))}
              {!data.agents.length && (
                <div className="empty-state">
                  <h2>Create your first agent</h2>
                  <p>
                    Choose a provider, describe its role, then teach it in chat.
                  </p>
                </div>
              )}
            </div>
          )}
          {(group || agent) && (
            <>
              <div className="conversation-heading">
                <div>
                  <h1>{group?.name || agent?.name}</h1>
                  <p className="muted">{group?.purpose || agent?.role}</p>
                </div>
                <div className="actions">
                  {group && (
                    <button
                      onClick={() =>
                        saveDirect("group", { ...group, paused: !group.paused })
                      }
                    >
                      {group.paused ? "Resume group" : "Pause group"}
                    </button>
                  )}
                  <button
                    onClick={() =>
                      setEditor(
                        group
                          ? { kind: "group", value: { ...group } }
                          : { kind: "agent", value: { ...agent! } },
                      )
                    }
                  >
                    Configure
                  </button>
                </div>
              </div>
              {group && (
                <div className="member-strip">
                  {group.members.map((id) => (
                    <button key={id} onClick={() => show("Agents", id)}>
                      <span className="avatar">{name(id)[0]}</span>
                      {name(id)}
                      <Badge
                        status={
                          data.executions.find(
                            (r) => r.agent_id === id && active(r.status),
                          )?.status || "ready"
                        }
                      />
                    </button>
                  ))}
                  <span className="muted">
                    {group.automatic
                      ? "Automatic handoffs"
                      : "Review each handoff"}
                    {group.paused ? " · Paused" : ""}
                  </span>
                </div>
              )}
              <div className="workspace-columns">
                <section className="chat-panel">
                  <div className="panel-heading">
                    <h3>{group ? "Group conversation" : "Conversation"}</h3>
                    <span className="muted">You can join at any time</span>
                  </div>
                  <div className="messages">
                    {!messages.length && (
                      <div className="chat-empty">
                        <span>✧</span>
                        <h2>What are we working on?</h2>
                        <p>
                          {group
                            ? "Give a task to a team member to start the conversation."
                            : "Describe the agent’s work, share instructions, or ask a question."}
                        </p>
                      </div>
                    )}
                    {messages.map((m) => (
                      <article className={`message ${m.role}`} key={m.id}>
                        <div className="message-meta">
                          <span className="avatar">{name(m.agent_id)[0]}</span>
                          <strong>{name(m.agent_id)}</strong>
                          {m.role === "handoff" && <Badge status="handoff" />}
                          <time>{date(m.created_at)}</time>
                          {m.execution_id && (
                            <button
                              className="text-button"
                              onClick={() => setInspect(m.execution_id!)}
                            >
                              View execution ↗
                            </button>
                          )}
                        </div>
                        <div className="message-content">{m.content}</div>
                      </article>
                    ))}
                  </div>
                  <form
                    className="composer"
                    onSubmit={async (e) => {
                      e.preventDefault();
                      if (!recipient) return;
                      const result = await act<{ id: string }>("send_message", {
                        input: {
                          agent_id: recipient,
                          group_id: group?.id || null,
                          job_id: null,
                          prompt: draft,
                        },
                      });
                      if (result) {
                        setDraft("");
                        setInspect(result.id);
                      }
                    }}
                  >
                    <textarea
                      aria-label="Message"
                      value={draft}
                      onChange={(e) => setDraft(e.target.value)}
                      placeholder="Describe a task, teach a process, or join the conversation…"
                      rows={3}
                    />
                    <div className="composer-bottom">
                      {group ? (
                        <select
                          aria-label="Recipient"
                          value={recipient}
                          onChange={(e) => setRecipient(e.target.value)}
                        >
                          <option value="" disabled>
                            Choose agent
                          </option>
                          {group.members.map((id) => (
                            <option key={id} value={id}>
                              @{name(id)}
                            </option>
                          ))}
                        </select>
                      ) : (
                        <span className="muted">
                          {agent?.provider} · private conversation
                        </span>
                      )}
                      <button
                        className="primary"
                        disabled={
                          busy || !draft.trim() || !recipient || group?.paused
                        }
                      >
                        Send ↗
                      </button>
                    </div>
                  </form>
                </section>
                <aside className="activity-panel">
                  <div className="panel-heading">
                    <h3>Executions</h3>
                    <span>{relevantRuns.length}</span>
                  </div>
                  {relevantRuns.length === 0 && (
                    <p className="muted padded">
                      Each agent’s work will appear here.
                    </p>
                  )}
                  {relevantRuns.map((r) => (
                    <button
                      className={`run-card ${inspect === r.id ? "chosen" : ""}`}
                      key={r.id}
                      onClick={() => setInspect(r.id)}
                    >
                      <div>
                        <strong>{name(r.agent_id)}</strong>
                        <Badge status={r.status} />
                      </div>
                      <p>{r.prompt}</p>
                      <small>{date(r.created_at)}</small>
                    </button>
                  ))}
                </aside>
              </div>
              {agent && (
                <section className="memory-section">
                  <div className="panel-heading">
                    <h2>What {agent.name} remembers</h2>
                    <button
                      onClick={() =>
                        setEditor({
                          kind: "memory",
                          value: {
                            id: uid(),
                            agent_id: agent.id,
                            content: "",
                            approved: true,
                          },
                        })
                      }
                    >
                      + Add memory
                    </button>
                  </div>
                  <p className="muted">
                    Lessons proposed in chat wait for your review before
                    influencing future work.
                  </p>
                  {data.memories
                    .filter((m) => m.agent_id === agent.id)
                    .map((m) => (
                      <div className="memory-card" key={m.id}>
                        <Badge status={m.approved ? "approved" : "pending"} />
                        <p>{m.content}</p>
                        <div className="actions">
                          {!m.approved && (
                            <button
                              onClick={() =>
                                saveDirect("memory", { ...m, approved: true })
                              }
                            >
                              Approve
                            </button>
                          )}
                          <button
                            onClick={() =>
                              setEditor({ kind: "memory", value: { ...m } })
                            }
                          >
                            Edit
                          </button>
                          <button
                            className="danger"
                            onClick={() =>
                              act("delete_entity", { kind: "memory", id: m.id })
                            }
                          >
                            Delete
                          </button>
                        </div>
                      </div>
                    ))}
                </section>
              )}
            </>
          )}
          {selectedApprovals.length > 0 && (
            <section className="approval-section">
              <h2>Waiting for your decision</h2>
              {selectedApprovals.map((a) => (
                <div className="approval-card" key={a.id}>
                  <div>
                    <strong>
                      {a.kind === "handoff"
                        ? "Agent handoff"
                        : "Tool execution"}
                    </strong>
                    <pre>{JSON.stringify(a.payload, null, 2)}</pre>
                  </div>
                  <div className="actions">
                    <button
                      onClick={() =>
                        act("resolve_approval", {
                          approvalId: a.id,
                          allow: false,
                        })
                      }
                    >
                      Reject
                    </button>
                    <button
                      className="primary"
                      onClick={() =>
                        act("resolve_approval", {
                          approvalId: a.id,
                          allow: true,
                        })
                      }
                    >
                      Approve once
                    </button>
                  </div>
                </div>
              ))}
            </section>
          )}
          {page === "Jobs" && (
            <>
              <div className="banner">
                Schedules run while OpenDots is in your tray. Missed runs are
                recorded for review, never automatically replayed.
              </div>
              <div className="job-list">
                {data.jobs.map((j) => (
                  <div className="job-card" key={j.id}>
                    <div>
                      <h2>{j.name}</h2>
                      <p>{j.prompt}</p>
                      <small>
                        {name(j.agent_id)} · {j.recurrence} · {j.timezone}
                      </small>
                      <p className="muted">
                        {j.paused
                          ? "Paused"
                          : j.next_at
                            ? `Next: ${date(j.next_at)}`
                            : "No future occurrence"}
                      </p>
                    </div>
                    <div className="actions">
                      <button
                        className="primary"
                        onClick={() => act("run_job", { jobId: j.id })}
                      >
                        Run now
                      </button>
                      <button
                        onClick={() =>
                          saveDirect("job", { ...j, paused: !j.paused })
                        }
                      >
                        {j.paused ? "Resume" : "Pause"}
                      </button>
                      <button
                        onClick={() =>
                          setEditor({ kind: "job", value: { ...j } })
                        }
                      >
                        Edit
                      </button>
                    </div>
                  </div>
                ))}
              </div>
              <h2>Schedule history</h2>
              {data.occurrences
                .slice()
                .reverse()
                .map((o) => (
                  <div className="history-row" key={o.id}>
                    <span>
                      {data.jobs.find((j) => j.id === o.job_id)?.name ||
                        "Deleted job"}
                    </span>
                    <time>{date(o.at)}</time>
                    <Badge status={o.status} />
                    {o.error && <span>{o.error}</span>}
                  </div>
                ))}
              <h2>Job executions</h2>
              {data.executions
                .filter((r) => r.job_id)
                .map((r) => (
                  <button
                    className="history-row"
                    key={r.id}
                    onClick={() => setInspect(r.id)}
                  >
                    <span>{name(r.agent_id)}</span>
                    <span>{r.prompt.slice(0, 90)}</span>
                    <Badge status={r.status} />
                  </button>
                ))}
            </>
          )}
          {page === "Settings" && (
            <>
              <section className="settings-section">
                <h2>Appearance</h2>
                <p className="muted">
                  Choose a light or dark workspace, or follow your device’s
                  appearance.
                </p>
                <Field label="Color theme">
                  <select
                    value={appearance}
                    onChange={(event) => {
                      const value = event.target.value as Appearance;
                      setAppearance(value);
                      saveAppearance(value);
                    }}
                  >
                    <option value="system">System</option>
                    <option value="light">Light</option>
                    <option value="dark">Dark</option>
                  </select>
                </Field>
              </section>
              <BrowserSettings agents={data.agents} />
              <section className="settings-section">
                <h2>Execution providers</h2>
                <p className="muted">
                  Install and sign in with each provider’s CLI. Native tools and
                  provider plugins are restricted in managed sessions; assign
                  MCP capabilities to each agent.
                </p>
                {(["claude", "codex", "cursor"] as const).map((p) => (
                  <div className="provider-card" key={p}>
                    <div>
                      <strong>
                        {p === "claude"
                          ? "Claude Code"
                          : p === "codex"
                            ? "Codex"
                            : "Cursor"}
                      </strong>
                      <small>
                        Sign in in your terminal:{" "}
                        {p === "claude"
                          ? "claude auth login"
                          : p === "codex"
                            ? "codex login"
                            : "agent login"}
                      </small>
                    </div>
                    <button
                      onClick={async () => {
                        const r = await act("discover_provider", {
                          provider: p,
                          path: "",
                        });
                        if (r) setChecks((s) => ({ ...s, [p]: r }));
                      }}
                    >
                      Check installation
                    </button>
                    {!!checks[p] && (
                      <pre>{JSON.stringify(checks[p], null, 2)}</pre>
                    )}
                  </div>
                ))}
              </section>
              <section className="settings-section">
                <div className="panel-heading">
                  <h2>MCP integrations</h2>
                  <button
                    className="primary"
                    onClick={() =>
                      setEditor({
                        kind: "integration",
                        value: {
                          id: uid(),
                          name: "",
                          url: "",
                          command: "",
                          args: [],
                        },
                      })
                    }
                  >
                    + Connect server
                  </button>
                </div>
                <p className="muted">
                  Connect a running server or a separately installed command.
                  Agents only receive integrations you assign.
                </p>
                {data.integrations.map((i) => (
                  <div className="integration-card" key={i.id}>
                    <div className="panel-heading">
                      <div>
                        <h3>{i.name}</h3>
                        <small>{i.url || i.command}</small>
                      </div>
                      <div className="actions">
                        <button onClick={() => testIntegration(i.id)}>
                          Discover tools
                        </button>
                        <button
                          onClick={() => setCredential({ id: i.id, value: "" })}
                        >
                          Bearer token
                        </button>
                        <button
                          onClick={() =>
                            setEditor({ kind: "integration", value: { ...i } })
                          }
                        >
                          Edit
                        </button>
                      </div>
                    </div>
                    {toolLists[i.id]?.map((t) => (
                      <div className="tool-row" key={t.name}>
                        <strong>{t.name}</strong>
                        <p>{t.description}</p>
                      </div>
                    ))}
                  </div>
                ))}
              </section>
            </>
          )}
        </div>
      </main>
      {inspect && currentRun && (
        <aside className="inspector">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">EXECUTION</p>
              <h2>{name(currentRun.agent_id)}</h2>
            </div>
            <button onClick={() => setInspect("")} aria-label="Close inspector">
              ×
            </button>
          </div>
          <Badge status={currentRun.status} />
          <p>{currentRun.prompt}</p>
          <small>{currentRun.id}</small>
          {currentRun.error && (
            <div className="banner error">{currentRun.error}</div>
          )}
          <div className="actions">
            {active(currentRun.status) ? (
              <button
                className="danger"
                onClick={() =>
                  act("cancel_execution", { executionId: inspect })
                }
              >
                Cancel execution
              </button>
            ) : (
              <button
                onClick={async () => {
                  const r = await act<{ id: string }>("retry_execution", {
                    executionId: inspect,
                  });
                  if (r) setInspect(r.id);
                }}
              >
                Retry as new run
              </button>
            )}
          </div>
          {!active(currentRun.status) && (
            <small>
              Retrying may repeat external actions. Review the tool history
              first.
            </small>
          )}
          <div className="stream-output">
            {events
              .filter((e) => e.kind === "text")
              .map((e) => String(e.data.text || ""))
              .join("")}
          </div>
          <BrowserInspector executionId={inspect} onReplay={setInspect} />
          <h3>Activity timeline</h3>
          {events
            .filter((e) => e.kind !== "text")
            .map((e) => (
              <details className="event" key={e.seq}>
                <summary>
                  <span>{e.kind.replace(/_/g, " ")}</span>
                  <time>{date(e.created_at)}</time>
                </summary>
                <pre>{JSON.stringify(e.data, null, 2)}</pre>
              </details>
            ))}
        </aside>
      )}
      {editor && (
        <div className="modal-backdrop">
          <section
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-label={`Edit ${editor.kind}`}
          >
            {error && (
              <div className="banner error" role="alert">
                {error}
              </div>
            )}
            <div className="panel-heading">
              <h2>
                {editor.kind === "memory"
                  ? "Review memory"
                  : `Configure ${editor.kind}`}
              </h2>
              <button onClick={() => setEditor(null)} aria-label="Close editor">
                ×
              </button>
            </div>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void save();
              }}
            >
              {"name" in editor.value && (
                <Field label="Name">
                  <input
                    required
                    value={editor.value.name}
                    onChange={(e) => update({ name: e.target.value })}
                  />
                </Field>
              )}
              {editor.kind === "agent" && (
                <>
                  <div className="form-grid">
                    <Field label="Role">
                      <input
                        value={editor.value.role}
                        onChange={(e) => update({ role: e.target.value })}
                        placeholder="Researcher, creator, publisher…"
                      />
                    </Field>
                    <Field label="Provider">
                      <select
                        value={editor.value.provider}
                        onChange={(e) => update({ provider: e.target.value })}
                      >
                        <option value="claude">Claude Code</option>
                        <option value="codex">Codex</option>
                        <option value="cursor">Cursor</option>
                      </select>
                    </Field>
                  </div>
                  <Field label="Instructions">
                    <textarea
                      rows={5}
                      value={editor.value.instructions}
                      onChange={(e) => update({ instructions: e.target.value })}
                      placeholder="What does this agent do? How should it work with you?"
                    />
                  </Field>
                  <div className="form-grid">
                    <Field label="Model (optional)">
                      <input
                        value={editor.value.model}
                        onChange={(e) => update({ model: e.target.value })}
                        placeholder="Provider default"
                      />
                    </Field>
                    <Field label="Executable path (optional)">
                      <input
                        value={editor.value.executable}
                        onChange={(e) => update({ executable: e.target.value })}
                        placeholder="Auto-detect"
                      />
                    </Field>
                  </div>
                  <Field
                    label="Workspace reference"
                    hint="Agents access this workspace through their assigned tools."
                  >
                    <input
                      value={editor.value.workspace}
                      onChange={(e) => update({ workspace: e.target.value })}
                    />
                  </Field>
                  <Field label="Import a Markdown skill">
                    <input
                      type="file"
                      accept=".md"
                      onChange={async (e) => {
                        const f = e.target.files?.[0];
                        if (f) {
                          if (f.size > 100000) {
                            setError("Skill exceeds 100 KB");
                            return;
                          }
                          const text = await f.text();
                          setEditor((current) =>
                            current?.kind === "agent"
                              ? {
                                  ...current,
                                  value: {
                                    ...current.value,
                                    skills: [...current.value.skills, text],
                                  },
                                }
                              : current,
                          );
                        }
                      }}
                    />
                  </Field>
                  {editor.value.skills.map((s, i) => (
                    <div className="skill-row" key={i}>
                      <span>{s.split("\n").find(Boolean)?.slice(0, 90)}</span>
                      <button
                        type="button"
                        onClick={() =>
                          update({
                            skills: (editor.value as Agent).skills.filter(
                              (_, j) => i !== j,
                            ),
                          })
                        }
                      >
                        Remove
                      </button>
                    </div>
                  ))}
                  <BrowserPolicies
                    agent={editor.value as Agent}
                    onChange={(policies) => update({ policies })}
                  />
                  <h3>Assigned integrations</h3>
                  {data.integrations.map((i) => {
                    const a = editor.value as Agent;
                    return (
                      <div className="integration-selection" key={i.id}>
                        <label className="checkbox">
                          <input
                            type="checkbox"
                            checked={a.integrations.includes(i.id)}
                            onChange={(e) =>
                              update({
                                integrations: e.target.checked
                                  ? [...a.integrations, i.id]
                                  : a.integrations.filter((id) => id !== i.id),
                              })
                            }
                          />
                          {i.name}
                        </label>
                        {a.integrations.includes(i.id) && (
                          <>
                            <button
                              type="button"
                              onClick={() => testIntegration(i.id)}
                            >
                              Load tool policies
                            </button>
                            {toolLists[i.id]?.map((t) => (
                              <div className="policy-row" key={t.name}>
                                <span>{t.name}</span>
                                <select
                                  aria-label={`Policy for ${t.name}`}
                                  value={
                                    a.policies[`${i.id}:${t.name}`] || "ask"
                                  }
                                  onChange={(e) =>
                                    update({
                                      policies: {
                                        ...a.policies,
                                        [`${i.id}:${t.name}`]: e.target.value,
                                      },
                                    })
                                  }
                                >
                                  <option value="ask">Ask me</option>
                                  <option value="allow">Allow</option>
                                  <option value="deny">Deny</option>
                                </select>
                              </div>
                            ))}
                          </>
                        )}
                      </div>
                    );
                  })}
                  {!data.integrations.length && (
                    <p className="muted">
                      Connect tools in Settings, then assign them here.
                    </p>
                  )}
                </>
              )}
              {editor.kind === "group" && (
                <>
                  <Field label="Purpose">
                    <textarea
                      rows={3}
                      value={editor.value.purpose}
                      onChange={(e) => update({ purpose: e.target.value })}
                    />
                  </Field>
                  <h3>Members</h3>
                  {data.agents.map((a) => (
                    <label className="checkbox" key={a.id}>
                      <input
                        type="checkbox"
                        checked={(editor.value as Group).members.includes(a.id)}
                        onChange={(e) => {
                          const g = editor.value as Group;
                          update({
                            members: e.target.checked
                              ? [...g.members, a.id]
                              : g.members.filter((id) => id !== a.id),
                          });
                        }}
                      />
                      {a.name} <small>{a.role}</small>
                    </label>
                  ))}
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={editor.value.automatic}
                      onChange={(e) => update({ automatic: e.target.checked })}
                    />
                    Allow automatic handoffs between members
                  </label>
                  <Field label="Maximum handoffs per task chain">
                    <input
                      type="number"
                      min={1}
                      max={1000}
                      value={editor.value.handoff_limit}
                      onChange={(e) =>
                        update({ handoff_limit: Number(e.target.value) })
                      }
                    />
                  </Field>
                  <p className="muted">
                    Tool policies are configured per agent. Unknown tools always
                    ask for approval.
                  </p>
                </>
              )}
              {editor.kind === "integration" && (
                <>
                  <Field
                    label="Server URL"
                    hint="For an already-running HTTP MCP server."
                  >
                    <input
                      type="url"
                      value={editor.value.url}
                      placeholder="http://127.0.0.1:8000/mcp"
                      onChange={(e) =>
                        update({ url: e.target.value, command: "", args: [] })
                      }
                    />
                  </Field>
                  <p className="muted">
                    Or launch a separately installed server:
                  </p>
                  <Field label="Executable">
                    <input
                      value={editor.value.command}
                      onChange={(e) =>
                        update({ command: e.target.value, url: "" })
                      }
                    />
                  </Field>
                  <Field label="Arguments (one per line)">
                    <textarea
                      value={editor.value.args.join("\n")}
                      onChange={(e) =>
                        update({ args: e.target.value.split("\n") })
                      }
                    />
                  </Field>
                  <p className="muted">
                    Save first, then add a bearer token through Settings. Tokens
                    are kept in your OS credential store.
                  </p>
                </>
              )}
              {editor.kind === "memory" && (
                <>
                  <Field label="Lesson">
                    <textarea
                      required
                      rows={6}
                      value={editor.value.content}
                      onChange={(e) => update({ content: e.target.value })}
                    />
                  </Field>
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={editor.value.approved}
                      onChange={(e) => update({ approved: e.target.checked })}
                    />
                    Approved for future conversations and jobs
                  </label>
                </>
              )}
              {editor.kind === "job" && (
                <>
                  <Field label="Group (optional)">
                    <select
                      value={editor.value.group_id || ""}
                      onChange={(e) =>
                        update({
                          group_id: e.target.value || null,
                          agent_id: "",
                        })
                      }
                    >
                      <option value="">Individual agent</option>
                      {data.groups.map((g) => (
                        <option key={g.id} value={g.id}>
                          {g.name}
                        </option>
                      ))}
                    </select>
                  </Field>
                  <Field label="Starting agent">
                    <select
                      required
                      value={editor.value.agent_id}
                      onChange={(e) => update({ agent_id: e.target.value })}
                    >
                      <option value="">Choose agent</option>
                      {data.agents
                        .filter(
                          (a) =>
                            !(editor.value as Job).group_id ||
                            data.groups
                              .find(
                                (g) => g.id === (editor.value as Job).group_id,
                              )
                              ?.members.includes(a.id),
                        )
                        .map((a) => (
                          <option key={a.id} value={a.id}>
                            {a.name}
                          </option>
                        ))}
                    </select>
                  </Field>
                  <Field label="Task">
                    <textarea
                      required
                      rows={4}
                      value={editor.value.prompt}
                      onChange={(e) => update({ prompt: e.target.value })}
                    />
                  </Field>
                  <div className="form-grid">
                    <Field label="Local date and time">
                      <input
                        required
                        type="datetime-local"
                        value={editor.value.local_time}
                        onChange={(e) => update({ local_time: e.target.value })}
                      />
                    </Field>
                    <Field label="Timezone">
                      <input
                        required
                        value={editor.value.timezone}
                        onChange={(e) => update({ timezone: e.target.value })}
                      />
                    </Field>
                  </div>
                  <Field label="Repeat">
                    <select
                      value={editor.value.recurrence}
                      onChange={(e) => update({ recurrence: e.target.value })}
                    >
                      <option value="once">Once</option>
                      <option value="daily">Daily</option>
                      <option value="weekly">Weekly</option>
                    </select>
                  </Field>
                </>
              )}
              <footer className="modal-footer">
                <button
                  type="button"
                  className="danger"
                  onClick={async () => {
                    const r = await act("delete_entity", {
                      kind: editor.kind,
                      id: editor.value.id,
                    });
                    if (r !== undefined) {
                      setEditor(null);
                      setSelected("");
                    }
                  }}
                >
                  Delete
                </button>
                <div className="actions">
                  <button type="button" onClick={() => setEditor(null)}>
                    Cancel
                  </button>
                  <button className="primary" disabled={busy}>
                    Save changes
                  </button>
                </div>
              </footer>
            </form>
          </section>
        </div>
      )}
      {credential && (
        <div className="modal-backdrop">
          <form
            className="modal small"
            role="dialog"
            aria-modal="true"
            aria-label="Bearer token"
            onSubmit={async (e) => {
              e.preventDefault();
              const r = await act("set_secret", {
                integrationId: credential.id,
                secret: credential.value,
              });
              if (r !== undefined) {
                setCredential(null);
                setNotice("Credential updated");
              }
            }}
          >
            <h2>Integration bearer token</h2>
            <Field
              label="Token"
              hint="Stored in the OS credential store. Leave blank to remove."
            >
              <input
                autoComplete="off"
                type="password"
                value={credential.value}
                onChange={(e) =>
                  setCredential({ ...credential, value: e.target.value })
                }
              />
            </Field>
            <div className="actions">
              <button type="button" onClick={() => setCredential(null)}>
                Cancel
              </button>
              <button className="primary" disabled={busy}>
                Save token
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}
