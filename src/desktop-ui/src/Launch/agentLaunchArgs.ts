import type { AgentLaunchPreference } from "./types";

export type CodexSandboxPreset =
  | "default"
  | "read-only"
  | "workspace-write"
  | "danger-full-access";

export type LaunchArgParseError =
  | "danglingEscape"
  | "lineBreak"
  | "unterminatedQuote";

export interface LaunchArgParseResult {
  args: string[];
  error: LaunchArgParseError | null;
}

export function parseLaunchArgInput(input: string): LaunchArgParseResult {
  const source = input.trim();
  if (!source) return { args: [], error: null };
  if (source.includes("\0") || source.includes("\n") || source.includes("\r")) {
    return { args: [], error: "lineBreak" };
  }

  const args: string[] = [];
  let current = "";
  let quote: "'" | '"' | null = null;
  let escaped = false;
  let started = false;

  for (const ch of source) {
    if (quote === "'") {
      if (ch === "'") {
        quote = null;
      } else {
        current += ch;
      }
      started = true;
      continue;
    }

    if (escaped) {
      current += ch;
      escaped = false;
      started = true;
      continue;
    }

    if (ch === "\\") {
      escaped = true;
      started = true;
      continue;
    }

    if (quote === '"') {
      if (ch === '"') {
        quote = null;
      } else {
        current += ch;
      }
      started = true;
      continue;
    }

    if (ch === "'" || ch === '"') {
      quote = ch;
      started = true;
      continue;
    }

    if (/\s/.test(ch)) {
      if (started) {
        args.push(current);
        current = "";
        started = false;
      }
      continue;
    }

    current += ch;
    started = true;
  }

  if (escaped) return { args: [], error: "danglingEscape" };
  if (quote) return { args: [], error: "unterminatedQuote" };
  if (started) args.push(current);

  return { args: args.filter((arg) => arg.trim() !== ""), error: null };
}

export function sameArgs(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((arg, index) => arg === right[index]);
}

export function removeCodexSandboxArgs(args: string[]): string[] {
  const out: string[] = [];
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (
      arg === "--dangerously-bypass-approvals-and-sandbox" ||
      arg.startsWith("--sandbox=") ||
      arg.startsWith("-s=")
    ) {
      continue;
    }
    if (arg === "--sandbox" || arg === "-s") {
      index += 1;
      continue;
    }
    out.push(arg);
  }
  return out;
}

export function inferCodexSandboxPreset(args: string[]): CodexSandboxPreset {
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    const next = args[index + 1];
    if (arg === "--sandbox" || arg === "-s") {
      if (isCodexSandboxMode(next)) return next;
    }
    if (arg.startsWith("--sandbox=") || arg.startsWith("-s=")) {
      const value = arg.slice(arg.indexOf("=") + 1);
      if (isCodexSandboxMode(value)) return value;
    }
  }
  return "default";
}

export function applyCodexSandboxPreset(
  args: string[],
  preset: CodexSandboxPreset,
): string[] {
  const out = removeCodexSandboxArgs(args);
  if (preset === "default") return out;
  out.push("--sandbox", preset);
  return out;
}

export function agentLaunchArgCount(preference?: AgentLaunchPreference): number {
  return preference?.launchArgs?.terminal?.length ?? 0;
}

function isCodexSandboxMode(value: unknown): value is Exclude<CodexSandboxPreset, "default"> {
  return (
    value === "read-only" ||
    value === "workspace-write" ||
    value === "danger-full-access"
  );
}
