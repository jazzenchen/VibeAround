"use client";

import { useState, useRef, useEffect, useCallback } from "react";

interface MobileInputBarProps {
  sendInput: (data: string) => void;
}

interface ShortcutBtn {
  label: string;
  data: string;
}

const ROW1: ShortcutBtn[] = [
  { label: "⌃C", data: "\x03" },
  { label: "⌃D", data: "\x04" },
  { label: "↑", data: "\x1b[A" },
  { label: "↓", data: "\x1b[B" },
];

const ROW2: ShortcutBtn[] = [
  { label: "Tab", data: "\t" },
  { label: "Esc", data: "\x1b" },
  { label: "Space", data: " " },
  { label: "Enter", data: "\r" },
];

const BTN_CLASS =
  "flex items-center justify-center rounded-md px-3 h-9 text-[11px] font-mono font-medium select-none active:scale-95 transition-transform touch-manipulation";

const BTN_STYLE = {
  backgroundColor: "oklch(0.20 0.01 260)",
  color: "oklch(0.80 0.005 260)",
  border: "1px solid oklch(0.28 0.01 260)",
};

/**
 * Track visual viewport height so the prompt area adapts when the
 * virtual keyboard opens/closes on mobile.
 */
function useVisualViewportHeight() {
  const [height, setHeight] = useState(() =>
    typeof window !== "undefined"
      ? window.visualViewport?.height ?? window.innerHeight
      : 800
  );

  useEffect(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    const update = () => setHeight(vv.height);
    vv.addEventListener("resize", update);
    vv.addEventListener("scroll", update);
    return () => {
      vv.removeEventListener("resize", update);
      vv.removeEventListener("scroll", update);
    };
  }, []);

  return height;
}

export function MobileInputBar({ sendInput }: MobileInputBarProps) {
  const [promptOpen, setPromptOpen] = useState(false);
  const [promptText, setPromptText] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const viewportHeight = useVisualViewportHeight();

  const handleSend = useCallback(() => {
    const text = promptText.trim();
    if (!text) return;
    sendInput(text + "\r");
    setPromptText("");
    setPromptOpen(false);
  }, [promptText, sendInput]);

  const openPrompt = useCallback(() => {
    setPromptOpen(true);
    // Synchronous focus in click handler → iOS opens keyboard immediately.
    textareaRef.current?.focus();
  }, []);

  const closePrompt = useCallback(() => {
    setPromptOpen(false);
    setPromptText("");
    textareaRef.current?.blur();
  }, []);

  return (
    <>
      {/* ── Two-row shortcut bar ── */}
      <div
        className="shrink-0 flex flex-col gap-1.5 px-2 py-2"
        style={{
          backgroundColor: "oklch(0.12 0.005 260)",
          borderTop: "1px solid oklch(0.22 0.01 260)",
        }}
        onTouchMove={(e) => e.stopPropagation()}
      >
        <div className="flex gap-1.5">
          {ROW1.map((btn) => (
            <button
              key={btn.label}
              type="button"
              className={`${BTN_CLASS} flex-1`}
              style={BTN_STYLE}
              onPointerDown={(e) => {
                e.preventDefault();
                sendInput(btn.data);
              }}
            >
              {btn.label}
            </button>
          ))}
          <button
            type="button"
            className={`${BTN_CLASS} flex-[1.6]`}
            style={{
              backgroundColor: "oklch(0.22 0.04 180)",
              color: "oklch(0.90 0.04 180)",
              border: "1px solid oklch(0.35 0.06 180)",
            }}
            onClick={openPrompt}
          >
            ✍️ Prompt
          </button>
        </div>
        <div className="flex gap-1.5">
          {ROW2.map((btn) => (
            <button
              key={btn.label}
              type="button"
              className={`${BTN_CLASS} flex-1`}
              style={BTN_STYLE}
              onPointerDown={(e) => {
                e.preventDefault();
                sendInput(btn.data);
              }}
            >
              {btn.label}
            </button>
          ))}
        </div>
      </div>

      {/* ── Prompt overlay ──
          The textarea is always in the DOM (height: 0 when closed).
          Clicking Prompt synchronously focuses it → keyboard opens instantly.
          When open, overlay fills the visual viewport above the keyboard. */}
      <div
        className="fixed left-0 right-0 z-50 flex flex-col"
        style={{
          top: 0,
          height: promptOpen ? `${viewportHeight}px` : "0px",
          overflow: "hidden",
          backgroundColor: "oklch(0.10 0.005 260)",
          transition: "height 0.2s ease-out",
          opacity: promptOpen ? 1 : 0,
          pointerEvents: promptOpen ? "auto" : "none",
        }}
      >
        {/* Top bar */}
        <div
          className="flex items-center justify-between px-4 py-3 shrink-0"
          style={{ borderBottom: "1px solid oklch(0.22 0.01 260)" }}
        >
          <span className="text-sm font-mono font-medium text-foreground">
            输入 Prompt
          </span>
          <button
            type="button"
            className="text-xs font-mono text-muted-foreground/60 active:text-foreground px-2 py-1 rounded active:scale-95 transition-transform"
            onClick={closePrompt}
          >
            取消
          </button>
        </div>

        {/* Textarea: always mounted, fills remaining space when open */}
        <div className="flex-1 min-h-0 p-3">
          <textarea
            ref={textareaRef}
            value={promptText}
            onChange={(e) => setPromptText(e.target.value)}
            placeholder="输入要发送到终端的文本…"
            className="w-full h-full resize-none rounded-lg p-3 font-mono text-foreground placeholder:text-muted-foreground/30 focus:outline-none"
            style={{
              fontSize: "16px",
              lineHeight: "1.5",
              backgroundColor: "oklch(0.14 0.01 260)",
              border: "1px solid oklch(0.25 0.01 260)",
              overflowX: "hidden",
              wordBreak: "break-word",
              overflowWrap: "break-word",
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                handleSend();
              }
            }}
          />
        </div>

        {/* Send button */}
        <div
          className="shrink-0 px-3 py-2"
          style={{ borderTop: "1px solid oklch(0.22 0.01 260)" }}
        >
          <button
            type="button"
            className="w-full rounded-lg py-2.5 font-mono font-semibold active:scale-[0.98] transition-transform"
            style={{
              fontSize: "15px",
              backgroundColor: promptText.trim()
                ? "oklch(0.72 0.15 180)"
                : "oklch(0.25 0.01 260)",
              color: promptText.trim()
                ? "oklch(0.13 0.005 260)"
                : "oklch(0.50 0.01 260)",
            }}
            disabled={!promptText.trim()}
            onClick={handleSend}
          >
            发送到终端
          </button>
        </div>
      </div>
    </>
  );
}
