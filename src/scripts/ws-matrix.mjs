#!/usr/bin/env node

import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import { createConnection } from "node:net";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import {
  chmod,
  mkdir,
  readFile,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";

const SERVER_PORT = 12358;
const ROOT = path.resolve(import.meta.dirname, "..");
const REAL_HOME = homedir();
const MATRIX_TIMEOUT_MS = Number(process.env.VIBEAROUND_MATRIX_TIMEOUT_MS ?? 45_000);

const IMAGE_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
const IMAGE_ATTACHMENT = {
  uri: "https://example.test/matrix-image.png",
  name: "matrix-image.png",
  mimeType: "image/png",
  size: Buffer.byteLength(IMAGE_BASE64, "base64"),
};

const PROVIDER_TARGETS = [
  {
    provider: "xai",
    profile: "matrix-xai",
    label: "Matrix xAI",
    targets: [
      { api: "openai-responses", model: "grok-4.3", imageInput: true },
      { api: "openai-chat", model: "grok-4.3", imageInput: true },
    ],
  },
  {
    provider: "deepseek",
    profile: "matrix-deepseek",
    label: "Matrix DeepSeek",
    targets: [
      { api: "anthropic", model: "deepseek-v4-flash" },
      { api: "openai-chat", model: "deepseek-v4-flash" },
    ],
  },
  {
    provider: "minimax",
    profile: "matrix-minimax",
    label: "Matrix MiniMax",
    targets: [
      { api: "anthropic", model: "MiniMax-M2.7", endpointId: "global" },
      { api: "openai-chat", model: "MiniMax-M2.7", endpointId: "global" },
    ],
  },
  {
    provider: "dashscope",
    profile: "matrix-dashscope",
    label: "Matrix DashScope",
    targets: [
      { api: "anthropic", model: "qwen3.6-plus", endpointId: "coding-plan", imageInput: true },
      { api: "openai-chat", model: "qwen3.6-plus", endpointId: "coding-plan", imageInput: true },
    ],
  },
  {
    provider: "gemini",
    profile: "matrix-gemini",
    label: "Matrix Gemini",
    targets: [
      { api: "gemini", model: "gemini-2.5-flash", endpointId: "gemini-api" },
      { api: "openai-chat", model: "gemini-2.5-flash", endpointId: "gemini-api" },
    ],
  },
];

const CLIENT_ROUTES = [
  { agent: "codex", clientApi: "openai-responses" },
  { agent: "claude", clientApi: "anthropic" },
  { agent: "pi", clientApi: "openai-chat" },
  { agent: "pi", clientApi: "openai-responses" },
  { agent: "pi", clientApi: "anthropic" },
  { agent: "gemini", clientApi: "gemini" },
  { agent: "opencode", clientApi: "openai-chat" },
  { agent: "opencode", clientApi: "openai-responses" },
  { agent: "opencode", clientApi: "anthropic" },
];

const IMAGE_SUBMATRIX = [
  {
    agent: "codex",
    clientApi: "openai-responses",
    provider: "xai",
    targetApi: "openai-responses",
    imageMode: "supported",
  },
  {
    agent: "claude",
    clientApi: "anthropic",
    provider: "dashscope",
    targetApi: "anthropic",
    imageMode: "supported",
  },
  {
    agent: "pi",
    clientApi: "openai-chat",
    provider: "dashscope",
    targetApi: "openai-chat",
    imageMode: "supported",
  },
  {
    agent: "gemini",
    clientApi: "gemini",
    provider: "xai",
    targetApi: "openai-chat",
    imageMode: "supported",
  },
  {
    agent: "codex",
    clientApi: "openai-responses",
    provider: "deepseek",
    targetApi: "openai-chat",
    imageMode: "unsupported",
  },
  {
    agent: "claude",
    clientApi: "anthropic",
    provider: "deepseek",
    targetApi: "anthropic",
    imageMode: "unsupported",
  },
  {
    agent: "pi",
    clientApi: "openai-chat",
    provider: "minimax",
    targetApi: "openai-chat",
    imageMode: "unsupported",
  },
  {
    agent: "gemini",
    clientApi: "gemini",
    provider: "deepseek",
    targetApi: "openai-chat",
    imageMode: "unsupported",
  },
];

const LEGACY_CASE_ALIASES = new Map([
  ["codex|openai-responses|xai|openai-responses|", "codex-xai-responses"],
  ["codex|openai-responses|deepseek|openai-chat|", "codex-deepseek-chat"],
  ["claude|anthropic|minimax|anthropic|", "claude-minimax-anthropic"],
  ["claude|anthropic|deepseek|openai-chat|", "claude-deepseek-chat"],
  ["pi|openai-chat|dashscope|openai-chat|", "pi-dashscope-chat"],
  ["pi|openai-chat|gemini|gemini|", "pi-gemini-target"],
]);

const ALL_CASES = [...buildTextMatrixCases(), ...buildImageMatrixCases()];

const selectedNames = new Set(
  process.argv
    .filter((arg) => arg.startsWith("--case="))
    .map((arg) => arg.slice("--case=".length)),
);
const CASES = selectedNames.size
  ? ALL_CASES.filter((item) => {
      return selectedNames.has(item.name) || (item.aliases ?? []).some((alias) => selectedNames.has(alias));
    })
  : ALL_CASES;

if (CASES.length === 0) {
  throw new Error(`No matrix cases matched ${[...selectedNames].join(", ")}`);
}

function buildTextMatrixCases() {
  const cases = [];
  for (const providerDef of PROVIDER_TARGETS) {
    for (const target of providerDef.targets) {
      for (const route of CLIENT_ROUTES) {
        cases.push(matrixCase(route, providerDef, target));
      }
    }
  }
  return cases;
}

function buildImageMatrixCases() {
  return IMAGE_SUBMATRIX.map((entry) => {
    const providerDef = providerDefFor(entry.provider);
    const target = targetFor(providerDef, entry.targetApi);
    return matrixCase(
      { agent: entry.agent, clientApi: entry.clientApi },
      providerDef,
      target,
      { imageMode: entry.imageMode },
    );
  });
}

function matrixCase(route, providerDef, target, options = {}) {
  const imageSuffix = options.imageMode ? `-image-${options.imageMode}` : "";
  const name = [
    route.agent,
    apiSlug(route.clientApi),
    providerDef.provider,
    "to",
    apiSlug(target.api),
  ].join("-") + imageSuffix;
  const aliasKey = [
    route.agent,
    route.clientApi,
    providerDef.provider,
    target.api,
    options.imageMode ?? "",
  ].join("|");
  return {
    name,
    aliases: LEGACY_CASE_ALIASES.has(aliasKey) ? [LEGACY_CASE_ALIASES.get(aliasKey)] : [],
    provider: providerDef.provider,
    profile: providerDef.profile,
    agent: route.agent,
    clientApi: route.clientApi,
    targetApi: target.api,
    model: target.model,
    imageMode: options.imageMode,
  };
}

function providerDefFor(provider) {
  const providerDef = PROVIDER_TARGETS.find((candidate) => candidate.provider === provider);
  if (!providerDef) throw new Error(`Unknown matrix provider ${provider}`);
  return providerDef;
}

function targetFor(providerDef, api) {
  const target = providerDef.targets.find((candidate) => candidate.api === api);
  if (!target) throw new Error(`Provider ${providerDef.provider} has no target API ${api}`);
  return target;
}

function apiSlug(apiType) {
  switch (apiType) {
    case "openai-responses":
      return "responses";
    case "openai-chat":
      return "chat";
    case "anthropic":
      return "anthropic";
    case "gemini":
      return "gemini";
    default:
      return apiType.replaceAll(/[^a-z0-9]+/g, "-");
  }
}

async function main() {
  if (typeof WebSocket === "undefined") {
    throw new Error("This script requires a runtime with global WebSocket. Run it with Bun.");
  }

  if (await tcpAccepts("127.0.0.1", SERVER_PORT)) {
    throw new Error(
      `Port ${SERVER_PORT} is already in use. Stop the running VibeAround server before running ws:matrix.`,
    );
  }

  const tempRoot = await makeTempDir("vibearound-ws-matrix-");
  const home = path.join(tempRoot, "home");
  const workspace = path.join(home, ".vibearound", "workspaces", "matrix-project");
  const upstream = await startFakeUpstream();
  let server = null;

  try {
    await writeMatrixHome(home, workspace, upstream.url);
    await writeFakeAgents(home);

    server = startVibeAroundServer(home);
    const token = await waitForAuthToken(home);
    const baseUrl = `http://127.0.0.1:${SERVER_PORT}`;

    const results = [];
    for (const testCase of CASES) {
      results.push(await runCase({ testCase, token, workspace, baseUrl, upstream }));
    }

    console.log("\nwebsocket matrix passed");
    console.log(`cases=${results.length}`);
    for (const result of results) {
      console.log(
        `- ${result.name}: ${result.agent}/${result.clientApi} -> ${result.provider}/${result.targetApi}, upstream calls=${result.upstreamCalls}${result.imageMode ? `, image=${result.imageMode}` : ""}`,
      );
    }
  } finally {
    if (server) {
      server.kill("SIGTERM");
      await waitForExit(server, 3_000).catch(() => server.kill("SIGKILL"));
    }
    await upstream.close();
    if (!process.env.VIBEAROUND_MATRIX_KEEP_HOME) {
      await rm(tempRoot, { recursive: true, force: true });
    } else {
      console.log(`kept matrix HOME at ${home}`);
    }
  }
}

async function runCase({ testCase, token, workspace, baseUrl, upstream }) {
  const beforeCalls = upstream.requests.length;
  const ws = await openChatSocket(token);
  const summary = {
    name: testCase.name,
    agent: testCase.agent,
    provider: testCase.provider,
    clientApi: testCase.clientApi,
    targetApi: testCase.targetApi,
    imageMode: testCase.imageMode,
    upstreamCalls: 0,
  };

  try {
    await waitForEvent(ws, (event) => event.kind === "config", "config");

    const turn1 = await sendMatrixTurn(ws, testCase, workspace, baseUrl, 1, true);
    assertTurn(testCase, turn1, 1, true);

    const turn2 = await sendMatrixTurn(ws, testCase, workspace, baseUrl, 2, false);
    assertTurn(testCase, turn2, 2, false);

    const afterCalls = upstream.requests.length;
    summary.upstreamCalls = afterCalls - beforeCalls;
    if (summary.upstreamCalls < 3) {
      throw new Error(`${testCase.name}: expected at least 3 upstream bridge calls`);
    }
    const targetCalls = upstream.requests.slice(beforeCalls).filter((request) => {
      return request.targetApi === testCase.targetApi;
    });
    if (targetCalls.length !== summary.upstreamCalls) {
      throw new Error(`${testCase.name}: upstream target protocol mismatch`);
    }
    const imageCalls = targetCalls.filter((request) => request.hasImage);
    if (testCase.imageMode === "supported" && imageCalls.length === 0) {
      throw new Error(`${testCase.name}: expected image content to reach upstream`);
    }
    if (testCase.imageMode === "unsupported" && imageCalls.length > 0) {
      throw new Error(`${testCase.name}: unsupported image request reached upstream`);
    }
    if (
      testCase.imageMode === "unsupported" &&
      !targetCalls.some((request) => request.hasOmittedMedia)
    ) {
      throw new Error(`${testCase.name}: expected unsupported image to be sanitized before upstream`);
    }
    return summary;
  } finally {
    ws.close();
  }
}

async function sendMatrixTurn(ws, testCase, workspace, baseUrl, turn, newSession) {
  const payload = {
    caseName: testCase.name,
    agent: testCase.agent,
    provider: testCase.provider,
    profile: testCase.profile,
    clientApi: testCase.clientApi,
    targetApi: testCase.targetApi,
    model: testCase.model,
    baseUrl,
    turn,
  };
  if (testCase.imageMode) payload.imageMode = testCase.imageMode;
  const message = {
    type: "message",
    messageId: randomUUID(),
    text: JSON.stringify(payload),
    sessionWorkspace: workspace,
  };
  if (testCase.imageMode && turn === 1) {
    message.attachments = [IMAGE_ATTACHMENT];
  }
  if (newSession) {
    message.agent = testCase.agent;
    message.profileId = testCase.profile;
    message.sessionAction = "new";
  }
  ws.send(JSON.stringify(message));

  const seen = {
    promptDone: false,
    okText: false,
    toolCall: false,
    toolUpdate: false,
    sessionReady: false,
    agentReady: false,
    attachment: false,
    imageSanitized: false,
    events: [],
  };
  const startedAt = Date.now();
  while (Date.now() - startedAt < MATRIX_TIMEOUT_MS) {
    const event = await ws.next(MATRIX_TIMEOUT_MS);
    seen.events.push(event);
    if (event.kind === "error") {
      throw new Error(`${testCase.name}: websocket error: ${event.error}`);
    }
    if (event.kind === "system_text" && event.text.includes("❌")) {
      throw new Error(`${testCase.name}: system error: ${event.text}`);
    }
    if (event.kind === "agent_ready") seen.agentReady = true;
    if (event.kind === "session_ready") seen.sessionReady = true;
    if (event.kind === "prompt_done") seen.promptDone = true;
    if (event.kind === "acp_notification") {
      const update = event.payload?.update;
      if (update?.sessionUpdate === "tool_call") seen.toolCall = true;
      if (update?.sessionUpdate === "tool_call_update") seen.toolUpdate = true;
      const text = update?.content?.text;
      if (
        update?.sessionUpdate === "agent_message_chunk" &&
        typeof text === "string" &&
        text.includes(`MATRIX_OK ${testCase.name} turn=${turn}`)
      ) {
        seen.okText = true;
      }
      if (
        update?.sessionUpdate === "agent_message_chunk" &&
        typeof text === "string" &&
        text.includes(`MATRIX_ATTACHMENT_OK ${testCase.name}`)
      ) {
        seen.attachment = true;
      }
      if (
        update?.sessionUpdate === "agent_message_chunk" &&
        typeof text === "string" &&
        text.includes(`MATRIX_IMAGE_SANITIZED ${testCase.name}`)
      ) {
        seen.imageSanitized = true;
      }
    }
    if (seen.promptDone && seen.okText) return seen;
  }
  throw new Error(`${testCase.name}: timed out waiting for turn ${turn}`);
}

function assertTurn(testCase, seen, turn, needsTool) {
  if (turn === 1 && !seen.agentReady) {
    throw new Error(`${testCase.name}: missing agent_ready on first turn`);
  }
  if (turn === 1 && !seen.sessionReady) {
    throw new Error(`${testCase.name}: missing session_ready on first turn`);
  }
  if (needsTool && (!seen.toolCall || !seen.toolUpdate)) {
    throw new Error(`${testCase.name}: missing tool call/update on turn ${turn}`);
  }
  if (turn === 1 && testCase.imageMode && !seen.attachment) {
    throw new Error(`${testCase.name}: image attachment did not reach agent prompt`);
  }
  if (turn === 1 && testCase.imageMode === "unsupported" && !seen.imageSanitized) {
    throw new Error(`${testCase.name}: missing unsupported-image sanitization marker`);
  }
  if (!seen.okText || !seen.promptDone) {
    throw new Error(`${testCase.name}: incomplete websocket turn ${turn}`);
  }
}

async function openChatSocket(token) {
  const url = `ws://127.0.0.1:${SERVER_PORT}/va/ws/chat?token=${encodeURIComponent(token)}`;
  const ws = new WebSocket(url);
  const queue = [];
  const waiters = [];
  let closed = false;
  let closeError = null;

  ws.addEventListener("message", (message) => {
    const event = JSON.parse(String(message.data));
    if (process.env.VIBEAROUND_MATRIX_TRACE) {
      console.log("ws", JSON.stringify(event));
    }
    const waiter = waiters.shift();
    if (waiter) waiter.resolve(event);
    else queue.push(event);
  });
  ws.addEventListener("close", () => {
    closed = true;
    closeError = closeError ?? new Error("websocket closed");
    while (waiters.length) waiters.shift().reject(closeError);
  });
  ws.addEventListener("error", () => {
    closeError = new Error("websocket error");
  });

  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("websocket open timeout")), 10_000);
    ws.addEventListener("open", () => {
      clearTimeout(timer);
      resolve();
    });
    ws.addEventListener("error", () => {
      clearTimeout(timer);
      reject(new Error("websocket open failed"));
    });
  });

  ws.next = (timeoutMs) => {
    if (queue.length) return Promise.resolve(queue.shift());
    if (closed) return Promise.reject(closeError);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const index = waiters.findIndex((waiter) => waiter.resolve === resolve);
        if (index >= 0) waiters.splice(index, 1);
        reject(new Error("websocket event timeout"));
      }, timeoutMs);
      waiters.push({
        resolve: (event) => {
          clearTimeout(timer);
          resolve(event);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
    });
  };
  return ws;
}

