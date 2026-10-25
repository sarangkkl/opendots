import { useEffect, useState } from "react";
import Markdown from "react-markdown";
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { Agent, Execution, Message } from "./types";

export function Chat({
  conversation,
  agents,
  executions,
  version,
  inspect,
  error,
  paused = false,
  initialReply,
  onPosted,
}: {
  conversation: string;
  agents: Agent[];
  executions: Execution[];
  version: unknown;
  inspect: (id: string) => void;
  error: (s: string) => void;
  paused?: boolean;
  initialReply?: string;
  onPosted: () => void;
}) {
  const [messages, setMessages] = useState<Message[]>([]),
    [thread, setThread] = useState<Message | null>(null),
    [replies, setReplies] = useState<Message[]>([]);
  const [draft, setDraft] = useState(""),
    [mentions, setMentions] = useState<string[]>([]),
    [activity, setActivity] = useState(false),
    [busy, setBusy] = useState(false),
    [reply, setReply] = useState<Message | null>(null),
    [request, setRequest] = useState(crypto.randomUUID());
  const [hasOlder, setHasOlder] = useState(false);
  const mergeMessages = (current: Message[], incoming: Message[]) =>
    Array.from(
      new Map([...current, ...incoming].map((m) => [m.id, m])).values(),
    ).sort((a, b) => (a.position || 0) - (b.position || 0));
  const name = (id: string | null) =>
    id === null || id === "you"
      ? "You"
      : agents.find((a) => a.id === id)?.name || "Former member";
  useEffect(() => {
    let alive = true;
    if (isTauri())
      void invoke<Message[]>("history", {
        conversationId: conversation,
        before: null,
        threadId: null,
      })
        .then((v) => {
          if (alive) {
            setMessages((current) => mergeMessages(current, v));
            setHasOlder(v.length === 50);
            if (initialReply) {
              const parent = v.find((m) => m.id === initialReply);
              if (!parent)
                void invoke<Message>("chat_message", { id: initialReply })
                  .then((m) => {
                    if (alive && m.conversation_id === conversation) {
                      setThread(m);
                      setReply(m);
                    }
                  })
                  .catch((e) => error(String(e)));
              if (parent) {
                setThread(parent);
                setReply(parent);
              }
            }
          }
        })
        .catch((e) => error(String(e)));
    return () => {
      alive = false;
    };
  }, [conversation, version, initialReply]);
  useEffect(() => {
    let alive = true;
    if (thread && isTauri())
      void invoke<Message[]>("history", {
        conversationId: conversation,
        before: null,
        threadId: thread.thread_id || thread.id,
      })
        .then((v) => {
          if (alive) setReplies(v);
        })
        .catch((e) => error(String(e)));
    return () => {
      alive = false;
    };
  }, [thread, version]);
  const add = (id: string) => {
    setMentions((v) => (v.includes(id) ? v : [...v, id]));
    setDraft((v) => v.replace(/@[^\s@]*$/, ""));
  };
  const query = draft.match(/@([^\s@]*)$/)?.[1];
  const chooseReply = (m: Message) => {
    setThread(messages.find((p) => p.id === (m.thread_id || m.id)) || m);
    setReply(m);
  };
  const render = (m: Message) => (
    <article className="message" key={m.id}>
      <div className="message-meta">
        <span className="avatar">{name(m.agent_id)[0]}</span>
        <strong>{name(m.agent_id)}</strong>
        <time>{new Date(m.created_at).toLocaleString()}</time>
        {m.execution_id && executions.find((r) => r.id === m.execution_id) && (
          <span className="badge">
            {executions
              .find((r) => r.id === m.execution_id)
              ?.status.replace(/_/g, " ")}
          </span>
        )}
        <button className="text-button" onClick={() => chooseReply(m)}>
          Reply
        </button>
        {m.execution_id && (
          <button
            className="text-button"
            onClick={() => inspect(m.execution_id!)}
          >
            View execution ↗
          </button>
        )}
      </div>
      <div className="message-content markdown">
        <div className="mention-chips">
          {m.mentions?.map((id) => (
            <span key={id}>@{name(id)}</span>
          ))}
        </div>
        <Markdown
          skipHtml
          components={{
            img: () => null,
            a: ({ href, children }) => (
              <a href={href} target="_blank" rel="noreferrer noopener">
                {children}
              </a>
            ),
          }}
        >
          {m.content}
        </Markdown>
        {m.attachments?.map((a) =>
          a.startsWith("browser:") ? (
            <button
              type="button"
              key={a}
              onClick={() => inspect(a.slice(8).split("/")[0])}
            >
              📎 {a.split("/").pop()}
            </button>
          ) : (
            <code key={a}>{a}</code>
          ),
        )}
        {(m.reply_count ||
          messages.filter((r) => r.thread_id === m.id).length) > 0 && (
          <button className="text-button" onClick={() => chooseReply(m)}>
            {m.reply_count ||
              messages.filter((r) => r.thread_id === m.id).length}{" "}
            replies · View thread
          </button>
        )}
      </div>
    </article>
  );
  return (
    <div className={`channel-layout ${thread || activity ? "with-panel" : ""}`}>
      <section className="chat-panel">
        <div className="panel-heading">
          <h3>Messages</h3>
          <button
            onClick={() => {
              setActivity(!activity);
              setThread(null);
            }}
          >
            Activity · {executions.length}
          </button>
        </div>
        <div className="messages" aria-label="Conversation history">
          {hasOlder && (
            <button
              onClick={async () => {
                try {
                  const older = await invoke<Message[]>("history", {
                    conversationId: conversation,
                    before: messages[0]?.position.toString(),
                    threadId: null,
                  });
                  setMessages([...older, ...messages]);
                  setHasOlder(older.length === 50);
                } catch (e) {
                  error(String(e));
                }
              }}
            >
              Load older messages
            </button>
          )}
          {!messages.length && (
            <p className="muted padded">
              Post an update, or mention an agent to start work.
            </p>
          )}
          {messages
            .filter(
              (m) =>
                !m.thread_id || !messages.some((p) => p.id === m.thread_id),
            )
            .map(render)}
        </div>
        <form
          className="composer"
          onSubmit={async (e) => {
            e.preventDefault();
            setBusy(true);
            try {
              await invoke("post_chat", {
                input: {
                  conversation_id: conversation,
                  content: draft,
                  mentions,
                  reply_to: reply?.id || null,
                  request_id: request,
                },
              });
              onPosted();
              setDraft("");
              setMentions([]);
              setRequest(crypto.randomUUID());
              const v = await invoke<Message[]>("history", {
                conversationId: conversation,
                before: null,
                threadId: null,
              });
              setMessages((current) => mergeMessages(current, v));
              if (thread)
                setReplies(
                  await invoke("history", {
                    conversationId: conversation,
                    before: null,
                    threadId: thread.thread_id || thread.id,
                  }),
                );
            } catch (e) {
              error(String(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          {reply && (
            <div className="reply-context">
              Replying to {name(reply.agent_id)}
              <div className="reply-preview">
                <Markdown skipHtml components={{ img: () => null }}>
                  {reply.content.slice(0, 160)}
                </Markdown>
              </div>
              <button type="button" onClick={() => setReply(null)}>
                Cancel reply
              </button>
            </div>
          )}
          <div className="mention-chips">
            {mentions.map((id) => (
              <button
                type="button"
                key={id}
                onClick={() => setMentions((v) => v.filter((x) => x !== id))}
              >
                @{name(id)} ×
              </button>
            ))}
          </div>
          <textarea
            aria-label="Message"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== "Enter" || e.shiftKey) return;
              e.preventDefault();
              if (query !== undefined) {
                const match = [...agents, { id: "you", name: "You" }].find(
                  (a) => a.name.toLowerCase().includes(query.toLowerCase()),
                );
                if (match) add(match.id);
              } else {
                const form = e.currentTarget.form;
                const submit =
                  form?.querySelector<HTMLButtonElement>("button.primary");
                if (submit && !submit.disabled) form?.requestSubmit(submit);
              }
            }}
            rows={2}
            placeholder="Message · type @ to address a member"
          />
          {query !== undefined && (
            <div
              className="mention-menu"
              role="listbox"
              aria-label="Mention a member"
            >
              {[
                ...agents.map((a) => ({ id: a.id, name: a.name })),
                { id: "you", name: "You" },
              ]
                .filter((a) =>
                  a.name.toLowerCase().includes(query.toLowerCase()),
                )
                .map((a) => (
                  <button
                    type="button"
                    role="option"
                    aria-selected={mentions.includes(a.id)}
                    key={a.id}
                    onClick={() => add(a.id)}
                  >
                    @{a.name}
                  </button>
                ))}
            </div>
          )}
          <div className="composer-bottom">
            <select
              aria-label="Recipient"
              value=""
              onChange={(e) => {
                if (e.target.value) add(e.target.value);
              }}
            >
              <option value="">Mention a member…</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  @{a.name}
                </option>
              ))}
              <option value="you">@You</option>
            </select>
            <small className="muted">
              {conversation.startsWith("agent:")
                ? "Private conversation"
                : mentions.length || reply
                  ? "Addressed message"
                  : "Channel update · no agent dispatched"}
            </small>
            <button
              className="primary"
              disabled={
                busy ||
                !draft.trim() ||
                (paused &&
                  (mentions.some((id) => id !== "you") || !!reply?.agent_id))
              }
            >
              Send ↗
            </button>
          </div>
        </form>
      </section>
      {thread && (
        <aside className="thread-panel">
          <div className="panel-heading">
            <h3>Thread</h3>
            <button
              onClick={() => {
                setThread(null);
                setReply(null);
              }}
            >
              Close
            </button>
          </div>
          <div className="thread-messages">
            {replies.length >= 50 && (
              <button
                onClick={async () => {
                  try {
                    const older = await invoke<Message[]>("history", {
                      conversationId: conversation,
                      before: replies[0]?.position.toString(),
                      threadId: thread.thread_id || thread.id,
                    });
                    setReplies((current) => mergeMessages(older, current));
                  } catch (e) {
                    error(String(e));
                  }
                }}
              >
                Load older replies
              </button>
            )}
            {replies.map(render)}
          </div>
          <button onClick={() => setReply(thread)}>Reply in thread</button>
        </aside>
      )}
      {activity && !thread && (
        <aside className="activity-panel">
          <div className="panel-heading">
            <h3>Executions</h3>
            <button onClick={() => setActivity(false)}>Close</button>
          </div>
          {executions.map((r) => (
            <button
              className="run-card"
              key={r.id}
              onClick={() => inspect(r.id)}
            >
              <strong>{name(r.agent_id)}</strong>
              <span className={`badge ${r.status}`}>
                {r.status.replace(/_/g, " ")}
              </span>
              <p>{r.prompt}</p>
            </button>
          ))}
        </aside>
      )}
    </div>
  );
}
export function WorkingFolder() {
  const [path, setPath] = useState(""),
    [status, setStatus] = useState("");
  useEffect(() => {
    if (isTauri())
      void invoke<string | null>("working_folder")
        .then((v) => setPath(v || ""))
        .catch((e) => setStatus(String(e)));
  }, []);
  return (
    <section className="settings-section">
      <h2>Working folder</h2>
      <p className="muted">
        Each agent has a private agents/&lt;id&gt; folder. File tools stay
        inside it; sharing copies files after approval.
      </p>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          try {
            await invoke("set_working_folder", { path });
            setStatus("Working folder saved");
          } catch (e) {
            setStatus(String(e));
          }
        }}
      >
        <input
          aria-label="Working folder"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="Choose an existing absolute directory"
        />
        <button>Set working folder</button>
      </form>
      <p role="status">{status}</p>
    </section>
  );
}
