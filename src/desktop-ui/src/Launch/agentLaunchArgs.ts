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
