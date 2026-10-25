import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const store = JSON.parse(localStorage.getItem("fixture") || "null") || {
      agents: [],
      groups: [],
      integrations: [],
      memories: [],
      jobs: [],
      executions: [],
      approvals: [],
      occurrences: [],
      messages: [],
    };
    const persist = () =>
      localStorage.setItem("fixture", JSON.stringify(store));
    Object.assign(window, {
      isTauri: true,
      __TAURI_INTERNALS__: {
        metadata: { currentWindow: { label: "main" } },
        transformCallback: () => 1,
        unregisterCallback: () => {},
        invoke: async (cmd: string, args: Record<string, any>) => {
          if (cmd === "plugin:window|set_theme") {
            store.native_theme = args.value;
            persist();
            return null;
          }
          if (cmd === "plugin:event|listen") return 1;
          if (cmd === "plugin:event|unlisten") return null;
          if (cmd === "browser_profiles")
            return {
              profiles: store.browser_profiles || [],
              uploads: [],
              executable: "/fixture/chrome",
            };
          if (cmd === "save_browser_profile") {
            store.browser_profiles ||= [];
            const at = store.browser_profiles.findIndex(
              (p: any) => p.id === args.profile.id,
            );
            if (at < 0) store.browser_profiles.push(args.profile);
            else store.browser_profiles[at] = args.profile;
            persist();
            return null;
          }
          if (cmd === "browser_control") {
            store.browser_mode = args.mode;
            persist();
            return null;
          }
          if (cmd === "browser_artifacts") return [];
          if (cmd === "browser_status") return {};
          if (cmd === "snapshot") return structuredClone(store);
          if (cmd === "messages")
            return store.messages.filter(
              (m: any) => m.conversation_id === args.conversationId,
            );
          if (cmd === "execution_events")
            return [
              {
                seq: 1,
                execution_id: args.executionId,
                kind: "status",
                data: { status: "queued" },
                created_at: new Date().toISOString(),
              },
            ];
          if (cmd === "save_entity") {
            const plural: Record<string, string> = {
              agent: "agents",
              group: "groups",
              job: "jobs",
              integration: "integrations",
              memory: "memories",
            };
            const list = store[plural[args.kind]];
            const i = list.findIndex((v: any) => v.id === args.value.id);
            if (i < 0) list.push(args.value);
            else list[i] = args.value;
            persist();
            return null;
          }
          if (cmd === "send_message") {
            const id = crypto.randomUUID();
            const r = {
              ...args.input,
              id,
              conversation_id: args.input.group_id
                ? `group:${args.input.group_id}`
                : `agent:${args.input.agent_id}`,
              status: "queued",
              created_at: new Date().toISOString(),
              updated_at: new Date().toISOString(),
              chain_id: id,
              depth: 0,
              error: null,
            };
            store.executions.unshift(r);
            store.messages.push({
              id: crypto.randomUUID(),
              conversation_id: r.conversation_id,
              agent_id: null,
              execution_id: id,
              role: "user",
              content: args.input.prompt,
              created_at: r.created_at,
            });
            persist();
            return r;
          }
          if (cmd === "cancel_execution") {
            store.executions.find(
              (r: any) => r.id === args.executionId,
            ).status = "cancelled";
            persist();
            return null;
          }
          if (cmd === "resolve_approval") {
            store.approvals.find((a: any) => a.id === args.approvalId).status =
              args.allow ? "approved" : "rejected";
            persist();
            return null;
          }
          throw new Error(`Unmocked command: ${cmd}`);
        },
      },
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
    });
  });
  await page.goto("/");
});

