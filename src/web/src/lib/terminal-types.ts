/** Terminal session status. */
export type TerminalStatus = "running" | "idle" | "stopped" | "error";

/** Tool type: determines panel and xterm theme (claude/codex/gemini/opencode/generic). */
export type ToolType = "claude" | "codex" | "gemini" | "opencode" | "generic";

export const TOOL_OPTIONS: ToolType[] = ["generic", "claude", "codex", "gemini", "opencode"];

export interface TerminalSession {
  id: string;
  name: string;
  group: string;
  tool: ToolType;
  status: TerminalStatus;
  command: string;
  cwd: string;
  startedAt: number;
  /** Backend created_at (seconds); optional, for display. */
  createdAt?: number;
  /** If attached to a tmux session, its name. */
  tmuxSession?: string;
}

export interface TerminalGroup {
  id: string;
  label: string;
  color: string;
  sessions: TerminalSession[];
  collapsed?: boolean;
}

export type ViewMode = "tabs" | "grid";

export interface ToolTheme {
  accent: string;
  accentFg: string;
  bg: string;
  headerBg: string;
  borderColor: string;
  label: string;
  cursorColor: string;
  selectionBg: string;
}

export type AppThemeMode = "light" | "dark";

/** Terminal background: only light vs dark (like iTerm), not per-tool. */
const TERMINAL_BG_LIGHT = "#ffffff";
const TERMINAL_BG_DARK = "#0d0d0d";
const TERMINAL_HEADER_BG_LIGHT = "#f5f5f5";
const TERMINAL_HEADER_BG_DARK = "#1a1a1a";

/** Per-tool: accent, label, cursor, selection. bg/headerBg come from app theme only. */
export const toolThemes: Record<ToolType, Omit<ToolTheme, "bg" | "headerBg"> & { bg: string; headerBg: string }> = {
  claude: {
    accent: "#d97706",
    accentFg: "#fef3c7",
    bg: TERMINAL_BG_DARK,
    headerBg: TERMINAL_HEADER_BG_DARK,
    borderColor: "#d9770640",
    label: "Claude",
    cursorColor: "#d97706",
    selectionBg: "#d9770633",
  },
  gemini: {
    accent: "#3b82f6",
    accentFg: "#dbeafe",
    bg: TERMINAL_BG_DARK,
    headerBg: TERMINAL_HEADER_BG_DARK,
    borderColor: "#3b82f640",
    label: "Gemini",
    cursorColor: "#3b82f6",
    selectionBg: "#3b82f633",
  },
  codex: {
    accent: "#10b981",
    accentFg: "#d1fae5",
    bg: TERMINAL_BG_DARK,
    headerBg: TERMINAL_HEADER_BG_DARK,
    borderColor: "#10b98140",
    label: "Codex",
    cursorColor: "#10b981",
    selectionBg: "#10b98133",
  },
  opencode: {
    accent: "#71717a",
    accentFg: "#e4e4e7",
    bg: TERMINAL_BG_DARK,
    headerBg: TERMINAL_HEADER_BG_DARK,
    borderColor: "#71717a40",
    label: "OpenCode",
    cursorColor: "#71717a",
    selectionBg: "#71717a33",
  },
  generic: {
    accent: "#64748b",
    accentFg: "#e2e8f0",
    bg: TERMINAL_BG_DARK,
    headerBg: TERMINAL_HEADER_BG_DARK,
    borderColor: "#64748b40",
    label: "Terminal",
    cursorColor: "#64748b",
    selectionBg: "#64748b33",
  },
};

export function getToolTheme(tool: ToolType, appTheme: AppThemeMode): ToolTheme {
  const t = toolThemes[tool];
  return {
    ...t,
    bg: appTheme === "light" ? TERMINAL_BG_LIGHT : TERMINAL_BG_DARK,
    headerBg: appTheme === "light" ? TERMINAL_HEADER_BG_LIGHT : TERMINAL_HEADER_BG_DARK,
  };
}

export const GROUP_COLOR_PRESETS = [
  { name: "Amber", hex: "#d97706" },
  { name: "Violet", hex: "#8b5cf6" },
  { name: "Cyan", hex: "#06b6d4" },
  { name: "Slate", hex: "#64748b" },
] as const;

export function getGroupColor(hex: string) {
  return {
    bg: hex,
    text: `${hex}cc`,
    ring: `${hex}60`,
    tabBg: `${hex}18`,
    lineBg: hex,
  };
}