async function waitForEvent(ws, predicate, label) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < MATRIX_TIMEOUT_MS) {
    const event = await ws.next(MATRIX_TIMEOUT_MS);
    if (predicate(event)) return event;
    if (event.kind === "error") throw new Error(`waiting for ${label}: ${event.error}`);
  }
  throw new Error(`timed out waiting for ${label}`);
}

async function startFakeUpstream() {
  const requests = [];
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const rawBody = Buffer.concat(chunks).toString("utf8");
    const body = rawBody ? JSON.parse(rawBody) : {};
    const targetApi = targetApiFromUrl(req.url ?? "");
    requests.push({
      method: req.method,
      url: req.url,
      targetApi,
      body,
      headers: req.headers,
      hasImage: bodyHasImage(body),
      hasOmittedMedia: bodyHasOmittedMedia(body),
    });

    try {
      const response = upstreamResponse(targetApi, body);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(response));
    } catch (error) {
      res.writeHead(500, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: { message: String(error?.message ?? error) } }));
    }
  });

  await new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", resolve);
    server.on("error", reject);
  });
  const address = server.address();
  return {
    url: `http://127.0.0.1:${address.port}`,
    requests,
    close: () =>
      new Promise((resolve) => {
        server.close(resolve);
      }),
  };
}

function bodyHasImage(value) {
  if (Array.isArray(value)) return value.some(bodyHasImage);
  if (!value || typeof value !== "object") {
    return typeof value === "string" && value.startsWith("data:image/");
  }
  const type = stringField(value, "type");
  if (type === "input_image" || type === "image" || type === "image_url") return true;
  if (value.image_url || value.imageUrl || value.inlineData || value.inline_data) return true;
  const mimeType = stringField(value, "mimeType") || stringField(value, "mime_type");
  if (mimeType?.startsWith("image/")) return true;
  return Object.values(value).some(bodyHasImage);
}