test("create agent, review memory, persist configuration, send and cancel execution", async ({
  page,
}) => {
  await page.getByRole("button", { name: "◉ Agents" }).click();
  await page.getByRole("button", { name: "+ New agent" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name", { exact: true }).fill("Researcher");
  await dialog.getByLabel("Role", { exact: true }).fill("Find content ideas");
  await dialog
    .getByLabel("Instructions")
    .fill("Ask me before choosing a topic.");
  await dialog.getByRole("button", { name: "Save changes" }).click();
  await expect(dialog).toBeHidden();
  await page
    .locator(".team-card")
    .filter({
      has: page.getByRole("heading", { name: "Researcher", exact: true }),
    })
    .click();
  await page.getByRole("button", { name: "+ Add memory" }).click();
  await page.getByLabel("Lesson").fill("Use 4:5 images for our feed.");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText("Use 4:5 images for our feed.")).toBeVisible();
  await page
    .getByRole("textbox", { name: "Message", exact: true })
    .fill("Suggest a topic for today");
  await page.getByRole("button", { name: "Send ↗" }).click();
  await expect(
    page.getByRole("button", { name: "Cancel execution" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Cancel execution" }).click();
  await expect(page.locator(".inspector .badge")).toHaveText("cancelled");
  await page.reload();
  await page.getByRole("button", { name: "◉ Agents" }).click();
  await expect(
    page.getByRole("heading", { name: "Researcher", exact: true }),
  ).toBeVisible();
});

test("group membership and autonomy settings are saved", async ({ page }) => {
  await page.getByRole("button", { name: "+ New group" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name", { exact: true }).fill("Sira Production");
  await dialog
    .getByLabel("Purpose")
    .fill("Research, create, and publish influencer content.");
  await dialog.getByLabel("Allow automatic handoffs between members").uncheck();
  await dialog.getByLabel("Maximum handoffs per task chain").fill("8");
  await dialog.getByRole("button", { name: "Save changes" }).click();
  await page
    .locator(".team-card")
    .filter({
      has: page.getByRole("heading", { name: "Sira Production", exact: true }),
    })
    .click();
  await expect(page.getByText("Review each handoff")).toBeVisible();
  await page.getByRole("button", { name: "Pause group" }).click();
  await expect(
    page.getByRole("button", { name: "Resume group" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Configure" }).click();
  await expect(page.getByLabel("Maximum handoffs per task chain")).toHaveValue(
    "8",
  );
  await expect(
    page.getByLabel("Allow automatic handoffs between members"),
  ).not.toBeChecked();
});

test("browser preview renders without implying a backend connection", async ({
  context,
}) => {
  const page = await context.newPage();
  await page.goto("/");
  await expect(
    page.getByText("Browser preview only.", { exact: false }),
  ).toBeVisible();
  await page.screenshot({ path: "test-results/workspace.png", fullPage: true });
  await expect(
    page.getByRole("heading", { name: "Your agent teams" }),
  ).toBeVisible();
});

test("group conversation exposes handoffs and pending tool approval", async ({
  page,
}) => {
  await page.evaluate(() => {
    const id = crypto.randomUUID();
    const creator = crypto.randomUUID();
    const group = crypto.randomUUID();
    const run = crypto.randomUUID();
    const at = new Date().toISOString();
    localStorage.setItem(
      "fixture",
      JSON.stringify({
        agents: [
          {
            id,
            name: "Researcher",
            role: "Find the next idea",
            provider: "claude",
            integrations: [],
          },
          {
            id: creator,
            name: "Creator",
            role: "Generate media",
            provider: "codex",
            integrations: [],
          },
        ],
        groups: [
          {
            id: group,
            name: "Sira Production",
            purpose: "Create thoughtful content for our AI influencer.",
            members: [id, creator],
            automatic: true,
            paused: false,
            handoff_limit: 20,
          },
        ],
        memories: [],
        integrations: [],
        jobs: [],
        occurrences: [],
        executions: [
          {
            id: run,
            agent_id: creator,
            conversation_id: `group:${group}`,
            group_id: group,
            prompt: "Generate a 4:5 portrait using the approved concept.",
            status: "waiting_approval",
            created_at: at,
            updated_at: at,
          },
        ],
        messages: [
          {
            id: "m1",
            conversation_id: `group:${group}`,
            agent_id: id,
            execution_id: run,
            role: "handoff",
            content:
              "@Creator: Generate a 4:5 portrait using the approved concept.",
            created_at: at,
          },
        ],
        approvals: [
          {
            id: "approval",
            execution_id: run,
            kind: "tool",
            payload: {
              tool: "generate_image",
              arguments: { aspect_ratio: "4:5" },
            },
            status: "pending",
            created_at: at,
          },
        ],
      }),
    );
  });
  await page.reload();
  await page
    .locator(".team-card")
    .filter({
      has: page.getByRole("heading", { name: "Sira Production", exact: true }),
    })
    .click();
  await expect(
    page.getByText(
      "@Creator: Generate a 4:5 portrait using the approved concept.",
    ),
  ).toBeVisible();
  await page.getByRole("button", { name: "View execution ↗" }).click();
  await expect(page.locator(".inspector .badge")).toHaveText(
    "waiting approval",
  );
  await page.screenshot({
    path: "test-results/sira-conversation.png",
    fullPage: true,
  });
  await page.getByRole("button", { name: "Reject", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Waiting for your decision" }),
  ).toBeHidden();
});

test("dedicated browser profile and per-interaction policies survive reload", async ({
  page,
}) => {
  await page.getByRole("button", { name: "◉ Agents" }).click();
  await page.getByRole("button", { name: "+ New agent" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Name", { exact: true }).fill("Browser Publisher");
  await dialog
    .getByText("Browser interaction policies", { exact: true })
    .click();
  await expect(dialog.getByLabel("Browser click policy")).toHaveValue("ask");
  await dialog.getByLabel("Browser upload policy").selectOption("deny");
  await dialog.getByRole("button", { name: "Save changes" }).click();
  await page
    .getByRole("button", { name: "Settings", exact: false })
    .first()
    .click();
  await page.getByLabel("Profile name", { exact: true }).fill("Sira Instagram");
  await page
    .locator("form")
    .filter({ has: page.getByLabel("Profile name", { exact: true }) })
    .getByLabel("Profile agent", { exact: true })
    .selectOption({ label: "Browser Publisher" });
  await page.getByRole("button", { name: "Save browser profile" }).click();
  await expect(page.getByText("Sira Instagram", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Log in / take over" }).click();
  await page.getByRole("button", { name: "Resume automation" }).click();
  await page.reload();
  await page
    .getByRole("button", { name: "Settings", exact: false })
    .first()
    .click();
  await expect(page.getByText("Sira Instagram", { exact: true })).toBeVisible();
  const saved = await page.evaluate(() =>
    JSON.parse(localStorage.getItem("fixture")!),
  );
  expect(saved.agents[0].policies["browser:upload"]).toBe("deny");
  expect(saved.browser_mode).toBe("resume");
});

test("appearance supports light, dark, system and survives reload", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page
    .getByRole("button", { name: "Settings", exact: false })
    .first()
    .click();
  const theme = page.getByLabel("Color theme");
  await expect(theme).toHaveValue("system");
  await theme.selectOption("light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page
    .getByRole("button", { name: "Settings", exact: false })
    .first()
    .click();
  await page.getByLabel("Color theme").selectOption("dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.screenshot({
    path: "test-results/settings-dark.png",
    fullPage: true,
    animations: "disabled",
  });
  await page.getByLabel("Color theme").selectOption("system");
  await page.emulateMedia({ colorScheme: "light" });
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.screenshot({
    path: "test-results/settings-light.png",
    fullPage: true,
    animations: "disabled",
  });
});
