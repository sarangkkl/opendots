import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Agent } from "./types";

interface Profile {
  id: string;
  name: string;
  agent_id: string;
  executable: string;
}
interface Upload {
  id: string;
  name: string;
  agent_id: string;
}
interface Artifact {
  name: string;
  bytes: number;
  complete: boolean;
}
const actions = [
  "navigate",
  "click",
  "click_at",
  "type",
  "keys",
  "scroll",
  "drag",
  "select",
  "new_tab",
  "switch_tab",
  "close_tab",
  "dialog",
  "upload",
  "download",
  "back",
];
export function BrowserPolicies({
  agent,
  onChange,
}: {
  agent: Agent;
  onChange: (policies: Record<string, string>) => void;
}) {
  return (
    <details>
      <summary>Browser interaction policies</summary>
      <p>
        Every interaction asks by default. Allowing clicks can allow submissions
        and publishing; OpenDots cannot infer a button’s consequences.
      </p>
      {actions.map((a) => (
        <label className="browser-policy" key={a}>
          <span>{a.replace(/_/g, " ")}</span>
          <select
            aria-label={`Browser ${a} policy`}
            value={agent.policies[`browser:${a}`] || "ask"}
            onChange={(e) =>
              onChange({ ...agent.policies, [`browser:${a}`]: e.target.value })
            }
          >
            <option value="ask">Ask each time</option>
            <option value="allow">Allow</option>
            <option value="deny">Deny</option>
          </select>
        </label>
      ))}
    </details>
  );
}
export function BrowserSettings({ agents }: { agents: Agent[] }) {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [uploads, setUploads] = useState<Upload[]>([]);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [edit, setEdit] = useState<Profile>({
    id: crypto.randomUUID(),
    name: "",
    agent_id: "",
    executable: "",
  });
  const [uploadPath, setUploadPath] = useState("");
  const [uploadAgent, setUploadAgent] = useState("");
  const reload = async () => {
    const r = await invoke<{ profiles: Profile[]; uploads: Upload[] }>(
      "browser_profiles",
    );
    setProfiles(r.profiles);
    setUploads(r.uploads);
  };
  const run = async (fn: () => Promise<unknown>) => {
    setError("");
    setMessage("");
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(String(e));
    }
  };
  useEffect(() => {
    void run(reload);
  }, []);
  return (
    <section className="settings-section">
      <h2>Saved browser profiles</h2>
      <p>
        Each agent gets separate Chrome logins and tabs. Open a profile, sign in
        manually, then choose Resume automation. Login and manual takeover are
        not recorded.
      </p>
      {error && (
        <p role="alert" className="banner error">
          {error}
        </p>
      )}
      {message && <p role="status">{message}</p>}
      {profiles.map((p) => (
        <div className="provider-card" key={p.id}>
          <div>
            <strong>{p.name}</strong>
            <small>
              {agents.find((a) => a.id === p.agent_id)?.name || "Deleted agent"}
            </small>
          </div>
          <div className="actions">
            <button onClick={() => setEdit(p)}>Edit</button>
            <button
              onClick={() =>
                void run(() =>
                  invoke("browser_control", { profileId: p.id, mode: "login" }),
                )
              }
            >
              Log in / take over
            </button>
            <button
              onClick={() =>
                void run(() =>
                  invoke("browser_control", {
                    profileId: p.id,
                    mode: "resume",
                  }),
                )
              }
            >
              Resume automation
            </button>
            <button
              onClick={() =>
                void run(() =>
                  invoke("browser_control", { profileId: p.id, mode: "close" }),
                )
              }
            >
              Close browser
            </button>
            <button
              className="danger"
              onClick={() => {
                if (
                  window.confirm(`Delete ${p.name} and all its saved logins?`)
                )
                  void run(() =>
                    invoke("delete_browser_profile", { profileId: p.id }),
                  );
              }}
            >
              Delete
            </button>
          </div>
        </div>
      ))}
      <form
        className="browser-form"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            await invoke("save_browser_profile", { profile: edit });
            setEdit({
              id: crypto.randomUUID(),
              name: "",
              agent_id: "",
              executable: "",
            });
          });
        }}
      >
        <h3>
          {profiles.some((p) => p.id === edit.id)
            ? "Edit profile"
            : "Create profile"}
        </h3>
        <label>
          Profile name
          <input
            required
            value={edit.name}
            onChange={(e) => setEdit({ ...edit, name: e.target.value })}
          />
        </label>
        <label>
          Agent
          <select
            required
            aria-label="Profile agent"
            value={edit.agent_id}
            onChange={(e) => setEdit({ ...edit, agent_id: e.target.value })}
          >
            <option value="">Choose agent</option>
            {agents.map((a) => (
              <option value={a.id} key={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Chrome executable (optional)
          <input
            placeholder="Auto-detect Chrome / Chromium"
            value={edit.executable}
            onChange={(e) => setEdit({ ...edit, executable: e.target.value })}
          />
        </label>
        <button type="submit">Save browser profile</button>
      </form>
      <h3>Files available for upload</h3>
      <p>
        Choose an agent and a local file path. OpenDots copies the file into its
        managed upload storage; the agent still needs approval to upload it.
      </p>
      <form
        className="browser-form"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            const id = await invoke<string>("add_browser_upload", {
              agentId: uploadAgent,
              path: uploadPath,
            });
            setMessage(`Upload artifact: ${id}`);
            setUploadPath("");
          });
        }}
      >
        <select
          aria-label="Upload agent"
          required
          value={uploadAgent}
          onChange={(e) => setUploadAgent(e.target.value)}
        >
          <option value="">Choose agent</option>
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
        <input
          required
          aria-label="Upload file path"
          placeholder="Absolute file path"
          value={uploadPath}
          onChange={(e) => setUploadPath(e.target.value)}
        />
        <button>Allow this file</button>
      </form>
      {uploads.map((u) => (
        <p key={u.id}>
          {u.name} <small>{u.id}</small>
        </p>
      ))}
    </section>
  );
}
export function BrowserInspector({
  executionId,
  onReplay,
}: {
  executionId: string;
  onReplay: (id: string) => void;
}) {
  const [status, setStatus] = useState<{
    profile?: Profile;
    manual?: boolean;
    owner?: string;
    frame?: { image: string };
  }>();
  const [files, setFiles] = useState<Artifact[]>([]);
  const [error, setError] = useState("");
  const [preview, setPreview] = useState<{
    url: string;
    mime: string;
    text?: string;
  }>();
  const [destination, setDestination] = useState("");
  useEffect(() => {
    let alive = true;
    let polling = false;
    setStatus(undefined);
    setPreview(undefined);
    setFiles([]);
    setError("");
    const refresh = async () => {
      if (polling) return;
      polling = true;
      try {
        const [s, f] = await Promise.all([
          invoke<typeof status>("browser_status", { executionId }),
          invoke<Artifact[]>("browser_artifacts", { executionId }),
        ]);
        if (alive) {
          setStatus(s);
          setFiles(f);
        }
      } catch {
        /* An agent without a browser profile still has an ordinary execution inspector. */
      } finally {
        polling = false;
      }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 1000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [executionId]);
  useEffect(
    () => () => {
      if (preview) URL.revokeObjectURL(preview.url);
    },
    [preview],
  );
  const run = async (fn: () => Promise<unknown>) => {
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    }
  };
  return (
    <section className="browser-inspector">
      <h3>Browser</h3>
      {error && (
        <p role="alert" className="banner error">
          {error}
        </p>
      )}
      {status?.profile ? (
        <>
          <p>
            {status.profile.name} ·{" "}
            {status.manual
              ? "Manual control — recording paused"
              : status.owner === executionId
                ? "Agent control"
                : "Idle"}
          </p>
          {status.frame?.image &&
          status.owner === executionId &&
          !status.manual ? (
            <img
              alt="Live agent browser viewport"
              src={`data:image/jpeg;base64,${status.frame.image}`}
            />
          ) : (
            <p className="muted">No active capture</p>
          )}
          <div className="actions">
            <button
              onClick={() =>
                void run(() =>
                  invoke("browser_control", {
                    profileId: status.profile!.id,
                    mode: "takeover",
                  }),
                )
              }
            >
              Take over
            </button>
            <button
              onClick={() =>
                void run(() =>
                  invoke("browser_control", {
                    profileId: status.profile!.id,
                    mode: "resume",
                  }),
                )
              }
            >
              Resume
            </button>
            <button
              onClick={() =>
                void run(async () => {
                  const r = await invoke<{ id: string }>("replay_browser", {
                    executionId,
                  });
                  onReplay(r.id);
                })
              }
            >
              Replay with current approvals
            </button>
          </div>
          <small>
            Recording can contain authenticated page content. Login and
            manual-control periods are excluded. Replay can repeat external
            actions.
          </small>
        </>
      ) : (
        <p className="muted">
          Assign this agent a browser profile in Settings.
        </p>
      )}
      <h4>
        Artifacts ·{" "}
        {(files.reduce((sum, f) => sum + f.bytes, 0) / 1024 / 1024).toFixed(1)}{" "}
        MB
      </h4>
      <label>
        Export destination
        <input
          aria-label="Artifact export destination"
          placeholder="Absolute destination file path"
          value={destination}
          onChange={(e) => setDestination(e.target.value)}
        />
      </label>
      {files.map((f) => (
        <div className="browser-artifact" key={f.name}>
          <small>
            {f.name} · {Math.round(f.bytes / 1024)} KB{" "}
            {!f.complete && "· recording / incomplete"}
          </small>
          <div className="actions">
            {f.complete && /\.(webm|png|txt|json)$/.test(f.name) && (
              <button
                onClick={() =>
                  void run(async () => {
                    const r = await invoke<{ data: string; mime: string }>(
                      "read_browser_artifact",
                      { executionId, name: f.name },
                    );
                    const bytes = Uint8Array.from(atob(r.data), (c) =>
                      c.charCodeAt(0),
                    );
                    setPreview({
                      url: URL.createObjectURL(
                        new Blob([bytes], { type: r.mime }),
                      ),
                      mime: r.mime,
                      text:
                        r.mime === "text/plain"
                          ? new TextDecoder().decode(bytes)
                          : undefined,
                    });
                  })
                }
              >
                Preview
              </button>
            )}
            <button
              disabled={!destination || !f.complete}
              onClick={() =>
                void run(() =>
                  invoke("export_browser_artifact", {
                    executionId,
                    name: f.name,
                    destination,
                  }),
                )
              }
            >
              Export
            </button>
            <button
              onClick={() => {
                if (window.confirm(`Delete ${f.name}?`))
                  void run(() =>
                    invoke("delete_browser_artifact", {
                      executionId,
                      name: f.name,
                    }),
                  );
              }}
            >
              Delete
            </button>
          </div>
        </div>
      ))}
      {preview &&
        (preview.mime === "text/plain" ? (
          <pre>{preview.text}</pre>
        ) : preview.mime.startsWith("video") ? (
          <video controls src={preview.url} />
        ) : (
          <img alt="Browser screenshot evidence" src={preview.url} />
        ))}
    </section>
  );
}