function bodyHasOmittedMedia(value) {
  if (Array.isArray(value)) return value.some(bodyHasOmittedMedia);
  if (!value || typeof value !== "object") {
    return (
      typeof value === "string" &&
      (value.includes("Image attachment omitted") ||
        value.includes("File attachment omitted") ||
        value.includes("Attachment omitted"))
    );
  }
  return Object.values(value).some(bodyHasOmittedMedia);
}

function stringField(value, key) {
  const field = value?.[key];
  return typeof field === "string" ? field : null;
}

function targetApiFromUrl(url) {
  if (url.includes("/chat/completions")) return "openai-chat";
  if (url.includes("/responses")) return "openai-responses";
  if (url.includes("/messages")) return "anthropic";
  if (url.includes(":generateContent")) return "gemini";
  return "unknown";
}

function upstreamResponse(targetApi, body) {
  switch (targetApi) {
    case "openai-chat":
      return openAiChatResponse(body);
    case "openai-responses":
      return openAiResponsesResponse(body);
    case "anthropic":
      return anthropicResponse(body);
    case "gemini":
      return geminiResponse(body);
    default:
      throw new Error(`unsupported upstream URL target ${targetApi}`);
  }
}

function openAiChatResponse(body) {
  const model = body.model ?? "matrix-openai-chat";
  const hasResult = (body.messages ?? []).some((message) => message.role === "tool");
  if (!hasResult) {
    return {
      id: "chatcmpl_matrix_tool",
      object: "chat.completion",
      created: 0,
      model,
      choices: [
        {
          index: 0,
          message: {
            role: "assistant",
            content: null,
            tool_calls: [
              {
                id: "call_lookup",
                type: "function",
                function: {
                  name: "lookup",
                  arguments: JSON.stringify({ query: "matrix first turn" }),
                },
              },
            ],
          },
          finish_reason: "tool_calls",
        },
      ],
      usage: usage(),
    };
  }
  return {
    id: "chatcmpl_matrix_final",
    object: "chat.completion",
    created: 0,
    model,
    choices: [
      {
        index: 0,
        message: {
          role: "assistant",
          content: `final via openai-chat (${model})`,
        },
        finish_reason: "stop",
      },
    ],
    usage: usage(),
  };
}

