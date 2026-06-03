import type { ChannelVerboseConfig } from "../types";

export function defaultChannelVerbose(): ChannelVerboseConfig {
  return {
    show_thinking: false,
    show_tool_use: false,
  };
}

export function parseChannelVerbose(value: unknown): ChannelVerboseConfig {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return defaultChannelVerbose();
  }
  const verbose = value as Record<string, unknown>;
  return {
    show_thinking:
      typeof verbose.show_thinking === "boolean"
        ? verbose.show_thinking
        : false,
    show_tool_use:
      typeof verbose.show_tool_use === "boolean"
        ? verbose.show_tool_use
        : false,
  };
}
