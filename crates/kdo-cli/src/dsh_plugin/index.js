import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { promisify } from "node:util";

const exec = promisify(execFile);
export const name = "kdo";
export const inject = ["sessions", "sessionTitle", "connection"];
const CHANNEL = "/kdo";
const SOURCES = new Set(["claude", "codex", "opencode", "grok", "kdo"]);

function publicError(message) {
  return { ok: false, error: { code: "import_failed", message, details: { issues: [] } } };
}

function turnGroups(messages) {
  const groups = [];
  for (const message of messages) {
    if (message.role === "user") groups.push({ user: message.text, assistants: [] });
    else if (groups.length) groups.at(-1).assistants.push(message.text);
  }
  return groups;
}

function appendConversation(session, messages, source, sourceId) {
  let turn = 0;
  for (const group of turnGroups(messages)) {
    turn += 1;
    session.append("turn/start", { turn });
    session.append(
      "user/message",
      {
        id: randomUUID(),
        role: "user",
        content: [{ type: "text", text: group.user }],
        source: { kind: "plugin", plugin: "kdo", form: "recall" },
      },
      { surfaceOp: "append" },
    );
    if (group.assistants.length) {
      session.append("step/start", { turn, step: 1 });
      session.append(
        "assistant/message",
        {
          turn,
          step: 1,
          message: {
            id: randomUUID(),
            role: "assistant",
            content: [{ type: "text", text: group.assistants.join("\n\n") }],
            source: { kind: "model", provider: `imported-${source}`, model: `session-${sourceId}` },
          },
        },
        { surfaceOp: "append" },
      );
      session.append("step/end", { turn, step: 1 });
    }
    session.append("turn/end", { turn, reason: { kind: "completed" } });
  }
}

async function loadChat(source, sourceId) {
  if (source === "kdo") return JSON.parse(await readFile(sourceId, "utf8"));
  const { stdout } = await exec(
    "kdo",
    ["chats", "export", "--harness", source, "--id", sourceId],
    { maxBuffer: 32 * 1024 * 1024 },
  );
  return JSON.parse(stdout);
}

async function importChat(ctx, payload) {
  const source = payload?.source;
  const sourceId = payload?.sourceId?.trim();
  if (!SOURCES.has(source)) {
    throw new Error("Choose claude, codex, opencode, grok, or kdo");
  }
  if (!sourceId) throw new Error("Enter a session id, or a .kdo.json path");
  const doc = await loadChat(source, sourceId);
  const messages = Array.isArray(doc.messages) ? doc.messages : [];
  if (!messages.some((message) => message.role === "user")) {
    throw new Error("That chat has no visible user messages");
  }
  const title =
    doc.title ||
    messages.find((message) => message.role === "user")?.text?.replace(/\s+/g, " ").slice(0, 80) ||
    sourceId;
  const sessionId = `session-${randomUUID()}`;
  const session = ctx.sessions.create(sessionId, {
    meta: {
      cwd: doc.cwd || process.cwd(),
      createdAt: Date.now(),
      delegationDepth: 0,
      agentPreset: "standard",
    },
  });
  appendConversation(session, messages, source, sourceId);
  ctx.sessionTitle.rename(session, title);
  await ctx.sessions.flush(session);
  return { sessionId, title, messageCount: messages.length, source, sourceId };
}

export function apply(ctx) {
  const handler = async (endpoint, payload, signal) => {
    try {
      signal.throwIfAborted();
      if (endpoint !== "import") return publicError("Unknown kdo endpoint");
      const value = await importChat(ctx, payload);
      signal.throwIfAborted();
      return { ok: true, value };
    } catch (error) {
      if (signal.aborted) throw error;
      return publicError(error instanceof Error ? error.message : "kdo import failed");
    }
  };
  ctx.effect(
    () => ctx.connection.rpc.handle(CHANNEL, handler, { authority: "loopback" }),
    "kdo: loopback import RPC",
  );
}