function openAiResponsesResponse(body) {
  const model = body.model ?? "matrix-openai-responses";
  const input = Array.isArray(body.input) ? body.input : [];
  const hasResult = input.some((item) => item?.type === "function_call_output");
  if (!hasResult) {
    return {
      id: "resp_matrix_tool",
      object: "response",
      created_at: 0,
      status: "completed",
      model,
      output: [
        {
          type: "function_call",
          id: "fc_lookup",
          call_id: "call_lookup",
          name: "lookup",
          arguments: JSON.stringify({ query: "matrix first turn" }),
          status: "completed",
        },
      ],
      usage: responseUsage(),
    };
  }
  return {
    id: "resp_matrix_final",
    object: "response",
    created_at: 0,
    status: "completed",
    model,
    output: [
      {
        type: "message",
        id: "msg_matrix_final",
        status: "completed",
        role: "assistant",
        content: [
          {
            type: "output_text",
            text: `final via openai-responses (${model})`,
            annotations: [],
          },
        ],
      },
    ],
    usage: responseUsage(),
  };
}

function anthropicResponse(body) {
  const model = body.model ?? "matrix-anthropic";
  const hasResult = (body.messages ?? []).some((message) => {
    const content = Array.isArray(message.content) ? message.content : [];
    return content.some((part) => part?.type === "tool_result");
  });
  if (!hasResult) {
    return {
      id: "msg_matrix_tool",
      type: "message",
      role: "assistant",
      model,
      content: [
        {
          type: "tool_use",
          id: "call_lookup",
          name: "lookup",
          input: { query: "matrix first turn" },
        },
      ],
      stop_reason: "tool_use",
      usage: { input_tokens: 8, output_tokens: 4 },
    };
  }
  return {
    id: "msg_matrix_final",
    type: "message",
    role: "assistant",
    model,
    content: [{ type: "text", text: `final via anthropic (${model})` }],
    stop_reason: "end_turn",
    usage: { input_tokens: 8, output_tokens: 4 },
  };
}

