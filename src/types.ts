export type Provider = "claude" | "codex" | "cursor";
export interface Agent {
  id: string;
  name: string;
  role: string;
  instructions: string;
  provider: Provider;
  model: string;
  executable: string;
  workspace: string;
  skills: string[];
  integrations: string[];
  policies: Record<string, string>;
}
export interface Group {
  id: string;
  name: string;
  purpose: string;
  members: string[];
  automatic: boolean;
  paused: boolean;
  handoff_limit: number;
}
export interface Integration {
  id: string;
  name: string;
  url: string;
  command: string;
  args: string[];
}
export interface Memory {
  id: string;
  agent_id: string;
  content: string;
  approved: boolean;
}
export interface Job {
  id: string;
  name: string;
  agent_id: string;
  group_id: string | null;
  prompt: string;
  timezone: string;
  local_time: string;
  recurrence: string;
  next_at: string | null;
  paused: boolean;
}
export interface Execution {
  id: string;
  agent_id: string;
  conversation_id: string;
  group_id: string | null;
  job_id: string | null;
  chain_id: string;
  depth: number;
  prompt: string;
  status: string;
  created_at: string;
  updated_at: string;
  error: string | null;
}
export interface Approval {
  id: string;
  execution_id: string;
  kind: string;
  payload: unknown;
  status: string;
  created_at: string;
}
export interface Message {
  position: number;
  reply_count?: number;
  thread_id?: string | null;
  reply_to?: string | null;
  mentions?: string[];
  attachments?: string[];
  id: string;
  conversation_id: string;
  agent_id: string | null;
  execution_id: string | null;
  role: string;
  content: string;
  created_at: string;
}
export interface RunEvent {
  seq: number;
  execution_id: string;
  kind: string;
  data: Record<string, unknown>;
  created_at: string;
}
export interface Snapshot {
  notifications?: {
    id: string;
    message_id: string;
    conversation_id: string;
    content: string;
    read: boolean;
    status: string;
  }[];
  agents: Agent[];
  groups: Group[];
  integrations: Integration[];
  memories: Memory[];
  jobs: Job[];
  executions: Execution[];
  approvals: Approval[];
  occurrences: {
    id: string;
    job_id: string;
    at: string;
    status: string;
    error?: string;
  }[];
}
export const empty: Snapshot = {
  agents: [],
  groups: [],
  integrations: [],
  memories: [],
  jobs: [],
  executions: [],
  approvals: [],
  occurrences: [],
};
export const active = (s: string) =>
  ["queued", "running", "waiting_approval"].includes(s);