function geminiResponse(body) {
  const model = body.__va_model ?? body.model ?? "matrix-gemini";
  const hasResult = (body.contents ?? []).some((content) => {
    return (content.parts ?? []).some((part) => part?.functionResponse);
  });
  if (!hasResult) {
    return {
      candidates: [
        {
          content: {
            role: "model",
            parts: [
              {
                functionCall: {
                  name: "lookup",
                  args: { query: "matrix first turn" },
                },
              },
            ],
          },
          finishReason: "STOP",
        },
      ],
      usageMetadata: geminiUsage(),
      modelVersion: model,
    };
  }
  return {
    candidates: [
      {
        content: {
          role: "model",
          parts: [{ text: `final via gemini (${model})` }],
        },
        finishReason: "STOP",
      },
    ],
    usageMetadata: geminiUsage(),
    modelVersion: model,
  };
}

function usage() {
  return { prompt_tokens: 8, completion_tokens: 4, total_tokens: 12 };
}

function responseUsage() {
  return { input_tokens: 8, output_tokens: 4, total_tokens: 12 };
}

function geminiUsage() {
  return {
    promptTokenCount: 8,
    candidatesTokenCount: 4,
    totalTokenCount: 12,
  };
}

async function writeMatrixHome(home, workspace, upstreamUrl) {
  const dataDir = path.join(home, ".vibearound");
  const profilesDir = path.join(dataDir, "profiles");
  await mkdir(profilesDir, { recursive: true });
  await mkdir(workspace, { recursive: true });
  await writeJson(path.join(dataDir, "settings.json"), {
    workspaces: [workspace],
    default_agent: "codex",
    enabled_agents: ["codex", "claude", "pi", "gemini", "opencode"],
    integrations: {
      mcp_auto_install: false,
      skill_auto_install: false,
    },
  });
  await writeJson(path.join(dataDir, "agents.json"), {
    profileConnections: Object.fromEntries(
      PROVIDER_TARGETS.map((providerDef) => [providerDef.profile, launchPreferences(providerDef)]),
    ),
  });

  const profiles = PROVIDER_TARGETS.map((providerDef) => {
    const apiTypes = providerDef.targets.map((target) => target.api);
    const models = Object.fromEntries(
      providerDef.targets.map((target) => [target.api, target.model]),
    );
    const endpointIds = Object.fromEntries(
      providerDef.targets
        .filter((target) => target.endpointId)
        .map((target) => [target.api, target.endpointId]),
    );
    return profile(
      providerDef.profile,
      providerDef.label,
      providerDef.provider,
      apiTypes,
      upstreamUrl,
      models,
      endpointIds,
    );
  });

  for (const item of profiles) {
    await writeJson(path.join(profilesDir, `${item.id}.json`), item);
  }
}

function launchPreferences(providerDef) {
  const preferredTarget = providerDef.targets[0];
  return {
    codex: bridgePreference("openai-responses", preferredTarget.api, preferredTarget.model),
    claude: bridgePreference("anthropic", preferredTarget.api, preferredTarget.model),
    pi: bridgePreference("openai-chat", preferredTarget.api, preferredTarget.model),
    gemini: bridgePreference("gemini", preferredTarget.api, preferredTarget.model),
    opencode: bridgePreference("openai-chat", preferredTarget.api, preferredTarget.model),
  };
}

function bridgePreference(clientApi, targetApi, model) {
  return {
    selectedApiType: clientApi,
    bridge: {
      [clientApi]: {
        enabled: true,
        targetApiType: targetApi,
        upstreamModel: model,
      },
    },
  };
}

function profile(id, label, provider, apiTypes, baseUrl, models, endpointIds = {}) {
  const overrides = {};
  for (const apiType of apiTypes) {
    overrides[apiType] = {
      base_url: baseUrl,
      model: models[apiType],
    };
    if (endpointIds[apiType]) overrides[apiType].endpoint_id = endpointIds[apiType];
  }
  return {
    id,
    label,
    provider,
    auth_mode: "api_key",
    api_types: apiTypes,
    credentials: { api_key: "matrix-test-key" },
    overrides,
  };
}

async function writeFakeAgents(home) {
  const plugins = path.join(home, ".vibearound", "plugins");
  const nodeModules = path.join(plugins, "node_modules");
  const bin = path.join(nodeModules, ".bin");
  const fakeAgent = path.join(plugins, "fake-acp-agent.cjs");
  await mkdir(bin, { recursive: true });
  await writeFile(fakeAgent, FAKE_AGENT_SOURCE, "utf8");
  await chmod(fakeAgent, 0o755);

  await linkBin(bin, "codex-acp", fakeAgent);
  await linkBin(bin, "claude-agent-acp", fakeAgent);
  await linkBin(bin, "pi-acp", fakeAgent);
  await linkBin(bin, "gemini", fakeAgent);
  await linkBin(bin, "opencode", fakeAgent);

  await writePackage(nodeModules, "@zed-industries/codex-acp", "0.14.0");
  await writePackage(nodeModules, "pi-acp", "0.0.27");
  await writePackage(nodeModules, "@agentclientprotocol/claude-agent-acp", "0.0.0");
}

async function linkBin(bin, name, target) {
  const link = path.join(bin, name);
  await rm(link, { force: true });
  await symlink(target, link);
}

async function writePackage(nodeModules, name, version) {
  const dir = name.split("/").reduce((current, part) => path.join(current, part), nodeModules);
  await mkdir(dir, { recursive: true });
  await writeJson(path.join(dir, "package.json"), { name, version });
}

function startVibeAroundServer(home) {
  const fakeBin = path.join(home, ".vibearound", "plugins", "node_modules", ".bin");
  const child = spawn("cargo", ["run", "-p", "server"], {
    cwd: ROOT,
    env: {
      ...process.env,
      HOME: home,
      USERPROFILE: home,
      PATH: `${fakeBin}${path.delimiter}${process.env.PATH ?? ""}`,
      CARGO_HOME: process.env.CARGO_HOME ?? path.join(REAL_HOME, ".cargo"),
      RUSTUP_HOME: process.env.RUSTUP_HOME ?? path.join(REAL_HOME, ".rustup"),
      VIBEAROUND_MATRIX_BASE_URL: `http://127.0.0.1:${SERVER_PORT}`,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    if (process.env.VIBEAROUND_MATRIX_VERBOSE) process.stdout.write(chunk);
  });
  child.stderr.on("data", (chunk) => {
    if (process.env.VIBEAROUND_MATRIX_VERBOSE) process.stderr.write(chunk);
  });
  child.on("exit", (code, signal) => {
    if (code && code !== 0 && !child.killed) {
      console.error(`vibearound-server exited with code ${code} signal ${signal ?? ""}`);
    }
  });
  return child;
}

async function waitForAuthToken(home) {
  const authPath = path.join(home, ".vibearound", "auth.json");
  const deadline = Date.now() + MATRIX_TIMEOUT_MS;
  while (Date.now() < deadline) {
    try {
      const raw = await readFile(authPath, "utf8");
      const parsed = JSON.parse(raw);
      if (parsed.port === SERVER_PORT && parsed.token) return parsed.token;
    } catch {
      // keep polling
    }
    if (await tcpAccepts("127.0.0.1", SERVER_PORT)) {
      await sleep(200);
    } else {
      await sleep(500);
    }
  }
  throw new Error("Timed out waiting for VibeAround auth token");
}

async function makeTempDir(prefix) {
  const dir = path.join(tmpdir(), `${prefix}${process.pid}-${Date.now()}`);
  await mkdir(dir, { recursive: true });
  return dir;
}

async function writeJson(file, value) {
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function tcpAccepts(host, port) {
  return new Promise((resolve) => {
    const socket = createConnection({ host, port, timeout: 500 });
    socket.on("connect", () => {
      socket.destroy();
      resolve(true);
    });
    socket.on("timeout", () => {
      socket.destroy();
      resolve(false);
    });
    socket.on("error", () => resolve(false));
  });
}

function waitForExit(child, timeoutMs) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("process exit timeout")), timeoutMs);
    child.on("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

const FAKE_AGENT_SOURCE = String.raw`#!/usr/bin/env node
const readline = require("node:readline");
const { randomUUID } = require("node:crypto");

const agent = process.env.VIBEAROUND_LAUNCH_TARGET || "matrix-agent";
const sessions = new Map();
const IMAGE_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
const IMAGE_DATA_URL = "data:image/png;base64," + IMAGE_BASE64;

const rl = readline.createInterface({ input: process.stdin });
rl.on("line", (line) => {
  handleLine(line).catch((error) => {
    write({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: [...sessions.keys()][0] || "matrix-session",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: {
            type: "text",
            text: "MATRIX_ERROR " + (error && error.stack ? error.stack : String(error)),
          },
        },
      },
    });
  });
});

async function handleLine(line) {
  if (!line.trim()) return;
  const message = JSON.parse(line);
  const id = message.id;
  switch (message.method) {
    case "initialize":
      result(id, {
        protocolVersion: message.params?.protocolVersion ?? 1,
        agentCapabilities: {},
        agentInfo: {
          name: agent,
          title: "Matrix " + agent,
          version: "0.0.1",
        },
      });
      break;
    case "session/new": {
      const sessionId = "matrix-" + agent + "-" + randomUUID();
      sessions.set(sessionId, { history: {} });
      result(id, { sessionId });
      break;
    }
    case "session/load":
    case "session/resume": {
      const sessionId = message.params?.sessionId || "matrix-" + agent + "-" + randomUUID();
      if (!sessions.has(sessionId)) sessions.set(sessionId, { history: {} });
      result(id, { sessionId });
      break;
    }
    case "session/prompt":
      await handlePrompt(id, message.params);
      break;
    case "session/cancel":
      break;
    default:
      result(id, {});
      break;
  }
}

async function handlePrompt(id, params) {
  const sessionId = params.sessionId;
  const session = sessions.get(sessionId) || { history: {} };
  sessions.set(sessionId, session);
  const prompt = params.prompt || [];
  const text = prompt.map((block) => block.text || "").find((item) => item.trim().startsWith("{"));
  if (!text) throw new Error("matrix prompt did not include JSON payload");
  const payload = JSON.parse(text);
  const attachments = prompt.filter(isResourceLink);

  notifyText(sessionId, "MATRIX_START " + payload.caseName + " turn=" + payload.turn);
  if (payload.imageMode && payload.turn === 1) {
    if (!attachments.some(isImageAttachment)) {
      throw new Error("matrix image case did not receive an image attachment");
    }
    notifyText(sessionId, "MATRIX_ATTACHMENT_OK " + payload.caseName);
  }
  const bridge = new BridgeConversation(payload, session, attachments);
  const outcome = await bridge.run();

  if (outcome.imageSanitized) {
    notifyText(sessionId, "MATRIX_IMAGE_SANITIZED " + payload.caseName);
  }
  if (outcome.tool) {
    notifyTool(sessionId, outcome.tool);
  }
  notifyText(
    sessionId,
    "MATRIX_OK " +
      payload.caseName +
      " turn=" +
      payload.turn +
      " client=" +
      payload.clientApi +
      " target=" +
      payload.targetApi +
      " final=" +
      outcome.finalText,
  );
  result(id, { stopReason: "end_turn" });
}

function isResourceLink(block) {
  if (!block || typeof block !== "object") return false;
  const text = JSON.stringify(block).toLowerCase();
  return text.includes("resource_link") || text.includes("resourcelink") || text.includes("uri");
}

function isImageAttachment(block) {
  if (!block || typeof block !== "object") return false;
  const text = JSON.stringify(block).toLowerCase();
  return text.includes("image/") || text.includes(".png") || text.includes(".jpg") || text.includes(".jpeg");
}

class BridgeConversation {
  constructor(payload, session, attachments) {
    this.payload = payload;
    this.session = session;
    this.attachments = attachments;
    this.model = payload.model || "matrix-model";
  }

  async run() {
    if (this.payload.turn === 1) {
      const imageSanitized = this.payload.imageMode === "unsupported";
      const first = await this.call(this.firstRequest({ image: Boolean(this.payload.imageMode) }));
      const tool = parseTool(this.payload.clientApi, first);
      if (!tool) throw new Error("bridge response did not include a tool call");
      const toolOutput = { ok: true, provider: this.payload.provider, targetApi: this.payload.targetApi };
      const includeImage = Boolean(this.payload.imageMode);
      const final = await this.call(this.toolResultRequest(tool, toolOutput, { image: includeImage }));
      const finalText = parseFinalText(this.payload.clientApi, final);
      if (!finalText) throw new Error("bridge tool-result response did not include final text");
      this.session.history[this.payload.caseName] = { tool, toolOutput, image: includeImage };
      return { tool, finalText, imageSanitized };
    }

    const final = await this.call(this.followupRequest());
    const finalText = parseFinalText(this.payload.clientApi, final);
    if (!finalText) throw new Error("bridge follow-up response did not include final text");
    return { tool: null, finalText };
  }

  async call(body) {
    const result = await this.callAllowError(body);
    if (!result.ok) {
      throw new Error("bridge HTTP " + result.status + ": " + JSON.stringify(result.json));
    }
    return result.json;
  }

  async callAllowError(body) {
    const response = await fetch(this.url(), {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: "Bearer matrix-test-key",
        "x-api-key": "matrix-test-key",
        "anthropic-version": "2023-06-01",
      },
      body: JSON.stringify(body),
    });
    const text = await response.text();
    let json = {};
    try {
      json = text ? JSON.parse(text) : {};
    } catch (error) {
      throw new Error("bridge returned invalid JSON: " + text);
    }
    return { ok: response.ok, status: response.status, json };
  }

  url() {
    const scope = this.payload.agent + "-" + this.payload.clientApi;
    const base =
      this.payload.baseUrl +
      "/va/local-api/" +
      this.payload.profile +
      "/" +
      scope +
      "/" +
      this.payload.targetApi;
    switch (this.payload.clientApi) {
      case "openai-responses":
        return base + "/v1/responses";
      case "openai-chat":
        return base + "/v1/chat/completions";
      case "anthropic":
        return base + "/v1/messages";
      case "gemini":
        return base + "/v1beta/models/" + encodeURIComponent(this.model) + ":generateContent";
      default:
        throw new Error("unsupported client API " + this.payload.clientApi);
    }
  }

  firstRequest(options = {}) {
    const text = options.text || (options.image ? "matrix image tool request" : "matrix tool request");
    switch (this.payload.clientApi) {
      case "openai-responses":
        return {
          model: this.model,
          input: [{ role: "user", content: openAiResponsesContent(text, options.image) }],
          tools: [openAiResponsesTool()],
          tool_choice: { type: "function", name: "lookup" },
        };
      case "openai-chat":
        return {
          model: this.model,
          messages: [{ role: "user", content: openAiChatContent(text, options.image) }],
          tools: [openAiChatTool()],
          tool_choice: { type: "function", function: { name: "lookup" } },
        };
      case "anthropic":
        return {
          model: this.model,
          max_tokens: 256,
          messages: [{ role: "user", content: anthropicContent(text, options.image) }],
          tools: [anthropicTool()],
          tool_choice: { type: "tool", name: "lookup" },
        };
      case "gemini":
        return {
          model: this.model,
          contents: [{ role: "user", parts: geminiParts(text, options.image) }],
          tools: [geminiTool()],
          tool_config: {
            function_calling_config: {
              mode: "ANY",
              allowed_function_names: ["lookup"],
            },
          },
        };
      default:
        throw new Error("unsupported client API " + this.payload.clientApi);
    }
  }

  toolResultRequest(tool, toolOutput, options = {}) {
    const output = JSON.stringify(toolOutput);
    const text = options.image ? "matrix image tool request" : "matrix tool request";
    switch (this.payload.clientApi) {
      case "openai-responses":
        return {
          model: this.model,
          input: [
            { role: "user", content: openAiResponsesContent(text, options.image) },
            {
              type: "function_call",
              id: tool.responseId || "fc_lookup",
              call_id: tool.callId,
              name: tool.name,
              arguments: JSON.stringify(tool.arguments || {}),
            },
            { type: "function_call_output", call_id: tool.callId, output },
          ],
          tools: [openAiResponsesTool()],
        };
      case "openai-chat":
        return {
          model: this.model,
          messages: [
            { role: "user", content: openAiChatContent(text, options.image) },
            {
              role: "assistant",
              content: null,
              tool_calls: [
                {
                  id: tool.callId,
                  type: "function",
                  function: { name: tool.name, arguments: JSON.stringify(tool.arguments || {}) },
                },
              ],
            },
            { role: "tool", tool_call_id: tool.callId, content: output },
          ],
          tools: [openAiChatTool()],
        };
      case "anthropic":
        return {
          model: this.model,
          max_tokens: 256,
          messages: [
            { role: "user", content: anthropicContent(text, options.image) },
            {
              role: "assistant",
              content: [{ type: "tool_use", id: tool.callId, name: tool.name, input: tool.arguments || {} }],
            },
            {
              role: "user",
              content: [{ type: "tool_result", tool_use_id: tool.callId, content: output }],
            },
          ],
          tools: [anthropicTool()],
        };
      case "gemini":
        return {
          model: this.model,
          contents: [
            { role: "user", parts: geminiParts(text, options.image) },
            {
              role: "model",
              parts: [{ functionCall: { name: tool.name, args: tool.arguments || {} } }],
            },
            {
              role: "user",
              parts: [{ functionResponse: { name: tool.name, response: toolOutput } }],
            },
          ],
          tools: [geminiTool()],
        };
      default:
        throw new Error("unsupported client API " + this.payload.clientApi);
    }
  }

  followupRequest() {
    const history = this.session.history[this.payload.caseName];
    if (!history) throw new Error("missing stored history for follow-up");
    const base = this.toolResultRequest(history.tool, history.toolOutput, { image: history.image });
    switch (this.payload.clientApi) {
      case "openai-responses":
        base.input.push({ role: "user", content: [{ type: "input_text", text: "matrix follow up" }] });
        break;
      case "openai-chat":
        base.messages.push({ role: "user", content: "matrix follow up" });
        break;
      case "anthropic":
        base.messages.push({ role: "user", content: "matrix follow up" });
        break;
      case "gemini":
        base.contents.push({ role: "user", parts: [{ text: "matrix follow up" }] });
        break;
    }
    return base;
  }
}

function openAiResponsesContent(text, image) {
  const content = [{ type: "input_text", text }];
  if (image) content.push({ type: "input_image", image_url: IMAGE_DATA_URL });
  return content;
}

function openAiChatContent(text, image) {
  if (!image) return text;
  return [
    { type: "text", text },
    { type: "image_url", image_url: { url: IMAGE_DATA_URL } },
  ];
}

function anthropicContent(text, image) {
  if (!image) return text;
  return [
    { type: "text", text },
    {
      type: "image",
      source: {
        type: "base64",
        media_type: "image/png",
        data: IMAGE_BASE64,
      },
    },
  ];
}

function geminiParts(text, image) {
  const parts = [{ text }];
  if (image) {
    parts.push({
      inlineData: {
        mimeType: "image/png",
        data: IMAGE_BASE64,
      },
    });
  }
  return parts;
}

function openAiChatTool() {
  return { type: "function", function: toolDefinition("parameters") };
}

function openAiResponsesTool() {
  return { type: "function", ...toolDefinition("parameters") };
}

function anthropicTool() {
  return { name: "lookup", description: "lookup matrix data", input_schema: toolSchema() };
}

function geminiTool() {
  return {
    function_declarations: [
      { name: "lookup", description: "lookup matrix data", parameters: toolSchema() },
    ],
  };
}

function toolDefinition(schemaKey) {
  return {
    name: "lookup",
    description: "lookup matrix data",
    [schemaKey]: toolSchema(),
  };
}

function toolSchema() {
  return {
    type: "object",
    properties: { query: { type: "string" } },
    required: ["query"],
  };
}

function parseTool(clientApi, response) {
  switch (clientApi) {
    case "openai-responses": {
      const item = (response.output || []).find((candidate) => candidate.type === "function_call");
      return item && {
        responseId: item.id,
        callId: item.call_id || item.id,
        name: item.name,
        arguments: parseArguments(item.arguments),
      };
    }
    case "openai-chat": {
      const tool = response.choices?.[0]?.message?.tool_calls?.[0];
      return tool && {
        callId: tool.id,
        name: tool.function?.name,
        arguments: parseArguments(tool.function?.arguments),
      };
    }
    case "anthropic": {
      const tool = (response.content || []).find((candidate) => candidate.type === "tool_use");
      return tool && { callId: tool.id, name: tool.name, arguments: tool.input || {} };
    }
    case "gemini": {
      const part = response.candidates?.[0]?.content?.parts?.find((candidate) => candidate.functionCall);
      const tool = part?.functionCall;
      return tool && { callId: "call_lookup", name: tool.name, arguments: tool.args || {} };
    }
    default:
      return null;
  }
}

function parseFinalText(clientApi, response) {
  switch (clientApi) {
    case "openai-responses":
      return (response.output || [])
        .flatMap((item) => item.content || [])
        .find((part) => part.type === "output_text")?.text;
    case "openai-chat":
      return response.choices?.[0]?.message?.content;
    case "anthropic":
      return (response.content || []).find((part) => part.type === "text")?.text;
    case "gemini":
      return response.candidates?.[0]?.content?.parts?.find((part) => typeof part.text === "string")?.text;
    default:
      return null;
  }
}

function parseArguments(value) {
  if (!value) return {};
  if (typeof value === "object") return value;
  try {
    return JSON.parse(value);
  } catch {
    return { raw: String(value) };
  }
}

function notifyText(sessionId, text) {
  notify("session/update", {
    sessionId,
    update: {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text },
    },
  });
}

function notifyTool(sessionId, tool) {
  notify("session/update", {
    sessionId,
    update: {
      sessionUpdate: "tool_call",
      toolCallId: tool.callId,
      title: "lookup",
      kind: "fetch",
      status: "in_progress",
      rawInput: tool.arguments || {},
    },
  });
  notify("session/update", {
    sessionId,
    update: {
      sessionUpdate: "tool_call_update",
      toolCallId: tool.callId,
      title: "lookup completed",
      status: "completed",
      rawOutput: { ok: true },
    },
  });
}

function notify(method, params) {
  write({ jsonrpc: "2.0", method, params });
}

function result(id, value) {
  write({ jsonrpc: "2.0", id, result: value });
}

function write(value) {
  process.stdout.write(JSON.stringify(value) + "\n");
}
`;

main().catch((error) => {
  console.error(error?.stack ?? error);
  process.exitCode = 1;
});
